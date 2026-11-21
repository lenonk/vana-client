//! Android loads this crate as `libvana_client.so` (`NativeActivity`'s `android.app.lib_name`).
//! Desktop builds the same crate as a library and a thin `vana-client` binary.

#[cfg(target_os = "android")]
mod android_fs;
mod audio;
mod cabx;
mod client;
mod clothshot;
mod combatshot;
mod config;
mod creatorshot;
mod dat;
mod dialogshot;
mod event_vm;
mod eventdump;
mod flashshot;
mod fx;
mod gamecfg;
mod install;
mod itemshot;
mod mpeg1;
mod padreg;
mod patch;
mod poldic;

/// unrar_sys's build script picks its sources by the host (`cfg!(windows)`), so a MinGW cross build from Linux
/// leaves out UnRAR's isnt.cpp; these are its two functions, under their C++ names.
#[cfg(all(windows, target_env = "gnu"))]
mod unrar_isnt {
    /// `DWORD WinNT()`: Windows 10 (WNT_W10), the oldest this client supports.
    #[unsafe(export_name = "_Z5WinNTv")]
    pub extern "C" fn win_nt() -> u32 {
        0x0a00
    }

    /// `bool IsWindows11OrGreater()`: UnRAR's stub answers false.
    #[unsafe(export_name = "_Z20IsWindows11OrGreaterv")]
    pub extern "C" fn is_windows_11_or_greater() -> bool {
        false
    }
}
mod inventory;
mod invitems;
mod invshot;
mod modpack;
mod motion;
mod motionq;
mod msi_db;
mod net;
mod render;
mod secret;
mod sounddump;
mod texpack;
mod uishot;
mod voice;

include!("entry.rs");

/// NativeActivity calls this once the process is up. The desktop binary calls [`entry`] directly.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Debug));
    if let Some(dir) = app.internal_data_path() {
        // SAFETY: only the android_main thread runs before other Rust code reads these.
        unsafe {
            std::env::set_var("VANA_CLIENT_DATA", &dir);
            std::env::set_var("HOME", &dir);
            // GLES compute culling often leaves every scenery run invisible (actors still draw). Use the unculled path.
            std::env::set_var("FFXI_NO_CULL", "1");
        }
    }
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("panic: {info}");
        default(info);
    }));
    client::set_android_app(app);
    // Coming back from another activity (Settings, the browser) runs android_main again; winit allows one EventLoop.
    if client::android_event_loop_running() {
        client::android_resume_after_activity();
        return;
    }
    if let Err(e) = entry() {
        log::error!("entry: {e:#}");
    }
}
