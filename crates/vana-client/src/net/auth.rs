//! xi_connect's auth service (TCP 54231, TLS 1.3, one JSON request and reply per connection).
//! Speaks the protocol of xiloader 2.2.x, which the server checks for; a server that names another version is asked
//! again as that one.

use super::tls;
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::io::{Read, Write};

pub const LOADER_VERSION: [u8; 3] = [2, 2, 0];
/// The most of a login reply taken before giving up: xi_connect's replies are a few hundred bytes of JSON.
const MAX_REPLY: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Command {
    Login = 0x10,
    CreateAccount = 0x20,
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

#[derive(Debug)]
pub enum Reply {
    LoggedIn { account_id: u32, session_hash: [u8; 16], trust_token: Option<String> },
    AccountCreated,
    PasswordChanged,
    TotpCreated { uri: String },
    TotpVerified { recovery_code: Option<String> },
    TotpRemoved,
}

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

/// The loader version a refusal asks for: the first `major.minor.patch` in it (patch may be `x`, sent as 0) that isn't ours.
fn asked_version(msg: &str) -> Option<[u8; 3]> {
    let b = msg.as_bytes();
    let num = |at: &mut usize| -> Option<u8> {
        let start = *at;
        while *at < b.len() && b[*at].is_ascii_digit() {
            *at += 1;
        }
        msg[start..*at].parse().ok()
    };
    (0..b.len()).filter(|&i| b[i].is_ascii_digit() && (i == 0 || !b[i - 1].is_ascii_digit() && b[i - 1] != b'.')).find_map(|i| {
        let mut at = i;
        let major = num(&mut at)?;
        (b.get(at) == Some(&b'.')).then_some(())?;
        at += 1;
        let minor = num(&mut at)?;
        (b.get(at) == Some(&b'.')).then_some(())?;
        at += 1;
        let patch = if matches!(b.get(at), Some(b'x' | b'X')) { 0 } else { num(&mut at)? };
        ([major, minor] != LOADER_VERSION[..2]).then_some([major, minor, patch])
    })
}

fn exchange(host: &str, port: u16, body: &Value) -> Result<Value> {
    let mut stream = tls::connect(host, port).with_context(|| format!("connecting to {host}:{port}"))?;
    stream.write_all(body.to_string().as_bytes())?;
    stream.flush()?;
    let reply = read_reply(&mut stream)?;
    stream.conn.send_close_notify();
    let _ = stream.flush();
    Ok(reply)
}

pub fn send(host: &str, port: u16, command: Command, req: &Request) -> Result<Reply> {
    let mut body = json!({
        "command": command as u8,
        "username": req.username,
        "password": req.password,
        "otp": req.otp,
        "new_password": req.new_password,
        "version": LOADER_VERSION,
    });
    if command == Command::Login {
        body["trust_token"] = json!(req.trust_token);
        body["trust_this_computer"] = json!(req.trust_this_computer);
    }
    let mut reply = exchange(host, port, &body)?;
    // A server built for another loader names the version it checks for; the requests are the same, so ask as that.
    if let Some(v) = reply["error_message"].as_str().and_then(asked_version) {
        log::info!("login: {host} asks for loader {}.{}.x; asking again as {}.{}.{}", v[0], v[1], v[0], v[1], v[2]);
        body["version"] = json!(v);
        reply = exchange(host, port, &body)?;
    }
    if let Some(msg) = reply["error_message"].as_str() {
        bail!("{msg}");
    }
    let result = reply["result"].as_u64().context("reply has no result code")?;
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
            }
        }
        0x03 => Reply::AccountCreated,
        0x06 => Reply::PasswordChanged,
        0x10 => Reply::TotpCreated { uri: reply["TOTP_uri"].as_str().unwrap_or_default().to_string() },
        0x11 => Reply::TotpVerified { recovery_code: reply["recovery_code"].as_str().map(str::to_string) },
        0x12 => Reply::TotpRemoved,
        other => bail!("{} (code 0x{other:02X})", describe(other)),
    })
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

    /// LSB's refusal and one server's own wording both give the version to ask as; ours alone gives none.
    #[test]
    fn asked_version_from_refusals() {
        let lsb = "Unsupported xiloader version 2.2.0.\nThis server requires version 2.0.x.";
        let other = "Your xiloader is too old.\nPlease update to version '2.0.x'.\nYour client reported '2.2.0'.";
        assert_eq!((asked_version(lsb), asked_version(other)), (Some([2, 0, 0]), Some([2, 0, 0])));
        assert_eq!(asked_version("needs 1.12.3 here"), Some([1, 12, 3]));
        assert_eq!((asked_version("Your client reported '2.2.0'."), asked_version("version 10.0")), (None, None));
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
