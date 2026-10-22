//! Animations packed as u32 words for the GPU posing pass (render/anim.rs), built on the loader threads so the
//! UI thread only appends them. Every offset is relative to its animation's start, so a block can go anywhere.

use super::character::{Animation, Channel};

/// Bone -> track table size per animation (render/anim.rs TABLE).
pub const TABLE: usize = 256;

/// Words of a list of animations, each one's start in `words`, and a hash of the words: models of one race
/// carry the same motions, and the GPU keeps one copy.
#[derive(Debug, Default)]
pub struct Packed {
    pub words: Vec<u32>,
    pub offsets: Vec<u32>,
    pub key: u64,
}

/// One animation: frames, track count, bone -> track + 1 table, then 10 channels of (key count or 0,
/// constant bits or key offset from the animation start) per track, keys after.
fn pack_one(a: &Animation, out: &mut Vec<u32>) {
    let start = out.len();
    out.push(a.frames as u32);
    out.push(a.tracks.len() as u32);
    out.extend(std::iter::repeat_n(0u32, TABLE));
    for (i, t) in a.tracks.iter().enumerate() {
        if t.bone < TABLE {
            out[start + 2 + t.bone] = i as u32 + 1;
        }
    }
    let chans_at = out.len();
    out.extend(std::iter::repeat_n(0u32, a.tracks.len() * 20));
    for (i, t) in a.tracks.iter().enumerate() {
        for (c, ch) in t.rot.iter().chain(&t.trans).chain(&t.scale).enumerate() {
            let at = chans_at + i * 20 + c * 2;
            match ch {
                Channel::Const(v) => out[at + 1] = v.to_bits(),
                Channel::Keys(k) => {
                    out[at] = k.len() as u32;
                    out[at + 1] = (out.len() - start) as u32;
                    out.extend(k.iter().map(|v| v.to_bits()));
                }
            }
        }
    }
}

pub fn pack(anims: &[Animation]) -> Packed {
    let mut p = Packed::default();
    for a in anims {
        p.offsets.push(p.words.len() as u32);
        pack_one(a, &mut p.words);
    }
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    p.words.hash(&mut h);
    p.offsets.hash(&mut h);
    p.key = h.finish();
    p
}
