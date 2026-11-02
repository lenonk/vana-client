//! TLS 1.3 client used for xi_connect auth and xi_profile. Private servers run
//! self-signed X.509 v1 certificates that webpki cannot parse and no authority signs, so the
//! certificate is pinned instead (not retail; xiloader checks nothing): the SHA-256 of the
//! certificate a server first sends is kept in known_servers.json, and a later connection with
//! another one is refused until the user trusts it (`vana-client trust-server`). A fingerprint
//! set there (`--fingerprint`, e.g. by whoever set up the server) is checked from the first
//! connection on. The handshake signature is checked too: the server must hold the private key
//! of the certificate it sent, its public key read out of the DER by hand.

use anyhow::{Context, Result, bail};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, SubjectPublicKeyInfoDer, UnixTime};
use rustls::{ClientConfig, ClientConnection, DigitallySignedStruct, SignatureScheme, StreamOwned};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::net::TcpStream;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub type Fingerprint = [u8; 32];

/// What a server's certificate must be: the SHA-256 of its DER, learned at the first connection or set by the user.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pin {
    pub sha256: String,
    /// Set by the user (trust-server --fingerprint), not learned: trust-server without a fingerprint leaves it.
    #[serde(default)]
    pub fixed: bool,
}

/// known_servers.json: "host:port" to its pin.
#[derive(Default, Serialize, Deserialize)]
pub struct KnownServers(pub BTreeMap<String, Pin>);

impl KnownServers {
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(Self(serde_json::from_str(&text).with_context(|| format!("reading {}", path.display()))?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    /// Written through a temporary file, so a crash never leaves half a file.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::File::create(&tmp)?.write_all(serde_json::to_string_pretty(&self.0)?.as_bytes())?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn expected(&self, key: &str) -> Result<Option<Fingerprint>> {
        self.0.get(key).map(|p| parse_fingerprint(&p.sha256).with_context(|| format!("known_servers.json: {key}"))).transpose()
    }
}

pub fn key(host: &str, port: u16) -> String {
    format!("{}:{port}", host.trim().to_ascii_lowercase())
}

/// 64 hex digits, colons and spaces ignored (openssl's "AB:CD:..." form works as it prints).
pub fn parse_fingerprint(text: &str) -> Result<Fingerprint> {
    let hex: String = text.chars().filter(|c| *c != ':' && !c.is_whitespace()).collect();
    let bytes = hex::decode(&hex).ok().filter(|b| b.len() == 32).with_context(|| format!("{text:?} is not a SHA-256 fingerprint (64 hex digits)"))?;
    Ok(bytes.try_into().unwrap())
}

/// "AB:CD:...", as openssl x509 -fingerprint -sha256 prints it.
pub fn show(f: &Fingerprint) -> String {
    f.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":")
}

pub fn fingerprint(cert: &[u8]) -> Fingerprint {
    ring::digest::digest(&ring::digest::SHA256, cert).as_ref().try_into().unwrap()
}

#[derive(Debug)]
struct AnyCert {
    provider: Arc<CryptoProvider>,
    /// The pinned fingerprint, when the server has one.
    expected: Option<Fingerprint>,
    /// The fingerprint the server sent, for the caller to pin or report.
    seen: Arc<Mutex<Option<Fingerprint>>>,
}

impl ServerCertVerifier for AnyCert {
    fn verify_server_cert(
        &self,
        cert: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let got = fingerprint(cert);
        *self.seen.lock().unwrap() = Some(got);
        match self.expected {
            Some(want) if want != got => Err(rustls::Error::InvalidCertificate(rustls::CertificateError::ApplicationVerificationFailure)),
            _ => Ok(ServerCertVerified::assertion()),
        }
    }

    /// client_config offers TLS 1.3 only, so a 1.2 handshake never gets here.
    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::PeerIncompatible(rustls::PeerIncompatible::Tls12NotOffered))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        let spki = cert_spki(cert).ok_or(rustls::Error::InvalidCertificate(rustls::CertificateError::BadEncoding))?;
        rustls::crypto::verify_tls13_signature_with_raw_key(message, &SubjectPublicKeyInfoDer::from(spki), dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

/// One DER element at the start of `d`: (tag, contents, the rest after it). Definite lengths only (DER).
fn der(d: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, d) = d.split_first()?;
    let (&first, d) = d.split_first()?;
    let (len, d) = if first < 0x80 {
        (first as usize, d)
    } else {
        let n = (first & 0x7f) as usize;
        if n == 0 || n > 4 || d.len() < n {
            return None;
        }
        (d[..n].iter().fold(0usize, |v, &b| v << 8 | b as usize), &d[n..])
    };
    (d.len() >= len).then(|| (tag, &d[..len], &d[len..]))
}

/// The SubjectPublicKeyInfo (tag and length included) of an X.509 certificate of any version: Certificate ->
/// tbsCertificate -> [0] version (v2/v3 only), serial, signature, issuer, validity, subject, subjectPublicKeyInfo.
fn cert_spki(cert: &[u8]) -> Option<&[u8]> {
    let (0x30, c, _) = der(cert)? else { return None };
    let (0x30, mut tbs, _) = der(c)? else { return None };
    if tbs.first() == Some(&0xa0) {
        tbs = der(tbs)?.2;
    }
    for _ in 0..5 {
        tbs = der(tbs)?.2;
    }
    let (0x30, _, rest) = der(tbs)? else { return None };
    Some(&tbs[..tbs.len() - rest.len()])
}

fn client_config(expected: Option<Fingerprint>, seen: Arc<Mutex<Option<Fingerprint>>>) -> Result<Arc<ClientConfig>> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AnyCert { provider, expected, seen }))
        .with_no_client_auth();
    Ok(Arc::new(config))
}

pub type TlsStream = StreamOwned<ClientConnection, TcpStream>;

pub fn connect(host: &str, port: u16) -> Result<TlsStream> {
    connect_pinned(&crate::config::known_servers_path(), host, port)
}

/// connect with the pins in `store`: the handshake runs here, so a changed certificate is refused before anything
/// (a password) is sent, and a first one is pinned once the handshake has passed.
pub fn connect_pinned(store: &Path, host: &str, port: u16) -> Result<TlsStream> {
    let key = key(host, port);
    let known = KnownServers::load(store)?;
    let expected = known.expected(&key)?;
    let seen: Arc<Mutex<Option<Fingerprint>>> = Default::default();
    let mut tcp = TcpStream::connect((host, port))?;
    // The login server sends nothing back for requests it rejects as malformed.
    tcp.set_read_timeout(Some(std::time::Duration::from_secs(10)))?;
    let name = ServerName::try_from(host.to_string())?;
    let mut conn = ClientConnection::new(client_config(expected, seen.clone())?, name)?;
    while conn.is_handshaking() {
        if let Err(e) = conn.complete_io(&mut tcp) {
            let got = *seen.lock().unwrap();
            if let (Some(want), Some(got)) = (expected, got)
                && want != got
            {
                bail!(
                    "{key}'s certificate is not the one this client knows (known {}, sent {}). Someone may be \
                     intercepting the connection. If the server's certificate was replaced (a reinstall makes a new \
                     one), trust the new one with `vana-client trust-server {host} --port {port}`.",
                    show(&want),
                    show(&got)
                );
            }
            return Err(e.into());
        }
    }
    if expected.is_none()
        && let Some(got) = *seen.lock().unwrap()
    {
        record_pin(store, &key, got)?;
    }
    Ok(StreamOwned::new(conn, tcp))
}

/// Pins `got` for `key` after a first handshake. Another connection may have pinned the server while that handshake
/// ran (the auth and lobby steps run on their own threads, and another client may share the file): re-read under a
/// lock, and refuse a certificate that differs from the pin found.
fn record_pin(store: &Path, key: &str, got: Fingerprint) -> Result<()> {
    static PINNING: Mutex<()> = Mutex::new(());
    let _held = PINNING.lock().unwrap_or_else(|e| e.into_inner());
    let mut known = KnownServers::load(store)?;
    match known.expected(key)? {
        Some(want) if want != got => bail!(
            "{key} sent two different certificates while it was being pinned (kept {}, refused {}). Someone may be \
             intercepting the connection.",
            show(&want),
            show(&got)
        ),
        Some(_) => Ok(()),
        None => {
            known.0.insert(key.to_string(), Pin { sha256: hex::encode(got), fixed: false });
            known.save(store)?;
            log::info!("tls: pinned {key}'s certificate {}", show(&got));
            Ok(())
        }
    }
}

/// trust-server: forgets `host`'s learned pins (all its ports, or `port`), so the next connection pins what it is
/// sent, or sets a fixed pin. Returns what changed, for the command to print.
pub fn trust(store: &Path, host: &str, port: Option<u16>, fingerprint: Option<&str>) -> Result<Vec<String>> {
    let mut known = KnownServers::load(store)?;
    let mut out = Vec::new();
    match fingerprint {
        Some(f) => {
            let port = port.context("--fingerprint needs --port (the auth port is usually 54231)")?;
            let f = parse_fingerprint(f)?;
            let k = key(host, port);
            known.0.insert(k.clone(), Pin { sha256: hex::encode(f), fixed: true });
            out.push(format!("{k}: pinned to {}", show(&f)));
        }
        None => {
            let prefix = format!("{}:", host.trim().to_ascii_lowercase());
            let keys: Vec<String> = known.0.keys().filter(|k| k.starts_with(&prefix) && port.is_none_or(|p| **k == key(host, p))).cloned().collect();
            for k in keys {
                if known.0[&k].fixed && port.is_none() {
                    out.push(format!("{k}: kept (a fixed fingerprint; forget it with --port)"));
                    continue;
                }
                known.0.remove(&k);
                out.push(format!("{k}: forgotten; the next connection pins the certificate it is sent"));
            }
            if out.is_empty() {
                out.push(format!("{host}: no certificate pinned"));
            }
        }
    }
    known.save(store)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{ECDSA_P256_SHA256_ASN1_SIGNING, EcdsaKeyPair, KeyPair};

    // Self-signed v1 certificates as LSB's cert_helpers.h makes them (no version field; RSA 4096 or P-256, SHA-384),
    // and the public keys openssl reads out of them.
    const RSA_V1: &[u8] = include_bytes!("testdata/rsa-v1.der");
    const RSA_SPKI: &[u8] = include_bytes!("testdata/rsa-spki.der");
    const EC_V1: &[u8] = include_bytes!("testdata/ec-v1.der");
    const EC_SPKI: &[u8] = include_bytes!("testdata/ec-spki.der");

    #[test]
    fn spki_from_v1_certs() {
        assert_eq!(cert_spki(RSA_V1), Some(RSA_SPKI));
        assert_eq!(cert_spki(EC_V1), Some(EC_SPKI));
        assert_eq!(cert_spki(&EC_V1[..EC_V1.len() / 2]), None);
        assert_eq!(cert_spki(&[]), None);
    }

    /// A minimal v1 or v3 certificate around `spki` (only the structure matters here; nothing checks its signature).
    fn cert_with(spki: &[u8], v3: bool) -> Vec<u8> {
        fn tlv(tag: u8, body: &[u8]) -> Vec<u8> {
            let mut v = vec![tag];
            if body.len() < 0x80 {
                v.push(body.len() as u8);
            } else {
                v.extend([0x82, (body.len() >> 8) as u8, body.len() as u8]);
            }
            v.extend_from_slice(body);
            v
        }
        let name = tlv(0x30, &[]);
        let alg = tlv(0x30, &[0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x03]);
        let mut tbs = if v3 { tlv(0xa0, &[0x02, 0x01, 0x02]) } else { vec![] };
        tbs.extend([0x02, 0x01, 0x01]);
        tbs.extend(&alg);
        tbs.extend(&name);
        tbs.extend(tlv(0x30, &[]));
        tbs.extend(&name);
        tbs.extend_from_slice(spki);
        let mut c = tlv(0x30, &tbs);
        c.extend(&alg);
        c.extend([0x03, 0x01, 0x00]);
        tlv(0x30, &c)
    }

    #[test]
    fn handshake_signature_must_match_the_cert() {
        let rng = ring::rand::SystemRandom::new();
        let key = |rng: &ring::rand::SystemRandom| {
            let pk8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, rng).unwrap();
            EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pk8.as_ref(), rng).unwrap()
        };
        let (ours, theirs) = (key(&rng), key(&rng));
        // SubjectPublicKeyInfo for a P-256 key: id-ecPublicKey, prime256v1, the point.
        let mut spki_body = vec![0x30, 0x13, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07];
        spki_body.extend([0x03, 0x42, 0x00]);
        spki_body.extend_from_slice(ours.public_key().as_ref());
        let mut spki = vec![0x30, spki_body.len() as u8];
        spki.extend(spki_body);

        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = AnyCert { provider, expected: None, seen: Default::default() };
        let msg = b"TLS 1.3, server CertificateVerify";
        let dss = |k: &EcdsaKeyPair| {
            let sig = k.sign(&rng, msg).unwrap().as_ref().to_vec();
            // DigitallySignedStruct's wire form: scheme (ecdsa_secp256r1_sha256), length, signature.
            let mut wire = vec![0x04, 0x03, (sig.len() >> 8) as u8, sig.len() as u8];
            wire.extend(sig);
            <DigitallySignedStruct as rustls::internal::msgs::codec::Codec>::read_bytes(&wire).unwrap()
        };
        for v3 in [false, true] {
            let cert = CertificateDer::from(cert_with(&spki, v3));
            assert!(verifier.verify_tls13_signature(msg, &cert, &dss(&ours)).is_ok());
            // Someone in the path who sends this cert without its key is refused.
            assert!(verifier.verify_tls13_signature(msg, &cert, &dss(&theirs)).is_err());
            assert!(verifier.verify_tls13_signature(b"another transcript", &cert, &dss(&ours)).is_err());
        }
        assert!(verifier.verify_tls13_signature(msg, &CertificateDer::from(vec![0x30, 0x00]), &dss(&ours)).is_err());
    }

    #[test]
    fn fingerprints_and_trust() {
        let f = fingerprint(b"cert");
        assert_eq!(parse_fingerprint(&show(&f)).unwrap(), f);
        assert_eq!(parse_fingerprint(&hex::encode(f)).unwrap(), f);
        assert!(parse_fingerprint("AB:CD").is_err());
        assert_eq!(key(" Example.ORG ", 54231), "example.org:54231");

        let dir = std::env::temp_dir().join(format!("vana-tls-trust-{}", std::process::id()));
        let store = dir.join("known_servers.json");
        let _ = std::fs::remove_dir_all(&dir);
        let mut known = KnownServers::default();
        known.0.insert(key("h", 1), Pin { sha256: hex::encode(f), fixed: false });
        known.0.insert(key("h", 2), Pin { sha256: hex::encode(f), fixed: true });
        known.0.insert(key("other", 1), Pin { sha256: hex::encode(f), fixed: false });
        known.save(&store).unwrap();
        // Without --port: learned pins go, a fixed one stays, other hosts are untouched.
        trust(&store, "H", None, None).unwrap();
        let k = KnownServers::load(&store).unwrap();
        assert_eq!(k.0.keys().collect::<Vec<_>>(), ["h:2", "other:1"]);
        // --port forgets a fixed one too; --fingerprint sets one (and needs --port).
        trust(&store, "h", Some(2), None).unwrap();
        assert!(trust(&store, "h", None, Some(&show(&f))).is_err());
        trust(&store, "h", Some(3), Some(&show(&f))).unwrap();
        let k = KnownServers::load(&store).unwrap();
        assert_eq!(k.0.get("h:3"), Some(&Pin { sha256: hex::encode(f), fixed: true }));
        assert!(!k.0.contains_key("h:2"));
        // A first connection that finds a pin made meanwhile keeps it, and refuses a different certificate.
        let g = fingerprint(b"other cert");
        record_pin(&store, "new:1", f).unwrap();
        record_pin(&store, "new:1", f).unwrap();
        assert!(record_pin(&store, "new:1", g).is_err());
        assert_eq!(KnownServers::load(&store).unwrap().expected("new:1").unwrap(), Some(f));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Pinning against real servers: openssl s_server with two LSB-style self-signed v1 certificates
    /// (`cargo test --release net::tls -- --ignored`; needs openssl on PATH).
    #[test]
    #[ignore = "needs the openssl command"]
    fn pins_a_server() {
        use std::process::{Command, Stdio};
        let dir = std::env::temp_dir().join(format!("vana-tls-pin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let openssl = |args: &[&str]| assert!(Command::new("openssl").args(args).current_dir(&dir).stderr(Stdio::null()).status().unwrap().success());
        for n in ["a", "b"] {
            openssl(&["genpkey", "-algorithm", "EC", "-pkeyopt", "ec_paramgen_curve:P-256", "-out", &format!("{n}.key")]);
            openssl(&["req", "-new", "-key", &format!("{n}.key"), "-subj", "/O=LSB self-signed certificate/CN=127.0.0.1", "-out", &format!("{n}.csr")]);
            openssl(&["x509", "-req", "-in", &format!("{n}.csr"), "-signkey", &format!("{n}.key"), "-sha384", "-days", "30", "-out", &format!("{n}.pem")]);
        }
        let der = |n: &str| {
            let pem = std::fs::read_to_string(dir.join(format!("{n}.pem"))).unwrap();
            let b64: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
            fingerprint(&ring_b64(&b64))
        };
        let port = 44000 + (std::process::id() % 1000) as u16;
        let serve = |n: &str| {
            let mut child = Command::new("openssl")
                .args(["s_server", "-quiet", "-tls1_3", "-naccept", "1", "-accept", &format!("127.0.0.1:{port}"), "-cert", &format!("{n}.pem"), "-key", &format!("{n}.key")])
                .current_dir(&dir)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(500));
            let stdin = child.stdin.take();
            (child, stdin)
        };
        let store = dir.join("known_servers.json");
        let attempt = |n: &str| {
            let (mut child, stdin) = serve(n);
            let r = connect_pinned(&store, "127.0.0.1", port).map(|_| ());
            drop(stdin);
            let _ = child.kill();
            let _ = child.wait();
            r
        };
        // First use pins a; a again passes; b is refused, naming both fingerprints.
        attempt("a").unwrap();
        assert_eq!(KnownServers::load(&store).unwrap().expected(&key("127.0.0.1", port)).unwrap(), Some(der("a")));
        attempt("a").unwrap();
        let e = format!("{:#}", attempt("b").unwrap_err());
        assert!(e.contains("is not the one this client knows") && e.contains(&show(&der("a"))) && e.contains(&show(&der("b"))), "{e}");
        // trust-server forgets it: b is pinned next.
        trust(&store, "127.0.0.1", Some(port), None).unwrap();
        attempt("b").unwrap();
        assert_eq!(KnownServers::load(&store).unwrap().expected(&key("127.0.0.1", port)).unwrap(), Some(der("b")));
        // A fixed fingerprint holds from the first connection.
        trust(&store, "127.0.0.1", Some(port), Some(&show(&der("a")))).unwrap();
        assert!(attempt("b").is_err());
        attempt("a").unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn ring_b64(s: &str) -> Vec<u8> {
        const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = Vec::new();
        let (mut acc, mut bits) = (0u32, 0);
        for c in s.bytes().filter(|&c| c != b'=') {
            acc = acc << 6 | A.iter().position(|&x| x == c).unwrap() as u32;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((acc >> bits) as u8);
            }
        }
        out
    }
}
