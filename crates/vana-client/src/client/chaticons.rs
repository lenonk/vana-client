//! Inline chat icons (chat.icon, ours): images mods register, each a plane-15 private-use character in a line's
//! text, drawn where the character stands by the classic game-font text and the modern chat log.

use std::sync::{Arc, Mutex};

pub const BASE: u32 = 0xF0000;
const MAX: usize = 0x1000;

pub struct Icon {
    /// The mod and file it came from (registered once).
    key: String,
    pub w: u32,
    pub h: u32,
    pub rgba: Arc<Vec<u8>>,
}

static ICONS: Mutex<Vec<Icon>> = Mutex::new(Vec::new());

/// The character for an image, registered under `key` the first time.
pub fn add(key: &str, w: u32, h: u32, rgba: Vec<u8>) -> Option<char> {
    let mut all = ICONS.lock().ok()?;
    let k = match all.iter().position(|i| i.key == key) {
        Some(k) => k,
        None if all.len() < MAX => {
            all.push(Icon { key: key.into(), w, h, rgba: Arc::new(rgba) });
            all.len() - 1
        }
        None => return None,
    };
    char::from_u32(BASE + k as u32)
}

/// An icon character's index.
pub fn index(c: char) -> Option<usize> {
    let k = (c as u32).checked_sub(BASE)? as usize;
    (k < MAX).then_some(k)
}

/// An icon's size and pixels.
pub fn get(k: usize) -> Option<(u32, u32, Arc<Vec<u8>>)> {
    ICONS.lock().ok()?.get(k).map(|i| (i.w, i.h, i.rgba.clone()))
}

/// The texture name an icon is uploaded under.
pub fn texture(k: usize) -> String {
    format!("\0chaticon {k}")
}
