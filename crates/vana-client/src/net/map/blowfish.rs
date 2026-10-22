//! FFXI's Blowfish variant. It differs from the standard cipher in two ways the server
//! relies on: the round function keeps only the low bit of the S1/S3 lookups, and key
//! bytes are sign-extended while mixing into P.

use super::bf_consts;
use md5::{Digest, Md5};

#[derive(Clone)]
pub struct Blowfish {
    p: [u32; 18],
    s: [[u32; 256]; 4],
}

impl Blowfish {
    pub fn new(key: &[u8]) -> Self {
        let mut bf = Self { p: bf_consts::P, s: bf_consts::S };
        let mut j = 0;
        for i in 0..18 {
            let mut data = 0u32;
            for _ in 0..4 {
                data = (data << 8) | (key[j] as i8 as i32 as u32);
                j = (j + 1) % key.len();
            }
            bf.p[i] ^= data;
        }
        let (mut l, mut r) = (0, 0);
        for i in (0..18).step_by(2) {
            (l, r) = bf.encipher(l, r);
            bf.p[i] = l;
            bf.p[i + 1] = r;
        }
        for b in 0..4 {
            for i in (0..256).step_by(2) {
                (l, r) = bf.encipher(l, r);
                bf.s[b][i] = l;
                bf.s[b][i + 1] = r;
            }
        }
        bf
    }

    /// The cipher for a 20-byte map session key: MD5 of the key, cut at its first zero byte.
    pub fn for_session_key(key: &[u8; 20]) -> Self {
        let mut hash: [u8; 16] = Md5::digest(key).into();
        if let Some(zero) = hash.iter().position(|&b| b == 0) {
            hash[zero..].fill(0);
        }
        Self::new(&hash)
    }

    fn f(&self, x: u32) -> u32 {
        let s = &self.s;
        ((s[1][((x >> 8) & 0xff) as usize] & 1) ^ 32)
            .wrapping_add((s[3][(x >> 24) as usize] & 1) ^ 32)
            .wrapping_add(s[2][((x >> 16) & 0xff) as usize])
            .wrapping_add(s[0][(x & 0xff) as usize])
    }

    fn encipher(&self, mut l: u32, mut r: u32) -> (u32, u32) {
        for i in 0..16 {
            l ^= self.p[i];
            r ^= self.f(l);
            std::mem::swap(&mut l, &mut r);
        }
        (r ^ self.p[17], l ^ self.p[16])
    }

    fn decipher(&self, mut l: u32, mut r: u32) -> (u32, u32) {
        for i in (2..18).rev() {
            l ^= self.p[i];
            r ^= self.f(l);
            std::mem::swap(&mut l, &mut r);
        }
        (r ^ self.p[0], l ^ self.p[1])
    }

    /// Enciphers whole 8-byte blocks in place (ECB, little-endian words); a tail stays plain.
    pub fn encrypt(&self, data: &mut [u8]) {
        self.blocks(data, Self::encipher);
    }

    pub fn decrypt(&self, data: &mut [u8]) {
        self.blocks(data, Self::decipher);
    }

    fn blocks(&self, data: &mut [u8], op: fn(&Self, u32, u32) -> (u32, u32)) {
        for block in data.chunks_exact_mut(8) {
            let l = u32::from_le_bytes(block[..4].try_into().unwrap());
            let r = u32::from_le_bytes(block[4..].try_into().unwrap());
            let (l, r) = op(self, l, r);
            block[..4].copy_from_slice(&l.to_le_bytes());
            block[4..].copy_from_slice(&r.to_le_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vectors from LSB's blowfish.cpp, keyed with bytes >= 0x80 to cover sign extension.
    #[test]
    fn matches_server_cipher() {
        let key: [u8; 20] = std::array::from_fn(|i| (i as u32 * 37 + 0x81) as u8);
        let bf = Blowfish::for_session_key(&key);
        let mut data: Vec<u8> = (1..=8u32).flat_map(|i| 0x01020304u32.wrapping_mul(i).to_le_bytes()).collect();
        bf.encrypt(&mut data);
        let words: Vec<u32> = data.chunks(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
        assert_eq!(
            words,
            [0x111edfaa, 0xb682b3d2, 0x3318739e, 0x92a2be12, 0x0af2eb55, 0xd3a3a53d, 0x12be7d65, 0x624e929b]
        );
    }

    #[test]
    fn round_trip() {
        let bf = Blowfish::for_session_key(&[0x9c; 20]);
        let plain: Vec<u8> = (0..64u8).collect();
        let mut data = plain.clone();
        bf.encrypt(&mut data);
        assert_ne!(data, plain);
        bf.decrypt(&mut data);
        assert_eq!(data, plain);
    }
}
