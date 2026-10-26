//! "menu    comment" (FUN_101dd2b0 / FUN_101ddcb0 / FUN_101dde60 / FUN_101dd330): three lines of 40 bytes. Mode 2 the
//! own search comment (arealist_spec section 3: searchma's Edit Comment through comgenre), sent on close as c2s 0x0E0
//! when it or the type changed; mode 0 our bazaar message (social_rows_spec B5: Set Bazaar's Edit Comment), as c2s
//! 0x0DE.

use super::World;
use super::game_ui::GameUi;
use super::net::NetCommand;
use crate::audio::Ui;

pub const EDITOR: &str = "menu    comment";
/// The text box's bytes per line (FUN_101c5be0(9, 40, ..)).
const LINE: usize = 40;
const WHITE: u32 = 0x8080_8080;

/// Block 2 (the lines), block 5 (what was sent last), +0x17 the type and +0x18 the type sent last (0xff at login).
#[derive(Clone, Debug)]
pub struct Store {
    pub lines: [String; 3],
    sent: [String; 3],
    pub kind: u8,
    sent_kind: u8,
    /// +0x19: our own message came (s2c 0x0CA with MyFlag; the bazaar store only).
    pub own_in: bool,
}

impl Default for Store {
    fn default() -> Self {
        Store { lines: Default::default(), sent: Default::default(), kind: 0, sent_kind: 0xff, own_in: false }
    }
}

/// The open editor: +0x15, the line whose box is open; +0x16 the mode (0 bazaar, 2 search comment).
#[derive(Clone, Copy, Debug)]
pub struct Editor {
    pub line: usize,
    pub mode: u8,
}

/// FUN_101dd2b0 (mode 2): the type, line 0 edited at once; stacked on the chat log (window flag 0x1000).
pub fn open(w: &mut World, kind: u8) {
    w.seacom.kind = kind;
    w.seacom_edit = Some(Editor { line: 0, mode: 2 });
}

/// FUN_101dd2b0 (mode 0, type 0x73): our bazaar message.
pub fn open_bazaar(w: &mut World) {
    w.seacom_edit = Some(Editor { line: 0, mode: 0 });
}

fn store(w: &mut World, mode: u8) -> &mut Store {
    if mode == 0 { &mut w.bazaar_msg } else { &mut w.seacom }
}

/// FUN_101ddae0(msg, 1): our own s2c 0x0CA message (MyFlag) cut into three lines of 40, trailing spaces trimmed, into
/// the bazaar store's lines and its last-sent copy.
pub fn own_message(w: &mut World, raw: &[u8]) {
    let lines: [String; 3] = std::array::from_fn(|k| {
        let line: Vec<u8> = raw.iter().skip(LINE * k).take(LINE).copied().take_while(|&c| c != 0).collect();
        crate::dat::font::decode_sjis(&line).trim_end().to_string()
    });
    let s = &mut w.bazaar_msg;
    (s.lines, s.sent, s.own_in) = (lines.clone(), lines, true);
}

/// The box is open the whole time the editor is.
/// The editor has no children: the mouse's only message is the right click, message 6 (FUN_1015def0): Escape.
pub fn right_click(w: &mut World) {
    if w.seacom_edit.is_some() {
        w.sounds.push(Ui::Cancel as u32);
        close(w);
    }
}

pub fn typing(w: &World) -> bool {
    w.seacom_edit.is_some()
}

/// FUN_101dde60 with the box: text (40 bytes), Backspace; Down / Enter commit and go to the next line (the last
/// reopens itself), Up the previous; Escape commits and closes (it keeps the text and sends).
pub fn text_events(w: &mut World, events: &[egui::Event]) {
    let Some(mut e) = w.seacom_edit else { return };
    for ev in events {
        let line = &mut store(w, e.mode).lines[e.line];
        match ev {
            egui::Event::Text(t) | egui::Event::Paste(t) => {
                for c in t.chars().filter(|c| (' '..='~').contains(c)) {
                    if line.len() < LINE {
                        line.push(c);
                    }
                }
            }
            egui::Event::Key { key: egui::Key::Backspace, pressed: true, .. } => {
                line.pop();
            }
            egui::Event::Key { key: egui::Key::ArrowDown | egui::Key::Enter, pressed: true, .. } => e.line = (e.line + 1).min(2),
            egui::Event::Key { key: egui::Key::ArrowUp, pressed: true, .. } => e.line = e.line.saturating_sub(1),
            egui::Event::Key { key: egui::Key::Escape, pressed: true, .. } => {
                w.sounds.push(Ui::Cancel as u32);
                close(w);
                return;
            }
            _ => {}
        }
    }
    w.seacom_edit = Some(e);
}

/// The lines padded with spaces to 40 each (123 bytes, FUN_101dd5a0).
fn padded(lines: &[String; 3]) -> Vec<u8> {
    let mut t = vec![b' '; 123];
    for (k, l) in lines.iter().enumerate() {
        let b = crate::dat::font::encode_sjis(l);
        let n = b.len().min(LINE);
        t[LINE * k..LINE * k + n].copy_from_slice(&b[..n]);
    }
    t
}

/// FUN_101dd330. Mode 0: when the lines changed, c2s 0x0DE {the 123 bytes, NUL} (FUN_100e55b0, 0x80 bytes). Mode 2:
/// when the lines or the type changed, all blank sends "" (block 5 line 0 cleared), else the padded lines; then c2s
/// 0x0E0 (FUN_100e5980).
fn close(w: &mut World) {
    let Some(e) = w.seacom_edit.take() else { return };
    if e.mode == 0 {
        let s = &mut w.bazaar_msg;
        if s.lines == s.sent {
            return;
        }
        s.sent = s.lines.clone();
        let mut b = vec![0u8; 0x7c];
        b[..123].copy_from_slice(&padded(&s.lines));
        let _ = w.net.commands.send(NetCommand::Packet { id: 0x0DE, body: b });
        return;
    }
    commit(w);
}

/// The search comment's send (FUN_101dd330 mode 2 / FUN_100e5980): c2s 0x0E0 when the lines or the type changed.
fn commit(w: &mut World) {
    let s = &mut w.seacom;
    if s.lines == s.sent && s.kind == s.sent_kind {
        return;
    }
    let text = if s.lines.iter().all(|l| l.trim_matches(' ').is_empty()) {
        s.sent[0].clear();
        Vec::new()
    } else {
        s.sent = s.lines.clone();
        padded(&s.lines)
    };
    s.sent_kind = s.kind;
    let kind = s.kind;
    send(w, kind, &text);
}

/// /seacom N ["text"] (FUN_1007efb0 -> FUN_101ddb30): line N (0..2) set to the text or cleared; nothing while the
/// editor is open; nothing sent.
pub fn set_line(w: &mut World, n: usize, text: &str) {
    if w.seacom_edit.is_some() || n > 2 {
        return;
    }
    w.seacom.lines[n] = text.chars().filter(|c| (' '..='~').contains(c)).take(LINE).collect();
}

/// /seacomup (FUN_1007f020): the type, then the send as the editor's close does.
pub fn upload(w: &mut World, kind: u8) {
    w.seacom.kind = kind;
    commit(w);
}

/// c2s 0x0E0 (0x98 bytes): text[128] @4, installTime @0x84, "WIN" @0x88, srvExCode @0x8c, cliExCode @0x90, msgType @0x94.
/// LSB reads only the text and the type *(inf: the time, platform byte and ex codes)*.
fn send(w: &World, kind: u8, text: &[u8]) {
    let mut b = vec![0u8; 0x94];
    b[..text.len().min(0x7f)].copy_from_slice(&text[..text.len().min(0x7f)]);
    b[0x80..0x84].copy_from_slice(&install_time().to_le_bytes());
    b[0x84..0x87].copy_from_slice(b"WIN");
    b[0x88..0x8c].copy_from_slice(&w.expansions.to_le_bytes());
    b[0x8c..0x90].copy_from_slice(&w.features.to_le_bytes());
    b[0x90..0x94].copy_from_slice(&(kind as u32).to_le_bytes());
    let _ = w.net.commands.send(NetCommand::Packet { id: 0x0E0, body: b });
}

/// FUN_100e3220: the u32 time() kept in "ffxiusr.sys" (8 bytes {time, -1}), made on first use.
fn install_time() -> u32 {
    let path = crate::config::data_dir().join("ffxiusr.sys");
    if let Ok(b) = std::fs::read(&path)
        && b.len() >= 4
    {
        return u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as u32);
    let _ = std::fs::write(&path, [now.to_le_bytes(), u32::MAX.to_le_bytes()].concat());
    now
}

/// Help slot 2: 0xe 0x32 while the editor is open; title slot 2 0xd 0x0f "Set Bazaar" in mode 0 (none in mode 2).
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    if let Some(e) = w.seacom_edit {
        bars.help[2] = gu.menu_msg(0x32, "");
        if e.mode == 0 {
            bars.title[2] = gu.title_msg(0x0f).unwrap_or_default();
        }
    }
}

/// FUN_101ddcb0: the lines at (x0 + 0x14, y0 + 4 + 16 k), 335 px, the edited one as the live box, and the genre dot
/// (FUN_1020b4b0) at (x0 + 8, y0 + 8) in mode 2; in mode 0 at x0 + 4, x1 - x0 - 8 wide, no dot.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    let Some(e) = w.seacom_edit else {
        gu.force_flags(EDITOR, None);
        return rects;
    };
    gu.force_flags(EDITOR, Some(0x1000));
    let Some(win) = gu.lobby_window_off(EDITOR, sw, sh, (0.0, 0.0), None, false, &[]) else { return rects };
    let [x0, y0, ww, _] = win.frame;
    let (s, dx, width) = if e.mode == 0 { (&w.bazaar_msg, 4.0, ww - 8.0) } else { (&w.seacom, 20.0, 335.0) };
    for (k, l) in s.lines.iter().enumerate() {
        let (x, y) = (x0 + dx, y0 + 4.0 + 16.0 * k as f32);
        if k == e.line {
            let caret = if (w.started.elapsed().as_secs_f32() * 60.0) as u32 % 30 < 20 { "|" } else { "" };
            gu.text(&format!("{l}{caret}"), x, y, 0, [0x80; 4]);
        } else {
            gu.text_fit(l, x, y, width, WHITE);
        }
    }
    let g = (w.seacom.kind >> 4 & 0xf) as usize;
    if e.mode == 2
        && let Some(&s) = super::search::GENRE.get(g.wrapping_sub(1))
    {
        gu.sprite("menu    windowps", s, x0 + 8.0, y0 + 8.0, WHITE);
    }
    rects.push(win.frame);
    rects
}
