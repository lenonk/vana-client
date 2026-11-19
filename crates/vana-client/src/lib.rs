//! The client as a library; the `vana-client` binary (main.rs) calls [`entry`].

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
