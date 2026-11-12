//! The screen's physical size, from its EDID (not retail), and the UI sizes a first start picks from it: retail's
//! Menu Size (0037 / 0038) and the modern layer's Interface Size, so a small screen does not start with menus too small
//! to use before Settings can be reached.

use crate::config::Config;
use winit::monitor::MonitorHandle;

/// The reference: a 24-inch 1080-line screen (299 mm tall), where retail's menus at 1:1 and the modern layer at 60%
/// read well.
const REF_MM: f32 = 299.0;
const REF_LINES: f32 = 1080.0;
const REF_SCALE: f32 = 60.0;

/// What an EDID says of its screen: the image size in mm (the panel's way round) and the preferred resolution.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edid {
    pub mm: [f32; 2],
    pub native: Option<[u32; 2]>,
}

/// An EDID base block: the first detailed timing's image size (mm) and active pixels, else bytes 21 / 22 (cm). None
/// without a size (a projector, or bytes 21 / 22 giving only an aspect ratio).
pub fn parse(b: &[u8]) -> Option<Edid> {
    if b.len() < 128 || b[..8] != [0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0] {
        return None;
    }
    let t = &b[54..72];
    let timing = u16::from_le_bytes([t[0], t[1]]) != 0;
    let native = timing.then(|| [t[2] as u32 | (t[4] as u32 >> 4) << 8, t[5] as u32 | (t[7] as u32 >> 4) << 8]);
    let sized = |m: &[u32; 2]| m[0] >= 40 && m[1] >= 40;
    let dtd = timing
        .then(|| [t[12] as u32 | (t[14] as u32 >> 4) << 8, t[13] as u32 | (t[14] as u32 & 0xf) << 8])
        .filter(sized);
    let mm = dtd.or(Some([b[21] as u32 * 10, b[22] as u32 * 10]).filter(sized))?;
    Some(Edid { mm: mm.map(|v| v as f32), native })
}

/// The monitor's physical size in mm as it is turned now (width, height), when its EDID gives one that fits its
/// resolution's shape.
pub fn physical_mm(m: &MonitorHandle) -> Option<[f32; 2]> {
    let px = m.size();
    let [w, h] = edid_of(m)?.mm;
    let mm = if (px.width > px.height) == (w > h) { [w, h] } else { [h, w] };
    // An odd shape for its pixels is a made-up size (some TVs and capture devices).
    let shape = (mm[0] / mm[1]) / (px.width as f32 / px.height.max(1) as f32);
    (0.75..=1.33).contains(&shape).then_some(mm)
}

/// Linux: the connected connectors' EDIDs in sysfs; the monitor's by its name (Wayland names outputs as their
/// connectors, HDMI-A-1; X11 drops HDMI's "-A"), else the one at its resolution, else the only one.
#[cfg(target_os = "linux")]
fn edid_of(m: &MonitorHandle) -> Option<Edid> {
    let mut found: Vec<(String, Edid)> = Vec::new();
    for e in std::fs::read_dir("/sys/class/drm").ok()?.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some((card, conn)) = name.split_once('-') else { continue };
        let dir = e.path();
        let connected = std::fs::read_to_string(dir.join("status")).is_ok_and(|s| s.trim() == "connected");
        if !card.starts_with("card") || !connected {
            continue;
        }
        if let Some(edid) = std::fs::read(dir.join("edid")).ok().as_deref().and_then(parse) {
            found.push((conn.to_string(), edid));
        }
    }
    pick(&found, &m.name().unwrap_or_default(), [m.size().width, m.size().height])
}

/// Of the connected connectors' EDIDs, the monitor's: by its name, else the one at its resolution, else the only one.
/// A name match must also be at the monitor's resolution when another connector is: NVIDIA's X11 driver numbers its
/// outputs from 0 (DP-0, HDMI-0) and the kernel from 1, so a name can be another port's, and two cards can have the
/// same connector names.
#[cfg(any(target_os = "linux", test))]
fn pick(found: &[(String, Edid)], name: &str, px: [u32; 2]) -> Option<Edid> {
    let norm = |s: &str| s.replace("HDMI-A-", "HDMI-").to_ascii_lowercase();
    let want = norm(name);
    let at = |e: &Edid| e.native.is_some_and(|n| n == px || n == [px[1], px[0]]);
    let named: Vec<Edid> = found.iter().filter(|(c, _)| norm(c) == want).map(|f| f.1).collect();
    let at_res: Vec<Edid> = found.iter().map(|f| f.1).filter(at).collect();
    named
        .iter()
        .copied()
        .find(at)
        .or_else(|| (named.len() == 1 && at_res.is_empty()).then(|| named[0]))
        .or_else(|| (at_res.len() == 1).then(|| at_res[0]))
        .or_else(|| (found.len() == 1).then(|| found[0].1))
}

/// Windows: the EDID in the monitor's registry key, found through its device interface (\\?\DISPLAY#id#instance#{..}).
#[cfg(windows)]
fn edid_of(m: &MonitorHandle) -> Option<Edid> {
    use windows_sys::Win32::Graphics::Gdi::{DISPLAY_DEVICEW, EnumDisplayDevicesW};
    use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_BINARY, RegGetValueW};
    use windows_sys::Win32::UI::WindowsAndMessaging::EDD_GET_DEVICE_INTERFACE_NAME;
    let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let device = wide(&m.name()?);
    let mut dd = DISPLAY_DEVICEW { cb: size_of::<DISPLAY_DEVICEW>() as u32, ..Default::default() };
    if unsafe { EnumDisplayDevicesW(device.as_ptr(), 0, &mut dd, EDD_GET_DEVICE_INTERFACE_NAME) } == 0 {
        return None;
    }
    let id = String::from_utf16_lossy(&dd.DeviceID[..dd.DeviceID.iter().position(|&c| c == 0).unwrap_or(128)]);
    let parts: Vec<&str> = id.split('#').collect();
    let key =
        wide(&format!(r"SYSTEM\CurrentControlSet\Enum\DISPLAY\{}\{}\Device Parameters", parts.get(1)?, parts.get(2)?));
    let (value, mut buf, mut len) = (wide("EDID"), vec![0u8; 1024], 1024u32);
    let r = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_BINARY,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut len,
        )
    };
    if r != 0 {
        return None;
    }
    buf.truncate(len as usize);
    parse(&buf)
}

/// macOS: CoreGraphics' size of the display (from its EDID).
#[cfg(target_os = "macos")]
fn edid_of(m: &MonitorHandle) -> Option<Edid> {
    use winit::platform::macos::MonitorHandleExtMacOS;
    #[repr(C)]
    struct CgSize {
        width: f64,
        height: f64,
    }
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGDisplayScreenSize(display: u32) -> CgSize;
    }
    let s = unsafe { CGDisplayScreenSize(m.native_id()) };
    (s.width >= 40.0 && s.height >= 40.0).then(|| Edid { mm: [s.width as f32, s.height as f32], native: None })
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
fn edid_of(_: &MonitorHandle) -> Option<Edid> {
    None
}

/// A first start's UI sizes.
#[derive(Debug, PartialEq)]
pub struct FirstSizes {
    /// Menu Size (0037 / 0038): one of Settings' steps of the window, None for the window's.
    pub menu: Option<[u32; 2]>,
    /// Interface Size, percent.
    pub interface: u32,
}

/// The UI sizes for a screen `height_mm` tall (None: no EDID size) and `height_px` lines at the system's `scale`, with
/// the window `window` big. A smaller screen is seen from nearer, so its UI grows by the root of how much smaller it is
/// than the reference: retail's menus to UI pixels of that size (as the system's scale says without a size), the
/// modern layer to that (over the system's scale, which it follows anyway), neither below its default.
pub fn first_sizes(height_mm: Option<f32>, height_px: u32, scale: f32, window: [u32; 2]) -> FirstSizes {
    let grow = height_mm.map(|h| (REF_MM / h).sqrt());
    let f = match grow {
        Some(g) => REF_LINES / (height_px.max(1) as f32 * g),
        None => 1.0 / scale.max(1.0),
    };
    let interface = grow.map_or(REF_SCALE, |g| (REF_SCALE * g / scale.max(0.5) / 5.0).round() * 5.0);
    FirstSizes { menu: menu_step(f, window), interface: interface.clamp(REF_SCALE, 150.0) as u32 }
}

/// Settings' Menu Size step nearest `f` of the window (config::ui_steps); None (the window's) from 97.5 % up.
fn menu_step(f: f32, window: [u32; 2]) -> Option<[u32; 2]> {
    if f >= 0.975 {
        return None;
    }
    crate::config::ui_steps(window).into_iter().min_by(|a, b| (a.0 - f).abs().total_cmp(&(b.0 - f).abs())).map(|s| s.1)
}

/// A new config's UI sizes from the monitor it opens on.
pub fn size_ui(cfg: &mut Config, m: &MonitorHandle) {
    let (px, mm, scale) = (m.size(), physical_mm(m), m.scale_factor() as f32);
    let s = first_sizes(mm.map(|v| v[1]), px.height, scale, cfg.window_size());
    if let Some([w, h]) = s.menu {
        cfg.set_tool("0037", w);
        cfg.set_tool("0038", h);
    }
    cfg.modern_scale = s.interface;
    log::info!(
        "first start: {} {}x{} px, {mm:?} mm, scale {scale:.2}: Menu Size {:?}, Interface Size {}%",
        m.name().unwrap_or_default(),
        px.width,
        px.height,
        s.menu,
        s.interface
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An EDID base block with a first detailed timing of `px` and `mm`, and bytes 21 / 22 of `cm`.
    fn block(px: [u32; 2], mm: [u32; 2], cm: [u8; 2]) -> Vec<u8> {
        let mut b = vec![0u8; 128];
        b[..8].copy_from_slice(&[0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0]);
        (b[21], b[22]) = (cm[0], cm[1]);
        let t = &mut b[54..72];
        (t[0], t[1]) = (0x02, 0x3a);
        (t[2], t[4]) = (px[0] as u8, ((px[0] >> 8) as u8) << 4);
        (t[5], t[7]) = (px[1] as u8, ((px[1] >> 8) as u8) << 4);
        (t[12], t[13], t[14]) = (mm[0] as u8, mm[1] as u8, ((mm[0] >> 8) as u8) << 4 | (mm[1] >> 8) as u8);
        b
    }

    /// The timing's mm and pixels; bytes 21 / 22 when the timing has no size; none for an aspect-only screen.
    #[test]
    fn edid_sizes() {
        let dell = parse(&block([1920, 1080], [527, 296], [53, 30])).unwrap();
        assert_eq!(dell, Edid { mm: [527.0, 296.0], native: Some([1920, 1080]) });
        assert_eq!(parse(&block([2560, 1440], [0, 0], [70, 40])).unwrap().mm, [700.0, 400.0]);
        assert_eq!(parse(&block([1920, 1080], [0, 0], [16, 0])), None);
        assert_eq!(parse(&[0u8; 128]), None);
    }

    /// The reference screen keeps the defaults; a small one grows both, a dense desktop retail's menus only; with no
    /// size, retail's menus follow the system's scale.
    #[test]
    fn sizes_by_screen() {
        let window = [1280, 800];
        assert_eq!(first_sizes(Some(296.0), 1080, 1.0, window), FirstSizes { menu: None, interface: 60 });
        // A 7-inch 1280x800 handheld: menus at 75 % (600 lines), the modern layer at 105 %.
        assert_eq!(first_sizes(Some(94.0), 800, 1.0, window), FirstSizes { menu: Some([960, 600]), interface: 105 });
        // A 13-inch 1080-line laptop: 75 % and 80 %; at the system's 125 % the modern layer takes 65.
        assert_eq!(first_sizes(Some(165.0), 1080, 1.0, window).menu, Some([960, 600]));
        assert_eq!(first_sizes(Some(165.0), 1080, 1.0, window).interface, 80);
        assert_eq!(first_sizes(Some(165.0), 1080, 1.25, window).interface, 65);
        // A 27-inch 4K screen at 150 % in a 4K window: retail's 1:1 menus are tiny there; the modern layer keeps 60.
        // (A first start's 1280x800 window stops at 65 %, its last step of 512 lines or more.)
        assert_eq!(
            first_sizes(Some(336.0), 2160, 1.5, [3840, 2160]),
            FirstSizes { menu: Some([2112, 1188]), interface: 60 }
        );
        assert_eq!(first_sizes(None, 1600, 2.0, [2560, 1600]).menu, Some([1280, 800]));
        assert_eq!(first_sizes(None, 1080, 1.0, window), FirstSizes { menu: None, interface: 60 });
        assert_eq!(first_sizes(Some(336.0), 2160, 1.5, window).menu, Some([832, 520]));
    }

    /// The monitor's EDID: by name, unless the name is another port's (NVIDIA's X11 numbering) while a connector is
    /// at the monitor's resolution; by resolution; the only one.
    #[test]
    fn edid_picked() {
        let e = |w: u32, h: u32, mm: f32| Edid { mm: [mm, mm * h as f32 / w as f32], native: Some([w, h]) };
        let (a, b) = (e(2560, 1440, 698.0), e(1920, 1080, 527.0));
        let found = vec![("DP-1".to_string(), a), ("DP-2".to_string(), b)];
        assert_eq!(pick(&found, "DP-1", [2560, 1440]), Some(a));
        // X11 on NVIDIA calls the 1080p monitor DP-1: the kernel's DP-1 is the 1440p one.
        assert_eq!(pick(&found, "DP-1", [1920, 1080]), Some(b));
        // Not at its native resolution and no other connector is: the name still counts.
        assert_eq!(pick(&found, "DP-2", [1280, 720]), Some(b));
        assert_eq!(pick(&found, "DisplayPort-0", [2560, 1440]), Some(a));
        assert_eq!(pick(&[("HDMI-A-1".to_string(), b)], "HDMI-1", [1280, 720]), Some(b));
        assert_eq!(pick(&found, "DisplayPort-0", [1280, 720]), None);
    }
}
