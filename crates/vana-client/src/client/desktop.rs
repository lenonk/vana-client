//! Desktop integration on Linux (not retail): the client's copy in the data dir, the icon and the .desktop entry.
//! Wayland ignores window icons; the compositor finds ours through the app id ("vana-client") and an entry of that
//! name, so the entry is always written, hidden from menus (NoDisplay) unless a menu entry was asked for.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

const ICON_512: &[u8] = include_bytes!("../../../../assets/vana-client-512.png");
const ICON_256: &[u8] = include_bytes!("../../assets/icon-256.png");

/// Whether this platform gets the menu / desktop questions.
pub fn supported() -> bool {
    cfg!(target_os = "linux")
}

fn data_home() -> Result<PathBuf> {
    dirs::data_dir().context("no data directory")
}

/// The client's copy, which the entries start.
fn home_exe() -> PathBuf {
    crate::config::data_dir().join("vana-client")
}

/// The version of the build at home_exe, written whenever one is put there.
fn home_version() -> PathBuf {
    crate::config::data_dir().join("vana-client.version")
}

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Whether the running build takes the copy's place: only a newer one. A copy with no recorded version was put there
/// before versions were recorded, by an older build.
fn replaces(copy: Option<&str>, running: &str) -> bool {
    copy.is_none_or(|c| super::updater::newer(running, c.trim()))
}

/// This binary at home_exe, through a .part file, and its version beside it.
fn put(exe: &Path, home: &Path) -> Result<()> {
    let part = home.with_extension("part");
    std::fs::copy(exe, &part)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&part, home)?;
    std::fs::write(home_version(), VERSION)?;
    Ok(())
}

/// At every start: with no copy in the data dir yet, this binary is put there and the entries point at it (the menu
/// keeps an entry's shown / hidden; shown when there was none, as the first-start page's default). A newer build run
/// from elsewhere (or updated there) replaces the copy; a link put there by hand is left alone.
pub fn settle() -> Result<()> {
    let home = home_exe();
    let exe = super::updater::exe().context("no executable path")?;
    if home.exists() {
        let copy = std::fs::read_to_string(home_version()).ok();
        if home.is_symlink() {
            return Ok(());
        }
        // The copy itself, perhaps just updated in place: its recorded version follows it.
        if std::fs::canonicalize(&exe).ok() == std::fs::canonicalize(&home).ok() {
            if copy.as_deref().map(str::trim) != Some(VERSION) {
                std::fs::write(home_version(), VERSION)?;
            }
            return Ok(());
        }
        if replaces(copy.as_deref(), VERSION) {
            put(&exe, &home)?;
            log::info!("updated {} to {VERSION} from {}", home.display(), exe.display());
        }
        return Ok(());
    }
    std::fs::create_dir_all(home.parent().context("no data directory")?)?;
    put(&exe, &home)?;
    let menu = std::fs::read_to_string(data_home()?.join("applications/vana-client.desktop"))
        .map_or(true, |t| !t.contains("NoDisplay=true"));
    install(menu)?;
    if dirs::desktop_dir().is_some_and(|d| d.join("vana-client.desktop").exists()) {
        desktop_shortcut()?;
    }
    log::info!("installed {} from {}", home.display(), exe.display());
    Ok(())
}

/// A path as a quoted Exec argument (the desktop entry spec's escapes: `\\\\` is one backslash).
fn quoted(p: &Path) -> String {
    let mut s = String::from("\"");
    for c in p.to_string_lossy().chars() {
        match c {
            '"' | '`' | '$' => s.extend(['\\', c]),
            '\\' => s.push_str("\\\\\\\\"),
            '%' => s.push_str("%%"),
            _ => s.push(c),
        }
    }
    s.push('"');
    s
}

fn entry(menu: bool) -> Result<String> {
    let home = home_exe();
    let exe = if home.is_file() { home } else { std::env::current_exe()? };
    Ok(format!(
        "[Desktop Entry]\nType=Application\nName=vana-client\nGenericName=FINAL FANTASY XI client\nComment=Native FINAL FANTASY XI client \
         for LandSandBoat servers\nExec={}\nIcon=vana-client\nTerminal=false\nCategories=Game;RolePlaying;\nStartupWMClass=vana-client\n{}",
        quoted(&exe),
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

    /// Only a newer build replaces the copy; one with no recorded version is older than any build that records them.
    #[test]
    fn newer_builds_replace_the_copy() {
        assert!(super::replaces(None, "1.0.6"));
        assert!(super::replaces(Some("1.0.5\n"), "1.0.6"));
        assert!(!super::replaces(Some("1.0.6"), "1.0.6"));
        assert!(!super::replaces(Some("1.0.7"), "1.0.6"));
        assert!(super::replaces(Some("1.1.0-rc1"), "1.1.0"));
    }

    #[test]
    fn exec_escapes() {
        let q = super::quoted(std::path::Path::new("/home/a b/$x\\y/100%/\"z\""));
        assert_eq!(q, r#""/home/a b/\$x\\\\y/100%%/\"z\"""#);
    }
}
