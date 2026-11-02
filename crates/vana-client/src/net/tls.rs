//! TLS 1.3 client used for xi_connect auth and xi_profile. Private servers run
//! self-signed X.509 v1 certificates that webpki cannot parse, so the certificate itself is
//! not checked (xiloader does the same). The handshake signature is: the server must hold the
//! private key of the certificate it sent, its public key read out of the DER by hand.

use anyhow::Result;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, SubjectPublicKeyInfoDer, UnixTime};
use rustls::{ClientConfig, ClientConnection, DigitallySignedStruct, SignatureScheme, StreamOwned};
use std::net::TcpStream;
use std::sync::Arc;

#[derive(Debug)]
struct AnyCert(Arc<CryptoProvider>);

impl ServerCertVerifier for AnyCert {
    fn verify_server_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
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
        rustls::crypto::verify_tls13_signature_with_raw_key(message, &SubjectPublicKeyInfoDer::from(spki), dss, &self.0.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
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

pub fn client_config() -> Result<Arc<ClientConfig>> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AnyCert(provider)))
        .with_no_client_auth();
    Ok(Arc::new(config))
}

pub type TlsStream = StreamOwned<ClientConnection, TcpStream>;

pub fn connect(host: &str, port: u16) -> Result<TlsStream> {
    let tcp = TcpStream::connect((host, port))?;
    // The login server sends nothing back for requests it rejects as malformed.
    tcp.set_read_timeout(Some(std::time::Duration::from_secs(10)))?;
    let name = ServerName::try_from(host.to_string())?;
    let conn = ClientConnection::new(client_config()?, name)?;
    Ok(StreamOwned::new(conn, tcp))
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
        let verifier = AnyCert(provider);
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
    #[ignore = "needs a TLS server at TLS_E2E (host:port)"]
    fn e2e_handshake() {
        use std::io::{Read, Write};
        let addr = std::env::var("TLS_E2E").unwrap();
        let (h, p) = addr.rsplit_once(':').unwrap();
        let mut s = connect(h, p.parse().unwrap()).unwrap();
        s.write_all(b"ping\n").unwrap();
        let mut b = [0u8; 4];
        s.read_exact(&mut b).unwrap();
        assert_eq!(&b, b"pong");
    }
}
