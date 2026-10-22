//! Desktop integration on Linux (not retail): the icon and the .desktop entry. Wayland ignores window icons; the
//! compositor finds ours through the app id ("vana-client") and an entry of that name, so the entry is always
//! written, hidden from menus (NoDisplay) unless a menu entry was asked for.

use anyhow::{Context, Result};
use std::path::PathBuf;

const ICON_512: &[u8] = include_bytes!("../../../../assets/vana-client-512.png");
const ICON_256: &[u8] = include_bytes!("../../assets/icon-256.png");

/// Whether this platform gets the menu / desktop questions.
pub fn supported() -> bool {
    cfg!(target_os = "linux")
}

fn data_home() -> Result<PathBuf> {
    dirs::data_dir().context("no data directory")
}

fn entry(menu: bool) -> Result<String> {
    let exe = std::env::current_exe()?;
    Ok(format!(
        "[Desktop Entry]\nType=Application\nName=vana-client\nGenericName=FINAL FANTASY XI client\nComment=Native FINAL FANTASY XI client \
         for LandSandBoat servers\nExec=\"{}\"\nIcon=vana-client\nTerminal=false\nCategories=Game;RolePlaying;\nStartupWMClass=vana-client\n{}",
        exe.display(),
        if menu { "" } else { "NoDisplay=true\n" }
    ))
}

/// The icons (256 and 512) and the application entry, shown in menus when `menu`.
pub fn install(menu: bool) -> Result<()> {
    let data = data_home()?;
    for (size, png) in [(256, ICON_256), (512, ICON_512)] {
        let dir = data.join(format!("icons/hicolor/{size}x{size}/apps"));
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("vana-client.png"), png)?;
    }
    let apps = data.join("applications");
    std::fs::create_dir_all(&apps)?;
    std::fs::write(apps.join("vana-client.desktop"), entry(menu)?)?;
    Ok(())
}

/// A launcher on the desktop: executable, and trusted where GNOME's gio is there to mark it.
pub fn desktop_shortcut() -> Result<()> {
    let dir = dirs::desktop_dir().context("no desktop directory")?;
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("vana-client.desktop");
    std::fs::write(&path, entry(true)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    }
    let _ = std::process::Command::new("gio").arg("set").arg(&path).arg("metadata::trusted").arg("true").output();
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn hidden_unless_in_the_menu() {
        let shown = super::entry(true).unwrap();
        let hidden = super::entry(false).unwrap();
        assert!(shown.contains("StartupWMClass=vana-client") && !shown.contains("NoDisplay"));
        assert!(hidden.ends_with("NoDisplay=true\n"));
        assert!(shown.contains("Exec=\""));
    }
}
