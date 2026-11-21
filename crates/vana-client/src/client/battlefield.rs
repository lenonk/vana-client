//! Battlefield info: c2s 0x112 (GP_CLI_COMMAND_BATTLEFIELD_REQ) and its reply s2c 0x072 (FFXiMain 30261102_0).
//!
//! mapframe's slot 0x30 (win_mapframe_v30 -> FUN_101fbc90(.., 0)) asks with kind 0 at every zone-in, from the handler
//! walk FUN_101604c0 (client/mod.rs `zone_walk`); a map of zone 0x11e set up (FUN_101fb060 -> FUN_101fc100) asks with
//! kind 2. Kind 1 has no caller (FUN_101fbc90 is called with 0 and 2 only). The senders set one-shot reply callbacks
//! the s2c 0x072 handler (FUN_100999a0) runs: kind 0 -> FUN_101fbd00 (the sidebar block), 1 -> FUN_101fbdd0 (the map
//! overlay). The zone load (FUN_10093720) clears both callbacks.
//!
//! The readers, both for zone 0x11e only: the sidebar (compass draw FUN_102219c0 -> FUN_10220a20 -> FUN_10220f60, our
//! team byte 8 / 9) and the map overlay (win_mapframe_v10 -> FUN_101fc130 -> FUN_101fc150).

use super::World;
use super::game_ui::GameUi;
use super::net::NetCommand;
use std::time::Instant;

/// The zone both readers and the kind 2 send are gated to (session +0x3c400 / mapframe +0x100).
pub const ZONE: u16 = 0x11e;

/// The one c2s 0x112 sender FUN_101fbc90(.., kind) picks: 0 FUN_10098080, 1 FUN_100980c0, 2 FUN_10098100.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Both blocks (the zone-in's).
    Both = 0,
    #[allow(dead_code)] // FUN_100980c0 has no caller
    Sidebar = 1,
    /// FUN_101fc100's, for a map of zone 0x11e.
    MapOverlay = 2,
}

/// FUN_101fbd00's block: three records of four words from 0x10664964 (record 0 stays zero), a flag (_DAT_10664994)
/// and a 3-bit value (DAT_10664998).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sidebar {
    pub records: [[u32; 4]; 3],
    #[allow(dead_code)] // FUN_101f8790, no caller
    pub flag: u32,
    pub mode: u32,
}

/// FUN_101fbdd0's block: ten records of four words from 0x1066499c.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Overlay {
    pub records: [[i32; 4]; 10],
}

#[derive(Default)]
pub struct Battlefield {
    /// DAT_1048904c / DAT_10489050: the reply callbacks set, one-shot.
    sidebar_cb: bool,
    overlay_cb: bool,
    /// The blocks; None while not valid (DAT_10664960 / DAT_10664961).
    pub sidebar: Option<Sidebar>,
    pub overlay: Option<Overlay>,
    /// The compass struct's gauge bytes (DAT_1066803c +1, +2, +4, +5, +6 by index), zeroed by FUN_102204c0; each
    /// eases after its draw (FUN_101cc4d0 / FUN_101cca00) at retail's 30 a second.
    gauges: [u8; 7],
    acc: f32,
    last: Option<Instant>,
    /// The sidebar was drawn (and eased) in the last HUD frame.
    drawn: bool,
}

fn word(p: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(std::array::from_fn(|i| p.get(o + i).copied().unwrap_or(0)))
}

/// One packed word of the sidebar block (FUN_101fbd00): 7, 10, 14 and 1 bits.
fn split(v: u32) -> [u32; 4] {
    [v & 0x7f, v >> 7 & 0x3ff, v >> 17 & 0x3fff, v >> 31]
}

impl Sidebar {
    /// FUN_101fbd00 over the reply from @8: record 1 from the word @0xc, record 2 from @8, the flag and mode from @0x10.
    fn decode(p: &[u8]) -> Self {
        let c = word(p, 0x10);
        Sidebar { records: [[0; 4], split(word(p, 0xc)), split(word(p, 8))], flag: c & 1, mode: c >> 1 & 7 }
    }
}

impl Overlay {
    /// FUN_101fbdd0 over the reply from @8: byte i, then the word @0x14 + 4i's low 2 bits, bits 2..16 and 17..31,
    /// both signed.
    fn decode(p: &[u8]) -> Self {
        Overlay {
            records: std::array::from_fn(|i| {
                let w = word(p, 0x14 + 4 * i);
                [p.get(8 + i).copied().unwrap_or(0) as i32, (w & 3) as i32, ((w << 15) as i32) >> 17, (w as i32) >> 17]
            }),
        }
    }
}

impl Battlefield {
    /// FUN_101fbc90 with the sender's gate: kind 0 always goes and sets both callbacks; kinds 1 and 2 only while the
    /// sidebar callback is clear (FUN_10098100 checks DAT_1048904c too, though it sets the overlay's). A sent kind
    /// marks its blocks not valid. The c2s 0x112 body (4 bytes, kind @4) when one goes out.
    pub fn request(&mut self, kind: Kind) -> Option<Vec<u8>> {
        match kind {
            Kind::Both => {
                (self.sidebar_cb, self.overlay_cb, self.sidebar, self.overlay) = (true, true, None, None);
            }
            _ if self.sidebar_cb => return None,
            Kind::Sidebar => (self.sidebar_cb, self.sidebar) = (true, None),
            Kind::MapOverlay => (self.overlay_cb, self.overlay) = (true, None),
        }
        Some(vec![kind as u8, 0, 0, 0])
    }

    /// s2c 0x072 (FUN_100999a0): Kind @4, 0 the sidebar block, 1 the overlay, each taken only while its callback is
    /// set; the callback is cleared.
    pub fn on_reply(&mut self, p: &[u8]) {
        match p.get(4) {
            Some(0) if self.sidebar_cb => (self.sidebar_cb, self.sidebar) = (false, Some(Sidebar::decode(p))),
            Some(1) if self.overlay_cb => (self.overlay_cb, self.overlay) = (false, Some(Overlay::decode(p))),
            _ => {}
        }
    }

    /// FUN_10093720 (the zone load): both callbacks cleared; the blocks stay.
    pub fn zone_load(&mut self) {
        (self.sidebar_cb, self.overlay_cb) = (false, false);
    }
}

impl Battlefield {
    /// FUN_101f8810: record i's first field, 0 while the block is not valid.
    fn side_f0(&self, i: usize) -> u32 {
        self.sidebar.as_ref().map_or(0, |s| s.records[i][0])
    }

    /// FUN_101f8870: FUN_101f8850 (record i's third field) x 100 / 15000, unsigned.
    fn side_pct(&self, i: usize) -> u32 {
        self.sidebar.as_ref().map_or(0, |s| s.records[i][2]) * 100 / 15000
    }

    /// FUN_101f87d0(i, j): record i's second field x 100 over i's and j's; 0 while not valid. Both zero: retail divides
    /// by zero (an integer divide fault); ours gives 0.
    fn side_share(&self, i: usize, j: usize) -> u32 {
        let Some(s) = &self.sidebar else { return 0 };
        let (a, b) = (s.records[i][1], s.records[j][1]);
        (a * 100).checked_div(b + a).unwrap_or(0)
    }

    /// FUN_101f87b0: the block's 3-bit value, 0 while not valid.
    fn side_mode(&self) -> u32 {
        self.sidebar.as_ref().map_or(0, |s| s.mode)
    }

    /// FUN_101fc100 from the mapframe's setup (FUN_101fb060): kind 2 when FUN_101fc0c0 holds for the new map.
    fn map_request(&mut self, map_zone: u16, player: bool, title_up: bool) -> Option<Vec<u8>> {
        overlay_shown(map_zone, player, title_up).then(|| self.request(Kind::MapOverlay)).flatten()
    }

    /// Once a HUD frame, before the draw: a frame the sidebar was not drawn in eased nothing (retail eases only in the
    /// compass draw), so the time since then is not paid back.
    pub fn frame(&mut self) {
        if !std::mem::take(&mut self.drawn) {
            (self.last, self.acc) = (None, 0.0);
        }
    }

    /// The gauges' ease after a draw, at 30 a second (FUN_101cc4d0 / FUN_101cca00 run once a draw at retail's rate);
    /// the first draw after a gap takes one step, as retail's does.
    pub fn ease(&mut self, targets: Eases, now: Instant) {
        self.acc += self.last.map_or(1.0 / 30.0, |t| (now - t).as_secs_f32().min(1.0));
        (self.last, self.drawn) = (Some(now), true);
        while self.acc >= 1.0 / 30.0 {
            self.acc -= 1.0 / 30.0;
            for (k, v) in targets {
                self.gauges[k] = ease(self.gauges[k], v);
            }
        }
    }
}

/// The sidebar's gauge bytes by index and the values they ease toward.
pub type Eases = [(usize, u8); 5];

/// FUN_101cc4d0 / FUN_101cca00's step: st + trunc((v - st) x 0.15), a byte add; v itself once that is 0.
fn ease(st: u8, v: u8) -> u8 {
    let d = ((v as i32 - st as i32) as f32 * 0.15) as i32;
    if d == 0 { v } else { st.wrapping_add(d as u8) }
}

/// FUN_10220a20: our entity's +0x1d0 bit 10 (FUN_1008e290: team byte +0x1ff in 2..99) and +0x1ff 8 or 9 (not
/// 0x28..0x2b), in zone 0x11e (session +0x3c400). Any other team byte in 2..99 draws only FUN_10220a90's Conflict
/// sidebar, which is not built.
pub fn sidebar_shown(team: u8, zone: u16) -> bool {
    matches!(team, 8 | 9) && zone == ZONE
}

/// FUN_101fc0c0: the map's record (+0xfc >= 0), its zone (+0x100) 0x11e, titlewin open (DAT_10579638 +8) and the
/// player's record set (+0x98, FUN_101fa4b0: our `show_player`).
pub fn overlay_shown(map_zone: u16, player: bool, title_up: bool) -> bool {
    map_zone == ZONE && player && title_up
}

/// FUN_101fb060 -> FUN_101fc100 for the map just set up (every `w.map = Some(..)`).
pub fn map_set_up(w: &mut World) {
    let Some(m) = w.map.as_ref() else { return };
    let (zone, player) = (m.zone(), m.shows_player());
    if let Some(body) = w.battlefield.map_request(zone, player, w.title_up) {
        let _ = w.net.commands.send(NetCommand::Packet { id: 0x112, body });
    }
}

const WINDOWPS: &str = "menu    windowps";
const WHITE: u32 = 0x8080_8080;

fn text(gu: &mut GameUi, s: &[u8], x: f32, y: f32, tint: u32) {
    let (bt, ui, _, _) = gu.parts();
    crate::render::screens::push_shape_text(bt, ui, s, x, y, tint);
}

fn fill_width(gu: &GameUi, fill: usize) -> f32 {
    gu.cell_bounds(WINDOWPS, fill, 0).map_or(0.0, |b| b[2] - b[0])
}

/// Outside [0, 1] counts as 0 (FUN_101cc2d0 / FUN_101cc850).
fn unit(f: f32) -> f32 {
    if (0.0..=1.0).contains(&f) { f } else { 0.0 }
}

fn rounds_to_zero(f: f32) -> bool {
    (f * 100.0 + 0.5) as i32 == 0
}

/// FUN_101cc2d0: the fill (blend 8) to `lo` scaled by `sx`, the ghost (0x30808080) from there to `hi` (its x at the
/// fill's width x lo, unscaled), each only when it rounds to 1 % or more; the frame dimmed 0x30505050 while lo is 0.
fn gauge(gu: &mut GameUi, x: f32, y: f32, (fill, frame): (usize, usize), lo: f32, hi: f32, sx: f32) {
    let (lo, hi) = (unit(lo), unit(hi));
    if !rounds_to_zero(lo) {
        gu.sprite_scaled_blend(WINDOWPS, fill, x + 4.0, y + 1.0, lo * sx, 1.0, WHITE, 8);
    }
    if !rounds_to_zero(hi) {
        let at = (fill_width(gu, fill) * lo) as i32 as f32;
        gu.sprite_scaled_blend(WINDOWPS, fill, x + 4.0 + at, y + 1.0, (hi - lo) * sx, 1.0, 0x3080_8080, 0);
    }
    gu.sprite(WINDOWPS, frame, x, y, if lo == 0.0 { 0x3050_5050 } else { WHITE });
}

/// FUN_101cc4d0: value `v` (percent) against the eased byte `st`: equal, the fill to v; else the fill to the smaller
/// and the ghost to the larger.
fn eased_gauge(gu: &mut GameUi, x: f32, y: f32, sprites: (usize, usize), v: u8, st: u8, sx: f32) {
    let (lo, hi) = if v == st { (v as f32 * 0.01, 0.0) } else { (v.min(st) as f32 * 0.01, v.max(st) as f32 * 0.01) };
    gauge(gu, x, y, sprites, lo, hi, sx);
}

/// FUN_101ccb20 -> FUN_101cca00 -> FUN_101cc850: windowps 97 in two colours, 0x80001080 to f = min(v, st) x 0.01 and
/// 0x80801000 for the rest (1 - f), then frame 38.
fn tug(gu: &mut GameUi, x: f32, y: f32, v: u8, st: u8) {
    let raw = v.min(st) as f32 * 0.01;
    let (f, rest) = (unit(raw), unit(1.0 - raw));
    if !rounds_to_zero(f) {
        gu.sprite_scaled_blend(WINDOWPS, 97, x + 4.0, y + 1.0, f, 1.0, 0x8000_1080, 8);
    }
    if !rounds_to_zero(rest) {
        let at = (fill_width(gu, 97) * f) as i32 as f32;
        gu.sprite_scaled_blend(WINDOWPS, 97, x + 4.0 + at, y + 1.0, rest, 1.0, 0x8080_1000, 8);
    }
    gu.sprite(WINDOWPS, 38, x, y, WHITE);
}

/// FUN_102210a0: the side's mark (sprite font), its value gauge (windowps 97 / 38) at (+0x1a, +7), "Base" in the side's
/// name colour at (+0x14, 0), and its percentage gauge (windowps 98 / 39, the fill 0.7 wide) at (+0x24, +0x11).
#[allow(clippy::too_many_arguments)]
fn side(gu: &mut GameUi, x: f32, y: f32, mark: u8, colour: u32, v: (u8, u8), pct: (u8, u8)) {
    text(gu, &[mark], x, y, WHITE);
    eased_gauge(gu, x + 26.0, y + 7.0, (97, 38), v.0, v.1, 1.0);
    text(gu, b"Base", x + 20.0, y, colour);
    eased_gauge(gu, x + 36.0, y + 17.0, (98, 39), pct.0, pct.1, 0.7);
}

/// FUN_10220f60 (from the compass draw, before the objective bars) at the objective anchor (FUN_10220970): side 1 at
/// +4, the share of record 1 against 2 at +0x20 (FUN_10221190), side 2 at +0x34, and the mode at +0x52
/// (FUN_10221120: glyph 0xaa, "x %02d" at +0x10). Side 1 is glyph 0xa9 in name colour 0x13, side 2 glyph 0xab in 0x12.
pub fn draw_sidebar(gu: &mut GameUi, w: &World) -> Option<Eases> {
    if !sidebar_shown(w.my_team, w.pos_zone) {
        return None;
    }
    let (x, y) = (16.0, super::objective::anchor(gu, w));
    let b = &w.battlefield;
    let g = b.gauges;
    let (v1, p1, share, v2, p2) =
        (b.side_f0(1) as u8, b.side_pct(1) as u8, b.side_share(1, 2) as u8, b.side_f0(2) as u8, b.side_pct(2) as u8);
    let mode = b.side_mode();
    let (c13, c12) = (gu.ncol(0x13), gu.ncol(0x12));
    side(gu, x, y + 4.0, 0xa9, c13, (v1, g[1]), (p1, g[4]));
    tug(gu, x + 16.0, y + 32.0 + 7.0, share, g[6]);
    text(gu, &[0xa9], x, y + 32.0, WHITE);
    text(gu, &[0xab], x + 80.0, y + 32.0, WHITE);
    side(gu, x, y + 52.0, 0xab, c12, (v2, g[2]), (p2, g[5]));
    text(gu, &[0xaa], x, y + 82.0, WHITE);
    text(gu, format!("x {mode:02}").as_bytes(), x + 16.0, y + 82.0, WHITE);
    Some([(1, v1), (4, p1), (6, share), (2, v2), (5, p2)])
}

/// FUN_101fc130 -> FUN_101fc150 for i in 0..10 while the block is valid: keytops3 150 centred less 6 on the point's
/// map pixel (blend 0x48) in the state's colour (0x80202020, 0x80001080, 0x80801000; state 3 or more the first),
/// "#%02d" of i at (+10, -2); a state other than 0 adds the bar FUN_101fc270 at (-13, +10): windowps 97 at (+2, +1)
/// scaled (min(byte x 0.6 x 0.01, 1), 0.72) in that colour (blend 8), then windowps 38 scaled (0.6, 0.8) in 0x80404040
/// (blend 0x48). `screen` maps (x, z) to the window: FUN_101f8b80 then FUN_101f11d0.
pub fn draw_overlay(gu: &mut GameUi, o: &Overlay, screen: impl Fn(f32, f32) -> (f32, f32)) {
    const COLOURS: [u32; 3] = [0x8020_2020, 0x8000_1080, 0x8080_1000];
    for (i, &[byte, state, x, z]) in o.records.iter().enumerate() {
        let (sx, sy) = screen(x as f32, z as f32);
        let colour = COLOURS.get(state as usize).copied().unwrap_or(COLOURS[0]);
        gu.sprite_scaled_blend("menu    keytops3", 150, sx - 6.0, sy - 6.0, 1.0, 1.0, colour, 0x48);
        text(gu, format!("#{i:02}").as_bytes(), sx + 10.0, sy - 2.0, WHITE);
        if state != 0 {
            let (bx, by) = (sx - 13.0, sy + 10.0);
            let f = (byte as f32 * 0.6 * 0.01).min(1.0);
            gu.sprite_scaled_blend(WINDOWPS, 97, bx + 2.0, by + 1.0, f, 0.72, colour, 8);
            gu.sprite_scaled_blend(WINDOWPS, 38, bx, by, 0.6, 0.8, 0x8040_4040, 0x48);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zone_in_request_is_kind_0() {
        let mut b = Battlefield::default();
        assert_eq!(b.request(Kind::Both), Some(vec![0, 0, 0, 0]));
        // A second kind 0 goes out as well (FUN_10098080 has no gate).
        assert_eq!(b.request(Kind::Both), Some(vec![0, 0, 0, 0]));
    }

    #[test]
    fn kinds_1_and_2_wait_on_the_sidebar_callback() {
        let mut b = Battlefield::default();
        b.request(Kind::Both);
        assert_eq!(b.request(Kind::Sidebar), None);
        assert_eq!(b.request(Kind::MapOverlay), None);
        b.zone_load();
        assert_eq!(b.request(Kind::MapOverlay), Some(vec![2, 0, 0, 0]));
        // Kind 2 set the overlay's callback only, so the sidebar's gate is still open.
        assert_eq!(b.request(Kind::Sidebar), Some(vec![1, 0, 0, 0]));
        assert_eq!(b.request(Kind::MapOverlay), None);
    }

    fn reply(kind: u8, body: &[u8]) -> Vec<u8> {
        let mut p = vec![0x72, 0x20, 0, 0, kind, 0, 0, 0];
        p.extend_from_slice(body);
        p.resize(0x3c, 0);
        p
    }

    #[test]
    fn sidebar_reply() {
        let mut b = Battlefield::default();
        b.request(Kind::Both);
        let a: u32 = 5 | 300 << 7 | 9000 << 17 | 1 << 31;
        let bw: u32 = 0x7f | 0x3ff << 7 | 15000 << 17;
        let c: u32 = 1 | 5 << 1 | 0xf0;
        let mut body = Vec::new();
        for v in [a, bw, c] {
            body.extend_from_slice(&v.to_le_bytes());
        }
        b.on_reply(&reply(0, &body));
        let s = b.sidebar.clone().unwrap();
        assert_eq!(s.records, [[0; 4], [0x7f, 0x3ff, 15000, 0], [5, 300, 9000, 1]]);
        assert_eq!((s.flag, s.mode), (1, 5));
        // One-shot: a second reply is dropped.
        b.on_reply(&reply(0, &[0xff; 12]));
        assert_eq!(b.sidebar, Some(s));
        // The overlay's callback is still set.
        assert!(b.overlay.is_none());
    }

    #[test]
    fn overlay_reply() {
        let mut b = Battlefield::default();
        b.request(Kind::Both);
        let mut body: Vec<u8> = (1..=12).collect();
        // Record 0: 2 | x = -3 | y = 100; record 9: 1 | x = 16383 | y = -1.
        let w0: u32 = 2 | ((-3i32 as u32) & 0x7fff) << 2 | (100u32 << 17);
        let w9: u32 = 1 | 0x3fff << 2 | ((-1i32 as u32) << 17);
        for i in 0..10 {
            let w = match i {
                0 => w0,
                9 => w9,
                _ => 3,
            };
            body.extend_from_slice(&w.to_le_bytes());
        }
        b.on_reply(&reply(1, &body));
        let o = b.overlay.clone().unwrap();
        assert_eq!(o.records[0], [1, 2, -3, 100]);
        assert_eq!(o.records[1], [2, 3, 0, 0]);
        assert_eq!(o.records[9], [10, 1, 16383, -1]);
        assert!(b.sidebar.is_none());
    }

    #[test]
    fn reply_without_a_request_is_dropped() {
        let mut b = Battlefield::default();
        b.on_reply(&reply(0, &[1; 12]));
        b.on_reply(&reply(1, &[1; 52]));
        assert!(b.sidebar.is_none() && b.overlay.is_none());
        b.request(Kind::Both);
        b.zone_load();
        b.on_reply(&reply(0, &[1; 12]));
        assert!(b.sidebar.is_none());
    }

    /// FUN_101f8870 / FUN_101f87d0 / FUN_101f87b0 over a reply, and their zeros while the block is not valid.
    #[test]
    fn sidebar_getters() {
        let mut b = Battlefield::default();
        assert_eq!((b.side_f0(1), b.side_pct(1), b.side_share(1, 2), b.side_mode()), (0, 0, 0, 0));
        b.request(Kind::Both);
        let a: u32 = 40 | 300 << 7 | 7500 << 17;
        let bw: u32 = 99 | 100 << 7 | 15000 << 17;
        let mut body = Vec::new();
        for v in [a, bw, 3 << 1] {
            body.extend_from_slice(&v.to_le_bytes());
        }
        b.on_reply(&reply(0, &body));
        // Record 1 is the word @0xc, record 2 the word @8.
        assert_eq!((b.side_f0(1), b.side_f0(2)), (99, 40));
        assert_eq!((b.side_pct(1), b.side_pct(2)), (100, 50));
        assert_eq!((b.side_share(1, 2), b.side_share(2, 1)), (25, 75));
        assert_eq!(b.side_mode(), 3);
    }

    #[test]
    fn share_of_nothing() {
        let mut b = Battlefield::default();
        b.request(Kind::Both);
        b.on_reply(&reply(0, &[0; 12]));
        assert_eq!(b.side_share(1, 2), 0);
    }

    /// One step at the first draw; a frame without the sidebar drops the time since, so the next draw is one step too.
    #[test]
    fn ease_runs_per_draw() {
        let mut b = Battlefield::default();
        let t0 = Instant::now();
        let at = |ms: u64| t0 + std::time::Duration::from_millis(ms);
        let to80 = [(1, 80), (4, 0), (6, 0), (2, 0), (5, 0)];
        b.frame();
        b.ease(to80, at(0));
        assert_eq!(b.gauges[1], 12, "the first draw eases once");
        b.frame();
        b.ease(to80, at(34));
        assert_eq!(b.gauges[1], 22);
        // Not drawn for two seconds: no catch-up when it comes back.
        b.frame();
        b.frame();
        b.ease(to80, at(2034));
        assert_eq!(b.gauges[1], 30);
    }

    #[test]
    fn gauge_ease_step() {
        assert_eq!(ease(0, 100), 15);
        assert_eq!(ease(15, 100), 27);
        assert_eq!(ease(95, 100), 100, "a step that truncates to 0 lands on the value");
        assert_eq!(ease(100, 0), 85);
        assert_eq!(ease(50, 50), 50);
    }

    /// FUN_10220a20 / FUN_101fc0c0: zone 0x11e only, Belligerency's team bytes only, titlewin up and the player's
    /// record set for the map.
    #[test]
    fn reader_gates() {
        assert!(sidebar_shown(8, 0x11e) && sidebar_shown(9, 0x11e));
        assert!(!sidebar_shown(8, 0x11d) && !sidebar_shown(2, 0x11e) && !sidebar_shown(0x28, 0x11e));
        assert!(overlay_shown(0x11e, true, true));
        assert!(!overlay_shown(0x11e, false, true) && !overlay_shown(0x11e, true, false));
        assert!(!overlay_shown(0x11d, true, true));
    }

    /// FUN_101fc100: a map of zone 0x11e asks with kind 2 unless the zone-in's request still waits.
    #[test]
    fn map_kind_2() {
        let mut b = Battlefield::default();
        assert_eq!(b.map_request(0x11e, true, true), Some(vec![2, 0, 0, 0]));
        assert_eq!(b.map_request(0x11d, true, true), None);
        b.request(Kind::Both);
        assert_eq!(b.map_request(0x11e, true, true), None);
        b.zone_load();
        assert_eq!(b.map_request(0x11e, true, true), Some(vec![2, 0, 0, 0]));
        assert!(b.overlay.is_none());
    }
}
