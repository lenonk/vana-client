//! Auto-translate input in the chat line (FUN_101b5d10, English's direct path): Tab completes the word at the end
//! of the line from the dictionary into "menu    fep" (FUN_1018f550); a phrase goes in as one unit (`FD id FD`,
//! four private-use chars here, font::PHRASE_BASE). Spec: ~/ffxi/lab/research/autotranslate_spec.md section 3.

use super::World;
use super::game_ui::GameUi;
use crate::audio::Ui as Se;
use crate::dat::font::PHRASE_BASE;

/// Raw and expanded limits of the chat line (E+0xa54 = 0x78).
const MAX: usize = 120;
/// Rows a page (FUN_1018f550).
const ROWS: usize = 7;

#[derive(Clone, Debug)]
pub struct Cand {
    pub text: String,
    /// A dictionary block; None for plain text (the typed word, a name).
    pub id: Option<[u8; 4]>,
}

#[derive(Debug)]
pub struct Fep {
    pub cands: Vec<Cand>,
    pub sel: usize,
    /// The category the list shows (T+0xec2c), and the root word and selection to go back to (DAT_10662d58 / d54).
    filter: Option<u8>,
    root: (String, usize),
    /// The chars of the word being completed at the end of the line.
    word_len: usize,
}

fn is_phrase(c: char) -> bool {
    (PHRASE_BASE..PHRASE_BASE + 0x400).contains(&(c as u32))
}

/// The word being completed: the chars after the last space at the end of the line (FUN_101b7c20).
fn word(line: &str) -> &str {
    let tail = line.rsplit(' ').next().unwrap_or("");
    if tail.chars().any(is_phrase) { "" } else { tail }
}

/// FUN_101b6ee0: the dictionary's candidates, then the typed word (plain) and the names that start with it (tell
/// history, party: FUN_1018d7f0).
fn query(w: &World, word: &str, filter: Option<u8>) -> Vec<Cand> {
    let Some(d) = w.battle.as_ref().and_then(|b| b.phrases.as_ref()) else { return Vec::new() };
    let (hits, matched) = d.query(word, filter);
    let mut out: Vec<Cand> = hits.into_iter().map(|(text, id)| Cand { text, id: Some(id) }).collect();
    if filter.is_none() && !word.is_empty() {
        if matched {
            out.push(Cand { text: word.to_string(), id: None });
        }
        let lw = word.to_lowercase();
        let party: Vec<String> = w.party.members().map(|m| m.name.clone()).collect();
        for n in w.tells.iter().chain(party.iter()) {
            if n.to_lowercase().starts_with(&lw) && !out.iter().any(|c| c.id.is_none() && c.text == *n) {
                out.push(Cand { text: n.clone(), id: None });
            }
        }
    }
    out
}

/// Tab / Pause with the list closed: open it on the word at the end of the line.
pub(super) fn open(w: &mut World) {
    let wd = word(&w.chat_input).to_string();
    let cands = query(w, &wd, None);
    if cands.is_empty() {
        return;
    }
    w.sounds.push(Se::Select as u32);
    w.fep = Some(Fep { cands, sel: 0, filter: None, root: (wd.clone(), 0), word_len: wd.chars().count() });
}

/// The line's raw (Shift-JIS, a phrase 6 bytes) and expanded lengths (FUN_101abc50 / FUN_101acc30).
fn lengths(w: &World, line: &str) -> (usize, usize) {
    let raw = crate::dat::font::encode_sjis(line).len();
    let plain = plain(w, line);
    (raw, crate::dat::font::encode_sjis(&plain).len())
}

/// The line with its phrases as plain text (no brackets).
pub(super) fn plain(w: &World, line: &str) -> String {
    let names = w.names.clone();
    match w.battle.as_ref().and_then(|b| b.phrases.as_ref()) {
        Some(d) => d.expand_plain(line, names.as_deref().map(|n| n as &dyn crate::dat::names::NameSource)),
        None => line.chars().filter(|c| !is_phrase(*c)).collect(),
    }
}

/// A key while the list is open; true when it was taken.
pub(super) fn key(w: &mut World, key: egui::Key, shift: bool) -> bool {
    let Some(mut f) = w.fep.take() else { return false };
    let n = f.cands.len().max(1);
    match key {
        egui::Key::Tab | egui::Key::Space | egui::Key::ArrowDown if !(shift && key == egui::Key::Tab) => f.sel = (f.sel + 1) % n,
        egui::Key::Tab | egui::Key::ArrowUp => f.sel = (f.sel + n - 1) % n,
        egui::Key::PageDown => f.sel = (f.sel + ROWS).min(n - 1),
        egui::Key::PageUp => f.sel = f.sel.saturating_sub(ROWS),
        egui::Key::Escape | egui::Key::Backspace | egui::Key::Delete => {
            // A category goes back to the root list, else the list closes.
            if f.filter.take().is_some() {
                let (wd, sel) = f.root.clone();
                f.cands = query(w, &wd, None);
                f.sel = sel.min(f.cands.len().saturating_sub(1));
            } else {
                w.sounds.push(Se::Cancel as u32);
                return true;
            }
        }
        egui::Key::Enter => {
            let Some(c) = f.cands.get(f.sel).cloned() else { return true };
            // The word being completed goes (FUN_101ad0d0), then the candidate in (FUN_101b6c00).
            let keep = w.chat_input.chars().count().saturating_sub(f.word_len);
            let mut line: String = w.chat_input.chars().take(keep).collect();
            match c.id {
                None => {
                    line.push_str(&c.text);
                    w.chat_input = line;
                }
                Some(id) if id[3] == 0 && id[0] < 7 => {
                    // A category: nothing goes in; the list reopens on its phrases.
                    w.chat_input = line;
                    if f.filter.is_none() {
                        f.root.1 = f.sel;
                    }
                    f.filter = Some(id[2]);
                    f.word_len = 0;
                    f.cands = query(w, "", f.filter);
                    f.sel = 0;
                    w.sounds.push(Se::Select as u32);
                    w.fep = Some(f);
                    return true;
                }
                Some(id) => {
                    let block: String = id.iter().enumerate().map(|(k, &b)| char::from_u32(PHRASE_BASE + 0x100 * k as u32 + b as u32).unwrap()).collect();
                    let next = format!("{line}{block}");
                    let (raw, plain) = lengths(w, &next);
                    if raw > MAX || plain > MAX {
                        w.sounds.push(Se::Error as u32);
                        return true;
                    }
                    w.chat_input = next;
                }
            }
            w.sounds.push(Se::Select as u32);
            return true;
        }
        _ => {
            w.fep = Some(f);
            return false;
        }
    }
    w.sounds.push(Se::Cursor as u32);
    w.fep = Some(f);
    true
}

/// Backspace with the list closed (FUN_101abef0 / FUN_101abf40): a phrase at the end becomes its plain text minus
/// its last letter; else the last char goes.
pub(super) fn backspace(w: &mut World) {
    let chars: Vec<char> = w.chat_input.chars().collect();
    if chars.len() >= 4 && chars[chars.len() - 4..].iter().all(|&c| is_phrase(c)) {
        let tail: String = chars[chars.len() - 4..].iter().collect();
        let mut text = plain(w, &tail);
        text.pop();
        w.chat_input = chars[..chars.len() - 4].iter().collect::<String>() + &text;
    } else {
        w.chat_input.pop();
    }
}

/// FUN_101ad4b0: a submitted line's first word has its phrase expanded (so 【Text Commands】 phrases work); a chat
/// command (table 0x1037ab98) keeps the rest's blocks, any other command gets plain text; a line without a command
/// (said in the chat mode) keeps its blocks.
pub(super) fn outgoing(w: &World, line: String) -> String {
    if !line.chars().any(is_phrase) {
        return line;
    }
    let lead = line.len() - line.trim_start().len();
    let (first, rest) = line[lead..].split_once(' ').map_or((&line[lead..], ""), |(a, b)| (a, b));
    let first = plain(w, first);
    if !first.starts_with('/') {
        return line;
    }
    const CHAT: [&str; 34] = [
        "/say", "/s", "/shout", "/sh", "/tell", "/t", "/party", "/p", "/linkshell", "/l", "/linkshell2", "/l2", "/unity", "/u", "/assistj", "/aj", "/assiste", "/ae", "/emote", "/em", "/message", "/me", "/yell", "/yl", "//worldmes", "//wm", "/linkshellmes", "/lsmes", "/linkshell2mes", "/ls2mes", "/nominate", "/propose", "/seacom", "/sc",
    ];
    if CHAT.contains(&first.to_lowercase().as_str()) {
        format!("{first} {rest}")
    } else {
        plain(w, &format!("{first} {rest}"))
    }
}

/// The line being typed as drawn (FUN_101b5470): plain runs and phrases (coloured, no brackets) as (text, phrase).
pub(super) fn pieces(w: &World, line: &str) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    let mut block = String::new();
    for ch in line.chars() {
        if is_phrase(ch) {
            block.push(ch);
            if block.chars().count() == 4 {
                out.push((plain(w, &block), true));
                block.clear();
            }
        } else {
            match out.last_mut() {
                Some((t, false)) => t.push(ch),
                _ => out.push((ch.to_string(), false)),
            }
        }
    }
    out
}

/// "menu    fep" (FUN_1018f550): above the chat line at the word's start (inf: where retail puts it); width the widest
/// row, 90..200 + 12; the header "%d/%d" at (+0x23, 0) after the ▲ at (+5, 0); seven rows 16 px apart; a row too wide
/// cut with "...".
pub(super) fn draw(gu: &mut GameUi, f: &Fep, x: f32, line_y: f32) -> [f32; 4] {
    let widest = f.cands.iter().map(|c| gu.text_width(&c.text)).fold(0.0, f32::max);
    let w = (widest.max(90.0) + 12.0).min(200.0);
    let h = 134.0;
    let y = line_y - h - 2.0;
    gu.frame(x, y, w, h);
    const C: [u8; 4] = [0x80; 4];
    gu.text("\u{25b2}", x + 5.0, y + 4.0, 0, C);
    gu.text(&format!("{}/{}", if f.cands.is_empty() { 0 } else { f.sel + 1 }, f.cands.len()), x + 35.0, y + 4.0, 0, C);
    let first = f.sel / ROWS * ROWS;
    for (k, c) in f.cands.iter().enumerate().skip(first).take(ROWS) {
        let mut t = c.text.clone();
        // The text from x + 6 (inf: retail cuts at the width - 15 from its own start).
        if gu.text_width(&t) > w - 12.0 {
            while !t.is_empty() && gu.text_width(&format!("{t}...")) > w - 12.0 {
                t.pop();
            }
            t = format!("{}...", t.trim_end());
        }
        let ry = y + 20.0 + 16.0 * (k - first) as f32;
        gu.text(&t, x + 6.0, ry, 0, C);
        if k == f.sel {
            gu.cursor(x + 3.0, ry);
        }
    }
    [x, y, w, h]
}
