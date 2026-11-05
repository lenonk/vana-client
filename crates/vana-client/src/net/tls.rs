//! TLS 1.3 for xi_connect and xi_profile (ours; xiloader checks nothing). A certificate a public authority signs is
//! checked as any is; a self-signed one (LSB's, X.509 v1) is pinned by its key the first time (known_servers.json).

use anyhow::{Context, Result, bail};
use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, SubjectPublicKeyInfoDer, UnixTime};
use rustls::{ClientConfig, ClientConnection, DigitallySignedStruct, SignatureScheme, StreamOwned};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// Pins this run could not save, by store and server: they stand in for the store's until it takes them.
fn session_pins() -> &'static Mutex<BTreeMap<(PathBuf, String), Pin>> {
    static PINS: OnceLock<Mutex<BTreeMap<(PathBuf, String), Pin>>> = OnceLock::new();
    PINS.get_or_init(Default::default)
}

fn session_pin(store: &Path, key: &str) -> Option<Pin> {
    session_pins().lock().unwrap_or_else(|e| e.into_inner()).get(&(store.to_path_buf(), key.to_string())).cloned()
}

/// The player's line when a pin could not be saved: set once a run, taken by the client.
static NOTICE: Mutex<(bool, Option<String>)> = Mutex::new((false, None));

/// The unsaved-pin line, once (the client shows it in the log).
pub fn take_notice() -> Option<String> {
    NOTICE.lock().unwrap_or_else(|e| e.into_inner()).1.take()
}

pub type Fingerprint = [u8; 32];

/// What a server's certificate must be.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "trust", rename_all = "lowercase")]
pub enum Pin {
    /// Signed by a public authority: a self-signed one is refused from then on.
    Authority,
    /// The SHA-256 of its public key (SubjectPublicKeyInfo): learned, or set by the user (`fixed`, which an
    /// authority-signed certificate does not replace).
    Key {
        sha256: String,
        #[serde(default)]
        fixed: bool,
    },
}

/// known_servers.json: "host:port" to its pin.
#[derive(Default, Serialize, Deserialize)]
pub struct KnownServers(pub BTreeMap<String, Pin>);

impl KnownServers {
    /// A file that doesn't read is set aside (".bad") and the pins learned again, so it can never block a login.
    pub fn load(path: &Path) -> Self {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            Err(e) => {
                log::warn!("tls: {}: {e}", path.display());
                return Self::default();
            }
        };
        serde_json::from_str(&text).map(Self).unwrap_or_else(|e| {
            log::warn!("tls: {} does not read ({e}); set aside, its pins are learned again", path.display());
            let _ = std::fs::rename(path, path.with_extension("json.bad"));
            Self::default()
        })
    }

    /// Written through a synced temporary file of this process, so neither a crash nor another client leaves half a
    /// file.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(serde_json::to_string_pretty(&self.0)?.as_bytes())?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)?;
        Ok(())
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

/// "AB:CD:...".
pub fn show(f: &Fingerprint) -> String {
    f.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":")
}

pub fn fingerprint(bytes: &[u8]) -> Fingerprint {
    ring::digest::digest(&ring::digest::SHA256, bytes).as_ref().try_into().unwrap()
}

/// What the server sent: its key's fingerprint, and whether a public authority signs its certificate.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Seen {
    key: Fingerprint,
    authority: bool,
}

/// Whether `pin` names this very certificate: its key (fixed or not), or an authority's signature.
fn names(pin: &Pin, seen: Seen) -> bool {
    match pin {
        Pin::Authority => seen.authority,
        Pin::Key { sha256, .. } => parse_fingerprint(sha256).is_ok_and(|k| k == seen.key),
    }
}

/// Whether a certificate passes its server's pin.
fn accepts(pin: Option<&Pin>, seen: Seen) -> bool {
    match pin {
        None => true,
        Some(Pin::Authority) => seen.authority,
        Some(Pin::Key { sha256, fixed }) => parse_fingerprint(sha256).is_ok_and(|k| k == seen.key) || (seen.authority && !fixed),
    }
}

/// The pin to keep after a handshake that passed, when it changes: the first one, or an authority-signed certificate
/// in place of a learned key.
fn learned(pin: Option<&Pin>, seen: Seen) -> Option<Pin> {
    let now = if seen.authority { Pin::Authority } else { Pin::Key { sha256: hex::encode(seen.key), fixed: false } };
    match pin {
        None => Some(now),
        Some(Pin::Key { fixed: false, .. }) if seen.authority => Some(now),
        Some(_) => None,
    }
}

#[derive(Debug)]
struct Verifier {
    provider: Arc<CryptoProvider>,
    authority: Arc<WebPkiServerVerifier>,
    pin: Option<Pin>,
    /// What the server sent, for the caller to pin or report.
    seen: Arc<Mutex<Option<Seen>>>,
}

impl ServerCertVerifier for Verifier {
    fn verify_server_cert(
        &self,
        cert: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        name: &ServerName<'_>,
        ocsp: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let key = cert_spki(cert).map(fingerprint).ok_or(rustls::Error::InvalidCertificate(rustls::CertificateError::BadEncoding))?;
        let authority = self.authority.verify_server_cert(cert, intermediates, name, ocsp, now).is_ok();
        let seen = Seen { key, authority };
        *self.seen.lock().unwrap() = Some(seen);
        if accepts(self.pin.as_ref(), seen) {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::InvalidCertificate(rustls::CertificateError::ApplicationVerificationFailure))
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

    /// The server must hold the key of the certificate it sent (read from the DER by hand: webpki can't parse v1).
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

/// The provider and the public authorities' verifier (webpki-roots), made once.
fn authorities() -> Result<&'static (Arc<CryptoProvider>, Arc<WebPkiServerVerifier>)> {
    static MADE: OnceLock<(Arc<CryptoProvider>, Arc<WebPkiServerVerifier>)> = OnceLock::new();
    if let Some(m) = MADE.get() {
        return Ok(m);
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
    let verifier = WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider.clone()).build()?;
    Ok(MADE.get_or_init(|| (provider, verifier)))
}

fn client_config(pin: Option<Pin>, seen: Arc<Mutex<Option<Seen>>>) -> Result<Arc<ClientConfig>> {
    let (provider, authority) = authorities()?;
    let config = ClientConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(Verifier { provider: provider.clone(), authority: authority.clone(), pin, seen }))
        .with_no_client_auth();
    Ok(Arc::new(config))
}

pub type TlsStream = StreamOwned<ClientConnection, TcpStream>;

pub fn connect(host: &str, port: u16) -> Result<TlsStream> {
    connect_pinned(&crate::config::known_servers_path(), host, port)
}

/// What a pin or a certificate is, for a refusal.
fn describe(pin: &Pin) -> String {
    match pin {
        Pin::Authority => "one a public authority signs".into(),
        Pin::Key { sha256, .. } => format!("key {}", sha256.get(..16).unwrap_or(sha256).to_ascii_uppercase()),
    }
}

/// connect with the pins in `store`: the handshake runs here, so a changed certificate is refused before anything
/// (a password) is sent, and a new pin is kept once the handshake has passed.
pub fn connect_pinned(store: &Path, host: &str, port: u16) -> Result<TlsStream> {
    let key = key(host, port);
    let pin = KnownServers::load(store).0.get(&key).cloned().or_else(|| session_pin(store, &key));
    let seen: Arc<Mutex<Option<Seen>>> = Default::default();
    let mut tcp = super::tcp(host, port)?;
    // The login server sends nothing back for requests it rejects as malformed.
    tcp.set_read_timeout(Some(std::time::Duration::from_secs(10)))?;
    let name = ServerName::try_from(host.to_string())?;
    let mut conn = ClientConnection::new(client_config(pin.clone(), seen.clone())?, name)?;
    while conn.is_handshaking() {
        if let Err(e) = conn.complete_io(&mut tcp) {
            if let (Some(want), Some(got)) = (&pin, *seen.lock().unwrap())
                && !accepts(Some(want), got)
            {
                let sent = if got.authority { Pin::Authority } else { Pin::Key { sha256: hex::encode(got.key), fixed: false } };
                log::warn!("tls: {key} sent key {} (authority-signed: {}); pinned {want:?}", show(&got.key), got.authority);
                // The caller names the server.
                bail!(
                    "the certificate changed: expected {}, sent {}. Someone may be intercepting the connection. If the \
                     server was reinstalled, run `vana-client trust-server {host}` and log in again.",
                    describe(want),
                    describe(&sent)
                );
            }
            return Err(e.into());
        }
    }
    if let Some(got) = *seen.lock().unwrap()
        && learned(pin.as_ref(), got).is_some()
    {
        record_pin(store, &key, pin.as_ref(), got)?;
    }
    Ok(StreamOwned::new(conn, tcp))
}

/// Keeps what `seen` teaches about `key` after a handshake that passed `was` (the pin read when it began). The auth
/// and profile connections run on their own threads, and trust-server can run meanwhile: the store (or this run's
/// pin) is re-read under a lock and, if its pin changed since `was`, the certificate is judged by the new pin instead
/// (a later fixed pin is never overwritten by a connection that started before it). A pin that can't be written is
/// kept for this run and the player told once; the next run learns it again.
fn record_pin(store: &Path, key: &str, was: Option<&Pin>, seen: Seen) -> Result<()> {
    static PINNING: Mutex<()> = Mutex::new(());
    let _held = PINNING.lock().unwrap_or_else(|e| e.into_inner());
    let mut known = KnownServers::load(store);
    let held = known.0.get(key).cloned().or_else(|| session_pin(store, key));
    let now = held.as_ref();
    if now != was {
        let sent = if seen.authority { Pin::Authority } else { Pin::Key { sha256: hex::encode(seen.key), fixed: false } };
        let have = now.map_or_else(|| "none".into(), describe);
        match was {
            // A first connection keeps a pin made meanwhile (another login, trust-server) that names this certificate.
            None if now.is_some_and(|p| names(p, seen)) => return Ok(()),
            None => bail!(
                "{key} sent two different certificates while it was being pinned (kept {have}, refused {}). Someone \
                 may be intercepting the connection.",
                describe(&sent)
            ),
            Some(_) if !accepts(now, seen) => bail!(
                "{key}'s pin changed while connecting (now {have}), and this connection was sent {}. Log in again.",
                describe(&sent)
            ),
            Some(_) => {}
        }
    }
    let Some(want) = learned(now, seen) else { return Ok(()) };
    log::info!("tls: {key} pinned: {want:?}");
    known.0.insert(key.to_string(), want.clone());
    let mut session = session_pins().lock().unwrap_or_else(|e| e.into_inner());
    let at = (store.to_path_buf(), key.to_string());
    // Only a pin the store can't take is held here: a saved one is the store's (trust-server from another process
    // forgets it there).
    if let Err(e) = known.save(store) {
        session.insert(at, want);
        log::warn!("tls: {key}'s pin not saved: {e:#}");
        let mut n = NOTICE.lock().unwrap_or_else(|e| e.into_inner());
        if !n.0 {
            *n = (true, Some(format!(
                "The server's certificate could not be saved ({}: {e:#}). It is checked until you quit, but the next login can't tell whether it changed.",
                store.display()
            )));
        }
    } else {
        session.remove(&at);
    }
    Ok(())
}

/// trust-server: forgets `host`'s pins (all its ports, or `port`), so the next connection pins what it is sent, or
/// sets a fixed key pin. Returns what changed, for the command to print.
pub fn trust(store: &Path, host: &str, port: Option<u16>, fingerprint: Option<&str>) -> Result<Vec<String>> {
    let mut known = KnownServers::load(store);
    let mut out = Vec::new();
    match fingerprint {
        Some(f) => {
            let port = port.context("--fingerprint needs --port (the login port is usually 54231)")?;
            let f = parse_fingerprint(f)?;
            let k = key(host, port);
            known.0.insert(k.clone(), Pin::Key { sha256: hex::encode(f), fixed: true });
            out.push(format!("{k}: pinned to key {}", show(&f)));
        }
        None => {
            let prefix = format!("{}:", host.trim().to_ascii_lowercase());
            let keys: Vec<String> = known.0.keys().filter(|k| k.starts_with(&prefix) && port.is_none_or(|p| **k == key(host, p))).cloned().collect();
            for k in keys {
                if matches!(known.0[&k], Pin::Key { fixed: true, .. }) && port.is_none() {
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
    // This run's pins for the host go too, so the store's say what holds.
    let prefix = format!("{}:", host.trim().to_ascii_lowercase());
    session_pins().lock().unwrap_or_else(|e| e.into_inner()).retain(|(s, k), _| !(s == store && k.starts_with(&prefix) && port.is_none_or(|p| *k == key(host, p))));
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

    /// A DigitallySignedStruct from its wire form: scheme, length, signature.
    fn dss_of(scheme: u16, sig: &[u8]) -> DigitallySignedStruct {
        let mut wire = scheme.to_be_bytes().to_vec();
        wire.extend((sig.len() as u16).to_be_bytes());
        wire.extend_from_slice(sig);
        <DigitallySignedStruct as rustls::internal::msgs::codec::Codec>::read_bytes(&wire).unwrap()
    }

    /// A minimal v1 or v3 certificate around `spki` (only the structure matters here; nothing checks its signature).
    fn cert_with(spki: &[u8], v3: bool) -> Vec<u8> {
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

        let (provider, authority) = authorities().unwrap();
        let verifier = Verifier { provider: provider.clone(), authority: authority.clone(), pin: None, seen: Default::default() };
        let msg = b"TLS 1.3, server CertificateVerify";
        // ecdsa_secp256r1_sha256.
        let dss = |k: &EcdsaKeyPair| dss_of(0x0403, k.sign(&rng, msg).unwrap().as_ref());
        for v3 in [false, true] {
            let cert = CertificateDer::from(cert_with(&spki, v3));
            assert!(verifier.verify_tls13_signature(msg, &cert, &dss(&ours)).is_ok());
            // Someone in the path who sends this cert without its key is refused.
            assert!(verifier.verify_tls13_signature(msg, &cert, &dss(&theirs)).is_err());
            assert!(verifier.verify_tls13_signature(b"another transcript", &cert, &dss(&ours)).is_err());
        }
        assert!(verifier.verify_tls13_signature(msg, &CertificateDer::from(vec![0x30, 0x00]), &dss(&ours)).is_err());
    }

    /// An RSA certificate's handshake (LSB's login port): rsa_pss_rsae_sha256 checks, PKCS#1 v1.5 is refused in 1.3.
    #[test]
    fn rsa_pss_handshake_signature() {
        let rng = ring::rand::SystemRandom::new();
        let key = ring::signature::RsaKeyPair::from_der(include_bytes!("testdata/rsa-key.der")).unwrap();
        // SubjectPublicKeyInfo: rsaEncryption, NULL, the RSAPublicKey as a bit string.
        let alg = tlv(0x30, &[0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01, 0x05, 0x00]);
        let mut bits = vec![0];
        bits.extend_from_slice(key.public().as_ref());
        let spki = tlv(0x30, &[alg, tlv(0x03, &bits)].concat());
        let cert = CertificateDer::from(cert_with(&spki, false));
        let (provider, authority) = authorities().unwrap();
        let verifier = Verifier { provider: provider.clone(), authority: authority.clone(), pin: None, seen: Default::default() };
        let msg = b"TLS 1.3, server CertificateVerify";
        let sign = |alg: &'static dyn ring::signature::RsaEncoding| {
            let mut sig = vec![0; key.public().modulus_len()];
            key.sign(alg, &rng, msg, &mut sig).unwrap();
            sig
        };
        let pss = sign(&ring::signature::RSA_PSS_SHA256);
        assert!(verifier.verify_tls13_signature(msg, &cert, &dss_of(0x0804, &pss)).is_ok());
        assert!(verifier.verify_tls13_signature(b"another transcript", &cert, &dss_of(0x0804, &pss)).is_err());
        assert!(verifier.verify_tls13_signature(msg, &cert, &dss_of(0x0401, &sign(&ring::signature::RSA_PKCS1_SHA256))).is_err());
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
        let learned_key = |f: Fingerprint| Pin::Key { sha256: hex::encode(f), fixed: false };
        let mut known = KnownServers::default();
        known.0.insert(key("h", 1), learned_key(f));
        known.0.insert(key("h", 2), Pin::Key { sha256: hex::encode(f), fixed: true });
        known.0.insert(key("h", 4), Pin::Authority);
        known.0.insert(key("other", 1), learned_key(f));
        known.save(&store).unwrap();
        // Without --port: learned pins go, a fixed one stays, other hosts are untouched.
        trust(&store, "H", None, None).unwrap();
        let k = KnownServers::load(&store);
        assert_eq!(k.0.keys().collect::<Vec<_>>(), ["h:2", "other:1"]);
        // --port forgets a fixed one too; --fingerprint sets one (and needs --port).
        trust(&store, "h", Some(2), None).unwrap();
        assert!(trust(&store, "h", None, Some(&show(&f))).is_err());
        trust(&store, "h", Some(3), Some(&show(&f))).unwrap();
        let k = KnownServers::load(&store);
        assert_eq!(k.0.get("h:3"), Some(&Pin::Key { sha256: hex::encode(f), fixed: true }));
        assert!(!k.0.contains_key("h:2"));
        // A first connection that finds a pin made meanwhile keeps it, and refuses a different certificate.
        let g = fingerprint(b"other cert");
        let self_signed = |k| Seen { key: k, authority: false };
        record_pin(&store, "new:1", None, self_signed(f)).unwrap();
        record_pin(&store, "new:1", None, self_signed(f)).unwrap();
        assert!(record_pin(&store, "new:1", None, self_signed(g)).is_err());
        assert!(record_pin(&store, "new:1", None, Seen { key: g, authority: true }).is_err());
        assert_eq!(KnownServers::load(&store).0.get("new:1"), Some(&learned_key(f)));
        // An authority-signed certificate replaces a learned key.
        record_pin(&store, "new:1", Some(&learned_key(f)), Seen { key: g, authority: true }).unwrap();
        assert_eq!(KnownServers::load(&store).0.get("new:1"), Some(&Pin::Authority));
        // A file that doesn't read is set aside, not an error.
        std::fs::write(&store, "{ not json").unwrap();
        assert!(KnownServers::load(&store).0.is_empty());
        assert!(store.with_extension("json.bad").is_file());
        trust(&store, "h", None, None).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Which certificates a pin lets through, and what is pinned after.
    #[test]
    fn pin_rules() {
        let (a, b) = (fingerprint(b"a"), fingerprint(b"b"));
        let self_signed = |k| Seen { key: k, authority: false };
        let signed = |k| Seen { key: k, authority: true };
        let learned_a = Pin::Key { sha256: hex::encode(a), fixed: false };
        let fixed_a = Pin::Key { sha256: hex::encode(a), fixed: true };
        // First use: anything, pinned as what it is.
        assert!(accepts(None, self_signed(a)));
        assert_eq!(learned(None, self_signed(a)), Some(learned_a.clone()));
        assert_eq!(learned(None, signed(a)), Some(Pin::Authority));
        // A key pin: that key; an authority-signed certificate replaces a learned key, never a fixed one.
        assert!(accepts(Some(&learned_a), self_signed(a)));
        assert!(!accepts(Some(&learned_a), self_signed(b)));
        assert!(accepts(Some(&learned_a), signed(b)));
        assert_eq!(learned(Some(&learned_a), signed(b)), Some(Pin::Authority));
        assert!(!accepts(Some(&fixed_a), signed(b)));
        assert_eq!(learned(Some(&learned_a), self_signed(a)), None);
        // Authority-signed once: a self-signed certificate is refused, whatever its key.
        assert!(accepts(Some(&Pin::Authority), signed(b)));
        assert!(!accepts(Some(&Pin::Authority), self_signed(a)));
        assert_eq!(learned(Some(&Pin::Authority), signed(a)), None);
    }

    /// A pin that changes between a connection's start and its record_pin is judged afresh, never overwritten.
    #[test]
    fn pin_changed_while_connecting() {
        let dir = std::env::temp_dir().join(format!("vana-tls-race-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = dir.join("known_servers.json");
        let k = "example.org:54231";
        let (a, b, c) = (fingerprint(b"a"), fingerprint(b"b"), fingerprint(b"c"));
        let learned_a = Pin::Key { sha256: hex::encode(a), fixed: false };
        let fixed_b = Pin::Key { sha256: hex::encode(b), fixed: true };
        let set = |p: Option<&Pin>| {
            let mut known = KnownServers::default();
            if let Some(p) = p {
                known.0.insert(k.into(), p.clone());
            }
            known.save(&store).unwrap();
        };
        let get = || KnownServers::load(&store).0.get(k).cloned();
        // Started on a learned key; trust-server --fingerprint fixed another meanwhile. An authority-signed
        // certificate with a third key no longer replaces it: refused, and the fixed pin stays.
        set(Some(&fixed_b));
        let e = format!("{:#}", record_pin(&store, k, Some(&learned_a), Seen { key: c, authority: true }).unwrap_err());
        assert!(e.contains("pin changed while connecting"), "{e}");
        assert_eq!(get(), Some(fixed_b.clone()));
        // ...but the fixed key itself, authority-signed, passes and leaves it as it is.
        record_pin(&store, k, Some(&learned_a), Seen { key: b, authority: true }).unwrap();
        assert_eq!(get(), Some(fixed_b));
        // Unchanged: the authority-signed certificate replaces the learned key as before.
        set(Some(&learned_a));
        record_pin(&store, k, Some(&learned_a), Seen { key: c, authority: true }).unwrap();
        assert_eq!(get(), Some(Pin::Authority));
        // First use on two threads: the same key is kept once, another key is refused.
        set(Some(&learned_a));
        record_pin(&store, k, None, Seen { key: a, authority: false }).unwrap();
        assert_eq!(get(), Some(learned_a.clone()));
        let e = format!("{:#}", record_pin(&store, k, None, Seen { key: b, authority: false }).unwrap_err());
        assert!(e.contains("two different certificates"), "{e}");
        // First use while trust-server fixed this very key: kept as it is, no alarm.
        let fixed_a = Pin::Key { sha256: hex::encode(a), fixed: true };
        set(Some(&fixed_a));
        record_pin(&store, k, None, Seen { key: a, authority: false }).unwrap();
        assert_eq!(get(), Some(fixed_a));
        // Nothing stored yet: first use pins it.
        set(None);
        record_pin(&store, k, None, Seen { key: a, authority: false }).unwrap();
        assert_eq!(get(), Some(learned_a));
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
            fingerprint(cert_spki(&ring_b64(&b64)).unwrap())
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
        let pinned = |store: &Path| match KnownServers::load(store).0.get(&key("127.0.0.1", port)) {
            Some(Pin::Key { sha256, .. }) => parse_fingerprint(sha256).ok(),
            _ => None,
        };
        assert_eq!(pinned(&store), Some(der("a")));
        attempt("a").unwrap();
        let e = format!("{:#}", attempt("b").unwrap_err());
        assert!(e.contains("certificate changed") && e.contains(&hex::encode(der("a"))[..16].to_ascii_uppercase()), "{e}");
        // trust-server forgets it: b is pinned next.
        trust(&store, "127.0.0.1", Some(port), None).unwrap();
        attempt("b").unwrap();
        assert_eq!(pinned(&store), Some(der("b")));
        // A fixed fingerprint holds from the first connection.
        trust(&store, "127.0.0.1", Some(port), Some(&show(&der("a")))).unwrap();
        assert!(attempt("b").is_err());
        attempt("a").unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A pin the store can't take holds for the rest of the run, and the player is told once.
    #[test]
    fn unsaved_pin_holds_this_run() {
        let dir = std::env::temp_dir().join(format!("vana-tls-unsaved-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // A file where the store's folder should be: every save fails.
        std::fs::write(dir.join("not-a-dir"), b"").unwrap();
        let store = dir.join("not-a-dir/known_servers.json");
        let a = Pin::Key { sha256: "aa".repeat(32), fixed: false };
        let (seen_a, seen_b) = (Seen { key: [0xaa; 32], authority: false }, Seen { key: [0xbb; 32], authority: false });
        record_pin(&store, "lsb:54231", None, seen_a).unwrap();
        assert_eq!(session_pin(&store, "lsb:54231"), Some(a.clone()));
        assert!(take_notice().is_some_and(|n| n.contains("could not be saved")));
        assert!(take_notice().is_none());
        // Another first connection is judged against this run's pin, as against a stored one.
        assert!(record_pin(&store, "lsb:54231", None, seen_b).is_err());
        record_pin(&store, "lsb:54231", None, seen_a).unwrap();
        assert!(take_notice().is_none());
        trust(&store, "lsb", None, None).ok();
        assert_eq!(session_pin(&store, "lsb:54231"), None);
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
