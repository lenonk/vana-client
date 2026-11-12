//! The remembered password (not retail: PlayOnline kept it) and a login server's trust token. Sealed with
//! ChaCha20-Poly1305 under a random key in its file (owner-only, apart from the config, so a shared or copied config
//! carries neither), bound to the server and account it was saved for, and to which of the two it is.

use anyhow::{Context, Result, anyhow};
use ring::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use std::path::PathBuf;

/// The key file: in the data dir, or beside an FFXI_CONFIG copy (test runs keep theirs).
fn key_path() -> PathBuf {
    match std::env::var_os("FFXI_CONFIG") {
        Some(p) => PathBuf::from(p).with_file_name("secret.key"),
        None => crate::config::data_dir().join("secret.key"),
    }
}

fn read_key() -> Option<[u8; 32]> {
    std::fs::read(key_path()).ok()?.try_into().ok()
}

/// The key, made on first use.
fn key() -> Result<[u8; 32]> {
    if let Some(k) = read_key() {
        return Ok(k);
    }
    let mut k = [0u8; 32];
    getrandom::fill(&mut k).map_err(|e| anyhow!("random key: {e}"))?;
    let path = key_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    std::io::Write::write_all(&mut opts.open(&path).with_context(|| format!("writing {}", path.display()))?, &k)?;
    Ok(k)
}

/// What a sealed text is bound to: a password to its server and account (as it always was), a trust token to them
/// under its prefix, so neither opens as the other.
fn aad(token: bool, server: &str, account: &str) -> Vec<u8> {
    let kind = if token { "trust\0" } else { "" };
    format!("{kind}{server}\0{account}").into_bytes()
}

fn seal_with(key: &[u8; 32], aad: Vec<u8>, text: &str) -> Result<String> {
    let k = LessSafeKey::new(UnboundKey::new(&CHACHA20_POLY1305, key).map_err(|_| anyhow!("bad key"))?);
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::fill(&mut nonce).map_err(|e| anyhow!("random nonce: {e}"))?;
    let mut data = text.as_bytes().to_vec();
    k.seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce), Aad::from(aad), &mut data)
        .map_err(|_| anyhow!("sealing failed"))?;
    Ok(hex::encode([&nonce[..], &data].concat()))
}

fn open_with(key: &[u8; 32], aad: Vec<u8>, sealed: &str) -> Option<String> {
    let raw = hex::decode(sealed).ok()?;
    if raw.len() < NONCE_LEN {
        return None;
    }
    let (nonce, data) = raw.split_at(NONCE_LEN);
    let k = LessSafeKey::new(UnboundKey::new(&CHACHA20_POLY1305, key).ok()?);
    let mut data = data.to_vec();
    let plain = k.open_in_place(Nonce::try_assume_unique_for_key(nonce).ok()?, Aad::from(aad), &mut data).ok()?;
    String::from_utf8(plain.to_vec()).ok()
}

/// The password sealed for this server and account (hex: nonce, then the ciphertext and its tag).
pub fn seal(server: &str, account: &str, password: &str) -> Result<String> {
    seal_with(&key()?, aad(false, server, account), password)
}

/// The remembered password, if it was sealed for this server and account with this computer's key.
pub fn open(server: &str, account: &str, sealed: &str) -> Option<String> {
    if sealed.is_empty() {
        return None;
    }
    open_with(&read_key()?, aad(false, server, account), sealed)
}

/// A trust token sealed for this server and account, as `seal`.
pub fn seal_token(server: &str, account: &str, token: &str) -> Result<String> {
    seal_with(&key()?, aad(true, server, account), token)
}

/// The trust token kept for this server and account, as `open`.
pub fn open_token(server: &str, account: &str, sealed: &str) -> Option<String> {
    if sealed.is_empty() {
        return None;
    }
    open_with(&read_key()?, aad(true, server, account), sealed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sealed password opens only for the same key, server and account.
    #[test]
    fn round_trip() {
        let key = [7u8; 32];
        let at = |server, account| aad(false, server, account);
        let s = seal_with(&key, at("127.0.0.1", "lenon"), "hunter2").unwrap();
        assert!(!s.contains("hunter2"));
        assert_eq!(open_with(&key, at("127.0.0.1", "lenon"), &s).as_deref(), Some("hunter2"));
        assert_eq!(open_with(&key, at("127.0.0.1", "other"), &s), None);
        assert_eq!(open_with(&key, at("10.0.0.1", "lenon"), &s), None);
        assert_eq!(open_with(&[8u8; 32], at("127.0.0.1", "lenon"), &s), None);
        // A fresh nonce each time.
        assert_ne!(seal_with(&key, at("127.0.0.1", "lenon"), "hunter2").unwrap(), s);
    }

    /// A trust token and a password, sealed for the same server and account, never open as each other.
    #[test]
    fn token_apart() {
        let key = [7u8; 32];
        let token = seal_with(&key, aad(true, "127.0.0.1", "lenon"), "abc123").unwrap();
        let password = seal_with(&key, aad(false, "127.0.0.1", "lenon"), "hunter2").unwrap();
        assert_eq!(open_with(&key, aad(true, "127.0.0.1", "lenon"), &token).as_deref(), Some("abc123"));
        assert_eq!(open_with(&key, aad(false, "127.0.0.1", "lenon"), &token), None);
        assert_eq!(open_with(&key, aad(true, "127.0.0.1", "lenon"), &password), None);
    }
}
