//! The client language, one per process as retail's: DAT_10457bd8 +8 (0 ja, 1 en, 2 de, 3 fr). PlayOnline hands it
//! to the game in its start-up block (FUN_100157d0 gives the host &DAT_10457b58; FUN_100157e0 copies the block's +0x28,
//! DAT_10457b80, to +8). Ours takes it from config.json's `language` (Config::load sets it).

use super::items::Lang;
use std::sync::atomic::{AtomicU8, Ordering};

static LANG: AtomicU8 = AtomicU8::new(Lang::En as u8);

/// The block's +0x4c bit 0 (DAT_10457ba4 -> DAT_10457bd8 +0x18 bit 7, FUN_100157e0): German and French read the English
/// files and compare as English (FUN_1025cb90, FUN_101b67e0, FUN_101bde00). PlayOnline sets it for an install without
/// the European files; ours has no such install, so it stays clear.
const EUROPEAN_AS_ENGLISH: bool = false;

/// The client language.
pub fn client() -> Lang {
    match LANG.load(Ordering::Relaxed) {
        0 => Lang::Ja,
        2 => Lang::De,
        3 => Lang::Fr,
        _ => Lang::En,
    }
}

pub fn set(l: Lang) {
    LANG.store(l as u8, Ordering::Relaxed);
}

/// The language whose files and word rules apply (the test FUN_1025cb90 and the dictionary's readers make): the
/// client's, but English for German and French while EUROPEAN_AS_ENGLISH.
pub fn files() -> Lang {
    match client() {
        Lang::De | Lang::Fr if EUROPEAN_AS_ENGLISH => Lang::En,
        l => l,
    }
}
