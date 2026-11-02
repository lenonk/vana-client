//! The pinned list of game code files (issue #84 follow-up). The patch server speaks plain TCP and its manifest
//! carries only a 4-byte MD5 word per file, so anyone who can answer for pc001.pol.com could hand the patcher their
//! own FFXiMain.dll. FFXiMain.dll carries no Authenticode signature, so the patcher instead writes an executable or
//! library only when its SHA-256 is on a list that a scheduled GitHub Action (.github/workflows/code-pins.yml) builds
//! from the real server and signs with the code-pins key (Ed25519; a CI secret, not the release key).
//!
//! The list (code-pins.txt in the `code-pins` pre-release, its Ed25519 signature in code-pins.txt.sig):
//! ```text
//! vana-code-pins 1
//! server <patch server version>
//! <sha256 hex> <length> <manifest path>
//! ```

use anyhow::{Context, Result, anyhow, bail};

/// The code-pins key's public half (release/code-pins-key.pub, 64 hex digits; the private half is the CI secret
/// CODE_PINS_KEY, made by release/new-code-pins-key.sh). Empty in a build without one: then no code file is written.
const KEY: &str = include_str!("../../../release/code-pins-key.pub");
const HEADER: &str = "vana-code-pins 1";
/// Where the workflow publishes the list (a pre-release, so the updater's releases/latest never picks it).
pub const URL: &str = "https://github.com/akreao/vana-client/releases/download/code-pins/code-pins.txt";

/// SHA-256 as lowercase hex.
pub fn sha256(data: &[u8]) -> String {
    hex::encode(ring::digest::digest(&ring::digest::SHA256, data))
}

/// One code file's line: what the list would say about `data` at `path`.
pub fn line(path: &str, data: &[u8]) -> String {
    format!("{} {} {path}", sha256(data), data.len())
}

/// The list text for these lines (sorted, so an unchanged server gives the same bytes).
pub fn render(server: &str, mut lines: Vec<String>) -> String {
    lines.sort_by(|a, b| a.splitn(3, ' ').nth(2).cmp(&b.splitn(3, ' ').nth(2)).then(a.cmp(b)));
    let mut out = format!("{HEADER}\nserver {server}\n");
    for l in lines {
        out.push_str(&l);
        out.push('\n');
    }
    out
}

#[derive(Debug)]
pub struct Pins {
    pub server: String,
    /// (lowercased path, length, sha256 hex).
    files: Vec<(String, u64, String)>,
}

impl Pins {
    /// A list whose signature has been checked.
    fn parse(text: &str) -> Result<Self> {
        let mut lines = text.lines();
        if lines.next() != Some(HEADER) {
            bail!("not a code-pins list");
        }
        let server = lines.next().and_then(|l| l.strip_prefix("server ")).context("code-pins list has no server line")?.to_string();
        let files = lines
            .filter(|l| !l.is_empty())
            .map(|l| {
                let mut it = l.splitn(3, ' ');
                let (h, n, p) = (it.next(), it.next().and_then(|n| n.parse().ok()), it.next());
                match (h, n, p) {
                    (Some(h), Some(n), Some(p)) if h.len() == 64 && h.bytes().all(|c| c.is_ascii_hexdigit()) => Ok((p.to_ascii_lowercase(), n, h.to_ascii_lowercase())),
                    _ => Err(anyhow!("code-pins line {l:?}")),
                }
            })
            .collect::<Result<_>>()?;
        Ok(Self { server, files })
    }

    /// `text` when `sig` is its signature by `key`.
    pub fn verified(key: &[u8; 32], text: &[u8], sig: &[u8]) -> Result<Self> {
        if ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, key).verify(text, sig).is_err() {
            bail!("the code-pins list is not signed by the code-pins key");
        }
        Self::parse(std::str::from_utf8(text).context("code-pins list is not UTF-8")?)
    }

    /// The published list, checked against the built-in key. VANA_CODE_PINS_URL: another place (tests).
    pub fn fetch() -> Result<Self> {
        let key = key()?;
        let url = std::env::var("VANA_CODE_PINS_URL").unwrap_or_else(|_| URL.to_string());
        let agent: ureq::Agent = ureq::Agent::config_builder().user_agent(concat!("vana-client/", env!("CARGO_PKG_VERSION"))).build().into();
        let get = |u: &str, limit: u64| -> Result<Vec<u8>> { Ok(agent.get(u).call().with_context(|| u.to_string())?.body_mut().with_config().limit(limit).read_to_vec()?) };
        let text = get(&url, 1 << 20)?;
        let sig = get(&format!("{url}.sig"), 1024)?;
        Self::verified(&key, &text, &sig)
    }

    /// `data` is a pinned version of `path` (paths compare without case, as Windows opens them).
    pub fn allows(&self, path: &str, data: &[u8]) -> bool {
        let (p, h) = (path.to_ascii_lowercase(), sha256(data));
        self.files.iter().any(|(fp, n, fh)| *fp == p && *n == data.len() as u64 && *fh == h)
    }
}

fn key() -> Result<[u8; 32]> {
    let hex = KEY.trim();
    if hex.is_empty() {
        bail!("this build has no code-pins key, so it writes no game code files");
    }
    hex::decode(hex).ok().and_then(|b| b.try_into().ok()).context("release/code-pins-key.pub is not 64 hex digits")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::KeyPair;

    #[test]
    fn signed_lists_pin_exact_files() {
        let rng = ring::rand::SystemRandom::new();
        let pair = ring::signature::Ed25519KeyPair::from_pkcs8(ring::signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap().as_ref()).unwrap();
        let key: [u8; 32] = pair.public_key().as_ref().try_into().unwrap();
        let text = render("30261006_0", vec![line("polboot.exe", b"boot"), line("FFXiMain.dll", b"main")]);
        assert!(text.starts_with("vana-code-pins 1\nserver 30261006_0\n") && text.find("FFXiMain").unwrap() < text.find("polboot").unwrap());
        let sig = pair.sign(text.as_bytes());
        let pins = Pins::verified(&key, text.as_bytes(), sig.as_ref()).unwrap();
        assert_eq!(pins.server, "30261006_0");
        assert!(pins.allows("FFXiMain.dll", b"main") && pins.allows("ffximain.DLL", b"main"));
        assert!(!pins.allows("FFXiMain.dll", b"evil") && !pins.allows("FFXiMain.dll", b"boot") && !pins.allows("other.dll", b"main"));
        // Any change to the text, or another key, fails the signature.
        let tampered = text.replace("30261006_0", "30261006_1");
        assert!(Pins::verified(&key, tampered.as_bytes(), sig.as_ref()).is_err());
        assert!(Pins::verified(&[7; 32], text.as_bytes(), sig.as_ref()).is_err());
        assert!(Pins::parse("vana-code-pins 1\nserver x\nnothex 1 a.dll\n").is_err());
        assert!(Pins::parse("something else\n").is_err());
    }
}
