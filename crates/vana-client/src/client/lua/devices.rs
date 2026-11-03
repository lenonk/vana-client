//! HID devices for mods (the `devices` permission): G-keys, Stream Decks and other vendor devices by their raw reports.
//! A device is read on a thread; its reports reach the mod's handlers in the next mod phase, as a socket's data does.

use super::api::{need, next_id, state};
use super::sock::{Kind, Msg};
use mlua::{Lua, Table};
use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

/// One HID interface: where it opens, its ids and product name, its USB interface, its report descriptor.
struct Dev {
    path: String,
    vendor: u16,
    product: u16,
    name: String,
    interface: Option<i32>,
    descriptor: Vec<u8>,
}

/// Linux: the hidraw nodes, from sysfs (HID_ID=bus:vendor:product, HID_NAME).
#[cfg(target_os = "linux")]
fn list() -> Vec<Dev> {
    let mut out = Vec::new();
    for e in std::fs::read_dir("/sys/class/hidraw").into_iter().flatten().flatten() {
        let node = e.file_name().to_string_lossy().into_owned();
        let uevent = std::fs::read_to_string(e.path().join("device/uevent")).unwrap_or_default();
        let get = |k: &str| uevent.lines().find_map(|l| l.strip_prefix(k)).map(str::to_string);
        let id = get("HID_ID=").unwrap_or_default();
        let parts: Vec<&str> = id.split(':').collect();
        let hex = |k: usize| parts.get(k).and_then(|s| u32::from_str_radix(s, 16).ok()).unwrap_or(0) as u16;
        out.push(Dev {
            path: format!("/dev/{node}"),
            vendor: hex(1),
            product: hex(2),
            name: get("HID_NAME=").unwrap_or_default(),
            interface: std::fs::read_to_string(e.path().join("device/../bInterfaceNumber")).ok().and_then(|s| i32::from_str_radix(s.trim(), 16).ok()),
            descriptor: std::fs::read(e.path().join("device/report_descriptor")).unwrap_or_default(),
        });
    }
    let num = |d: &Dev| d.path.trim_start_matches(|c: char| !c.is_ascii_digit()).parse::<u32>().unwrap_or(0);
    out.sort_by_key(num);
    out
}

/// Windows: the HID class's interfaces (SetupAPI), each opened for its attributes and product string.
#[cfg(windows)]
fn list() -> Vec<Dev> {
    use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;
    use windows_sys::Win32::Devices::HumanInterfaceDevice::*;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
    let mut out = Vec::new();
    // SAFETY: SetupAPI and HID calls on buffers sized as the calls ask; every handle is closed.
    unsafe {
        let mut guid = std::mem::zeroed();
        HidD_GetHidGuid(&mut guid);
        let set = SetupDiGetClassDevsW(&guid, std::ptr::null(), std::ptr::null_mut(), DIGCF_PRESENT | DIGCF_DEVICEINTERFACE);
        if set == INVALID_HANDLE_VALUE as HDEVINFO {
            return out;
        }
        for i in 0.. {
            let mut data: SP_DEVICE_INTERFACE_DATA = std::mem::zeroed();
            data.cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32;
            if SetupDiEnumDeviceInterfaces(set, std::ptr::null(), &guid, i, &mut data) == 0 {
                break;
            }
            let mut need = 0u32;
            SetupDiGetDeviceInterfaceDetailW(set, &data, std::ptr::null_mut(), 0, &mut need, std::ptr::null_mut());
            if need == 0 {
                continue;
            }
            let mut buf = vec![0u64; (need as usize).div_ceil(8)];
            let detail = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
            (*detail).cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
            if SetupDiGetDeviceInterfaceDetailW(set, &data, detail, need, std::ptr::null_mut(), std::ptr::null_mut()) == 0 {
                continue;
            }
            let p = std::ptr::addr_of!((*detail).DevicePath) as *const u16;
            let len = (0..).take_while(|&k| *p.add(k) != 0).count();
            let wide = std::slice::from_raw_parts(p, len + 1);
            let path = String::from_utf16_lossy(&wide[..len]);
            let mut dev = Dev { path, vendor: 0, product: 0, name: String::new(), interface: None, descriptor: Vec::new() };
            let h = CreateFileW(wide.as_ptr(), 0, FILE_SHARE_READ | FILE_SHARE_WRITE, std::ptr::null(), OPEN_EXISTING, 0, std::ptr::null_mut());
            if h != INVALID_HANDLE_VALUE {
                let mut attrs: HIDD_ATTRIBUTES = std::mem::zeroed();
                attrs.Size = std::mem::size_of::<HIDD_ATTRIBUTES>() as u32;
                if HidD_GetAttributes(h, &mut attrs) {
                    (dev.vendor, dev.product) = (attrs.VendorID, attrs.ProductID);
                }
                let mut name = [0u16; 128];
                if HidD_GetProductString(h, name.as_mut_ptr() as *mut _, (name.len() * 2) as u32) {
                    dev.name = String::from_utf16_lossy(&name[..name.iter().position(|&c| c == 0).unwrap_or(name.len())]);
                }
                CloseHandle(h);
            }
            dev.interface = dev.path.to_ascii_lowercase().split("&mi_").nth(1).and_then(|s| i32::from_str_radix(s.get(..2)?, 16).ok());
            out.push(dev);
        }
        SetupDiDestroyDeviceInfoList(set);
    }
    out
}

#[cfg(not(any(target_os = "linux", windows)))]
fn list() -> Vec<Dev> {
    Vec::new()
}

/// Waits up to 200 ms for a report (Unix), so a closed device's reader notices the stop.
#[cfg(unix)]
fn ready(f: &std::fs::File) -> bool {
    use std::os::fd::AsRawFd;
    let mut p = libc::pollfd { fd: f.as_raw_fd(), events: libc::POLLIN, revents: 0 };
    unsafe { libc::poll(&mut p, 1, 200) > 0 }
}

#[cfg(not(unix))]
fn ready(_: &std::fs::File) -> bool {
    true
}

/// The device's reader and writer threads: one report a read, until it fails or the mod closes it.
fn run(id: u64, path: &str, report: Sender<Msg>, out: Receiver<Vec<u8>>, stop: Arc<AtomicBool>) -> std::io::Result<()> {
    let mut f = std::fs::OpenOptions::new().read(true).write(true).open(path)?;
    let mut w = f.try_clone()?;
    std::thread::Builder::new().name("lua-hid-w".into()).spawn(move || {
        for b in out {
            if w.write_all(&b).is_err() {
                return;
            }
        }
    })?;
    std::thread::Builder::new().name("lua-hid-r".into()).spawn(move || {
        let mut buf = vec![0u8; 4096];
        while !stop.load(Ordering::Relaxed) {
            if !ready(&f) {
                continue;
            }
            match f.read(&mut buf) {
                Ok(0) => {}
                Ok(n) => {
                    if report.send(Msg::Data(id, buf[..n].to_vec(), None)).is_err() {
                        return;
                    }
                }
                Err(e) => {
                    let _ = report.send(Msg::Closed(id, Some(e.to_string())));
                    return;
                }
            }
        }
    })?;
    Ok(())
}

pub(super) fn install(lua: &Lua, g: &Table) -> mlua::Result<()> {
    let t = lua.create_table()?;
    t.set(
        "list",
        lua.create_function(|lua, ()| {
            need(lua, "devices")?;
            let out = lua.create_table()?;
            for d in list() {
                let e = lua.create_table()?;
                (e.set("path", d.path)?, e.set("vendor", d.vendor)?, e.set("product", d.product)?, e.set("name", d.name)?);
                (e.set("interface", d.interface)?, e.set("descriptor", lua.create_string(&d.descriptor)?)?);
                out.push(e)?;
            }
            Ok(out)
        })?,
    )?;
    t.set(
        "open",
        lua.create_function(|lua, (path, handlers): (String, Table)| {
            need(lua, "devices")?;
            // Only a HID device: the permission is not a way to any file.
            if !list().iter().any(|d| d.path == path) {
                return Err(mlua::Error::runtime(format!("{path}: not a HID device (devices.list gives them)")));
            }
            let id = next_id(&state(lua));
            let report = super::sock::reports(lua);
            let (tx, rx) = channel::<Vec<u8>>();
            let stop = Arc::new(AtomicBool::new(false));
            let _ = report.send(match run(id, &path, report.clone(), rx, stop.clone()) {
                Ok(()) => Msg::Open(id, Box::new(|| {})),
                Err(e) => Msg::Closed(id, Some(format!("{path}: {e}"))),
            });
            super::sock::keep(lua, id, handlers, Kind::Tcp { tx, stream: None, stop })?;
            Ok(id)
        })?,
    )?;
    t.set(
        "write",
        lua.create_function(|lua, (id, data): (u64, mlua::LuaString)| {
            need(lua, "devices")?;
            let st = state(lua);
            let s = st.borrow();
            match s.socks.iter().find(|x| x.id == id).map(|x| &x.kind) {
                Some(Kind::Tcp { tx, .. }) => tx.send(data.as_bytes().to_vec()).map_err(|_| mlua::Error::runtime("the device is closed")),
                _ => Err(mlua::Error::runtime(format!("no device {id}"))),
            }
        })?,
    )?;
    t.set("close", lua.create_function(|lua, id: u64| super::sock::close(lua, id))?)?;
    g.set("devices", t)?;
    Ok(())
}
