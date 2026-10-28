//! xi_connect's auth service (TCP 54231, TLS 1.3, one JSON request and reply per connection).
//! Speaks the protocol of xiloader 2.2.x, which the server checks for.

use super::tls;
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::io::{Read, Write};

pub const LOADER_VERSION: [u8; 3] = [2, 2, 0];

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

pub fn send(host: &str, port: u16, command: Command, req: &Request) -> Result<Reply> {
    let mut stream = tls::connect(host, port).with_context(|| format!("connecting to {host}:{port}"))?;
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
    stream.write_all(body.to_string().as_bytes())?;
    stream.flush()?;

    // The reply is one JSON document; read until it parses.
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let reply: Value = loop {
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
            break v;
        }
    };
    stream.conn.send_close_notify();
    let _ = stream.flush();

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
