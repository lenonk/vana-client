//! xi_connect's auth service (TCP 54231, TLS 1.3, one JSON request and reply per connection).
//! Speaks the protocol of xiloader 3.0.x, which the server checks for; a server that names another version is asked
//! again as that one, and asked as it from then on. From 3.0 LSB serves the lobby data and view ports over TLS too,
//! with this port's key; before it they are plain (lobby.rs).

use super::tls;
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const LOADER_VERSION: [u8; 3] = [3, 0, 0];
/// The most of a login reply taken before giving up: xi_connect's replies are a few hundred bytes of JSON.
const MAX_REPLY: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Command {
    Login = 0x10,
    CreateAccount = 0x20,
    /// The account window's Change password (client/twofa.rs): `password` the current one, `new_password`.
    ChangePassword = 0x30,
    CreateTotp = 0x31,
    RemoveTotp = 0x32,
    RegenerateRecovery = 0x33,
    VerifyTotp = 0x34,
}

#[derive(Default)]
pub struct Request<'a> {
    pub username: &'a str,
    pub password: &'a str,
    pub otp: &'a str,
    pub new_password: &'a str,
    pub trust_token: &'a str,
    pub trust_this_computer: bool,
}

pub enum Reply {
    /// `lobby_key`: the login server's key when it took a 3.x loader version, whose lobby is TLS with that key; none
    /// for an older server's plain lobby.
    LoggedIn {
        account_id: u32,
        session_hash: [u8; 16],
        /// A new trust token, when asked for with a one-time code ("Trust this computer"): sent instead of the code
        /// next time.
        trust_token: Option<String>,
        lobby_key: Option<tls::Fingerprint>,
    },
    AccountCreated,
    PasswordChanged,
    TotpCreated {
        uri: String,
    },
    TotpVerified {
        recovery_code: Option<String>,
    },
    TotpRemoved,
}

/// Without the secrets: the session hash, the trust token, the TOTP key (in its URI) and the recovery code.
impl std::fmt::Debug for Reply {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let hidden = |on: bool| if on { "<hidden>" } else { "none" };
        match self {
            Reply::LoggedIn { account_id, trust_token, lobby_key, .. } => f
                .debug_struct("LoggedIn")
                .field("account_id", account_id)
                .field("trust_token", &hidden(trust_token.is_some()))
                .field("lobby_key", lobby_key)
                .finish_non_exhaustive(),
            Reply::AccountCreated => f.write_str("AccountCreated"),
            Reply::PasswordChanged => f.write_str("PasswordChanged"),
            Reply::TotpCreated { .. } => f.write_str("TotpCreated { .. }"),
            Reply::TotpVerified { recovery_code } => {
                f.debug_struct("TotpVerified").field("recovery_code", &hidden(recovery_code.is_some())).finish()
            }
            Reply::TotpRemoved => f.write_str("TotpRemoved"),
        }
    }
}

/// The result code a trust token was turned down with (expired, or the password changed): the code is needed again.
pub const TRUST_REJECTED: u64 = 0x13;

/// A login server's refusal: its result code (`describe` says what it means).
#[derive(Debug)]
pub struct Refused(pub u64);

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{} (code 0x{:02X})", describe(self.0), self.0)
    }
}

impl std::error::Error for Refused {}

fn describe(result: u64) -> &'static str {
    match result {
        0x00 => "the account is not active",
        0x02 => "invalid username, password or one-time code",
        0x04 => "that username is already taken",
        0x07 => "the password could not be changed",
        0x08 => "account creation is disabled on this server",
        0x09 => "the account could not be created",
        0x0A => "the account is already logged in",
        0x0B => "the server does not support this loader version",
        0x13 => "the saved trust token was rejected; enter your one-time code",
        _ => "unexpected reply from the login server",
    }
}

/// The reply is one JSON document: read until it parses, at most MAX_REPLY bytes.
fn read_reply(stream: &mut impl Read) -> Result<Value> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = match stream.read(&mut chunk) {
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                bail!("the login server did not reply (it ignores requests it considers malformed)")
            }
            r => r.context("reading the login reply")?,
        };
        if n == 0 {
            bail!("the login server closed the connection without replying");
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Ok(v) = serde_json::from_slice(&buf) {
            return Ok(v);
        }
        if buf.len() >= MAX_REPLY {
            bail!("the login server's reply is over {MAX_REPLY} bytes and still not JSON");
        }
    }
}

/// The loader version a refusal asks for: the first `major.minor.patch` in it (patch may be `x`, sent as 0) that isn't
/// the one `sent`.
fn asked_version(msg: &str, sent: [u8; 3]) -> Option<[u8; 3]> {
    let b = msg.as_bytes();
    let num = |at: &mut usize| -> Option<u8> {
        let start = *at;
        while *at < b.len() && b[*at].is_ascii_digit() {
            *at += 1;
        }
        msg[start..*at].parse().ok()
    };
    (0..b.len())
        .filter(|&i| b[i].is_ascii_digit() && (i == 0 || !b[i - 1].is_ascii_digit() && b[i - 1] != b'.'))
        .find_map(|i| {
            let mut at = i;
            let major = num(&mut at)?;
            (b.get(at) == Some(&b'.')).then_some(())?;
            at += 1;
            let minor = num(&mut at)?;
            (b.get(at) == Some(&b'.')).then_some(())?;
            at += 1;
            let patch = if matches!(b.get(at), Some(b'x' | b'X')) { 0 } else { num(&mut at)? };
            ([major, minor] != sent[..2]).then_some([major, minor, patch])
        })
}

/// Each server's last taken loader version, by "host:port": loader_versions.json beside known_servers.json.
fn versions_path() -> PathBuf {
    crate::config::known_servers_path().with_file_name("loader_versions.json")
}

fn read_versions(path: &Path) -> BTreeMap<String, [u8; 3]> {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// Keeps `v` for `server`; a file that can't be written only costs the refused first request next time.
fn remember(path: &Path, server: &str, v: [u8; 3]) {
    static WRITING: Mutex<()> = Mutex::new(());
    let _held = WRITING.lock().unwrap_or_else(|e| e.into_inner());
    let mut all = read_versions(path);
    if all.get(server) == Some(&v) {
        return;
    }
    all.insert(server.to_string(), v);
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let saved = (|| -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&tmp, serde_json::to_string_pretty(&all)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    })();
    if let Err(e) = saved {
        log::warn!("login: {} not saved: {e:#}", path.display());
    }
}

/// The reply, and the key of the certificate the server sent.
fn exchange(host: &str, port: u16, body: &Value) -> Result<(Value, Option<tls::Fingerprint>)> {
    let mut stream = tls::connect(host, port).with_context(|| format!("connecting to {host}:{port}"))?;
    let key = tls::peer_key(&stream);
    stream.write_all(body.to_string().as_bytes())?;
    stream.flush()?;
    let reply = read_reply(&mut stream)?;
    stream.conn.send_close_notify();
    let _ = stream.flush();
    Ok((reply, key))
}

pub fn send(host: &str, port: u16, command: Command, req: &Request) -> Result<Reply> {
    let mut body = json!({
        "command": command as u8,
        "username": req.username,
        "password": req.password,
        "otp": req.otp,
        "new_password": req.new_password,
    });
    if command == Command::Login {
        body["trust_token"] = json!(req.trust_token);
        body["trust_this_computer"] = json!(req.trust_this_computer);
    }
    let (path, server) = (versions_path(), tls::key(host, port));
    let mut version = read_versions(&path).get(&server).copied().unwrap_or(LOADER_VERSION);
    body["version"] = json!(version);
    let (mut reply, mut key) = exchange(host, port, &body)?;
    // A server built for another loader names the version it checks for; the requests are the same, so ask as that.
    if let Some(v) = reply["error_message"].as_str().and_then(|m| asked_version(m, version)) {
        log::info!("login: {host} asks for loader {}.{}.x; asking again as {}.{}.{}", v[0], v[1], v[0], v[1], v[2]);
        body["version"] = json!(v);
        version = v;
        (reply, key) = exchange(host, port, &body)?;
    }
    if let Some(msg) = reply["error_message"].as_str() {
        bail!("{msg}");
    }
    let result = reply["result"].as_u64().context("reply has no result code")?;
    if result != 0x0B {
        remember(&path, &server, version);
    }
    Ok(match result {
        0x01 => {
            let bytes: Vec<u8> = reply["session_hash"]
                .as_array()
                .context("reply has no session hash")?
                .iter()
                .map(|b| b.as_u64().unwrap_or(0) as u8)
                .collect();
            Reply::LoggedIn {
                account_id: reply["account_id"].as_u64().context("reply has no account id")? as u32,
                session_hash: bytes.try_into().map_err(|_| anyhow::anyhow!("session hash is not 16 bytes"))?,
                trust_token: reply["trust_token"].as_str().filter(|t| !t.is_empty()).map(str::to_string),
                lobby_key: lobby_key(version, key)?,
            }
        }
        0x03 => Reply::AccountCreated,
        0x06 => Reply::PasswordChanged,
        0x10 => Reply::TotpCreated { uri: reply["TOTP_uri"].as_str().unwrap_or_default().to_string() },
        0x11 => Reply::TotpVerified { recovery_code: reply["recovery_code"].as_str().map(str::to_string) },
        0x12 => Reply::TotpRemoved,
        other => return Err(Refused(other).into()),
    })
}

/// The key the lobby ports must hold: the login server's, when it took a 3.x or later version (TLS lobby).
fn lobby_key(version: [u8; 3], key: Option<tls::Fingerprint>) -> Result<Option<tls::Fingerprint>> {
    if version[0] < 3 {
        return Ok(None);
    }
    key.map(Some).context("the login server sent no certificate key to hold the lobby to")
}

#[cfg(test)]
mod reply_tests {
    use super::*;

    /// A reader that hands out `left` bytes of `fill`, then the end.
    struct Endless(u8, usize);
    impl Read for Endless {
        fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
            let n = b.len().min(self.1);
            b[..n].fill(self.0);
            self.1 -= n;
            Ok(n)
        }
    }

    /// LSB's refusal and one server's wording both give the version to ask as; the one sent alone gives none.
    #[test]
    fn asked_version_from_refusals() {
        let v3 = LOADER_VERSION;
        let lsb = "Unsupported xiloader version 3.0.0.\nThis server requires version 2.2.x.";
        let other = "Your xiloader is too old.\nPlease update to version '2.0.x'.\nYour client reported '3.0.0'.";
        assert_eq!((asked_version(lsb, v3), asked_version(other, v3)), (Some([2, 2, 0]), Some([2, 0, 0])));
        assert_eq!(asked_version("needs 1.12.3 here", v3), Some([1, 12, 3]));
        assert_eq!(
            (asked_version("Your client reported '3.0.0'.", v3), asked_version("version 10.0", v3)),
            (None, None)
        );
        // A remembered 2.2 that the server has since moved on from.
        let moved = "Unsupported xiloader version 2.2.0.\nThis server requires version 3.0.x.";
        assert_eq!(asked_version(moved, [2, 2, 0]), Some([3, 0, 0]));
    }

    /// A taken version is kept per server, and a store that can't be read or written only means none.
    #[test]
    fn versions_are_remembered() {
        let dir = std::env::temp_dir().join(format!("vana-loader-versions-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("loader_versions.json");
        assert!(read_versions(&path).is_empty());
        remember(&path, "lsb:54231", [2, 2, 0]);
        remember(&path, "other:54231", [3, 0, 0]);
        remember(&path, "lsb:54231", [3, 0, 0]);
        let all = read_versions(&path);
        assert_eq!((all.get("lsb:54231"), all.get("other:54231")), (Some(&[3, 0, 0]), Some(&[3, 0, 0])));
        std::fs::write(&path, "not json").unwrap();
        assert!(read_versions(&path).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A 3.x server's lobby is held to its login key; a 2.x server's lobby is plain.
    #[test]
    fn lobby_key_follows_the_version() {
        let k = [7u8; 32];
        assert_eq!(lobby_key([3, 0, 0], Some(k)).unwrap(), Some(k));
        assert_eq!(lobby_key([2, 2, 0], Some(k)).unwrap(), None);
        assert!(lobby_key([3, 1, 0], None).is_err());
    }

    #[test]
    fn reply_parses_when_whole() {
        let v = read_reply(&mut &br#"{"result": 1}"#[..]).unwrap();
        assert_eq!(v["result"], 1);
    }

    /// A server streaming non-JSON is cut off at MAX_REPLY, not read until it stops.
    #[test]
    fn endless_reply_is_cut_off() {
        let mut r = Endless(b' ', usize::MAX);
        let e = read_reply(&mut r).unwrap_err();
        assert!(e.to_string().contains("still not JSON"), "{e}");
        assert!(usize::MAX - r.1 <= MAX_REPLY + 4096);
    }
}
