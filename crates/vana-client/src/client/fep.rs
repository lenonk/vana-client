//! Auto-translate input in the chat line (FUN_101b5d10, English's direct path): Tab completes the word at the end
//! of the line from the dictionary into "menu    fep" (FUN_1018f550); a phrase goes in as one unit (`FD id FD`,
//! four private-use chars here, font::PHRASE_BASE). Spec: ~/ffxi/lab/research/autotranslate_spec.md section 3.

use super::World;
use super::game_ui::GameUi;
use crate::audio::Ui as Se;
use crate::dat::font::PHRASE_BASE;

/// Raw and expanded limits of the chat line (E+0xa54 = 0x78).
pub(super) const MAX: usize = 120;
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

/// The line's unit boundaries (FUN_1018fb00 / FUN_1018fa80): a phrase block is one unit, any other char one.
fn bounds(chars: &[char]) -> Vec<usize> {
    let mut out = vec![0];
    let mut i = 0;
    while i < chars.len() {
        i += if is_phrase(chars[i]) && i + 4 <= chars.len() { 4 } else { 1 };
        out.push(i);
    }
    out
}

/// The caret (E+0xa58) as a char index: None is the end of the line; never inside a block.
pub(super) fn caret(w: &World) -> usize {
    let chars: Vec<char> = w.chat_input.chars().collect();
    let at = w.chat_caret.map_or(chars.len(), |c| c.min(chars.len()));
    bounds(&chars).into_iter().filter(|&b| b <= at).max().unwrap_or(0)
}

fn set_caret(w: &mut World, at: usize) {
    w.chat_caret = (at < w.chat_input.chars().count()).then_some(at);
}

/// The selected chars (an addition: retail's line has no selection): between the anchor Shift left and the caret.
pub(super) fn selection(w: &World) -> Option<(usize, usize)> {
    let a = w.chat_anchor?.min(w.chat_input.chars().count());
    let c = caret(w);
    (a != c).then(|| (a.min(c), a.max(c)))
}

/// The selection as plain text (phrases written out), for the clipboard.
pub(super) fn selected_text(w: &World) -> Option<String> {
    let (s, e) = selection(w)?;
    Some(plain(w, &w.chat_input.chars().skip(s).take(e - s).collect::<String>()))
}

/// Removes the selection; false when there is none.
pub(super) fn delete_selection(w: &mut World) -> bool {
    let Some((s, e)) = selection(w) else { return false };
    w.chat_input = w.chat_input.chars().take(s).chain(w.chat_input.chars().skip(e)).collect();
    w.chat_anchor = None;
    set_caret(w, s);
    true
}

pub(super) fn select_all(w: &mut World) {
    w.chat_anchor = Some(0);
    w.chat_caret = None;
}

/// Left / Right (FUN_101ac4e0 / FUN_101ac520): one unit; Home / End (VK 0x24 / 0x23): the start / the end. With
/// Shift the selection grows from where the caret was.
pub(super) fn caret_key(w: &mut World, key: egui::Key, shift: bool) {
    let chars: Vec<char> = w.chat_input.chars().collect();
    let (at, b) = (caret(w), bounds(&chars));
    w.chat_anchor = if shift { Some(w.chat_anchor.unwrap_or(at)) } else { None };
    let to = match key {
        egui::Key::ArrowLeft => b.into_iter().filter(|&x| x < at).max().unwrap_or(0),
        egui::Key::ArrowRight => b.into_iter().find(|&x| x > at).unwrap_or(chars.len()),
        egui::Key::Home => 0,
        _ => chars.len(),
    };
    set_caret(w, to);
}

fn spliced(line: &str, at: usize, text: &str) -> String {
    line.chars().take(at).chain(text.chars()).chain(line.chars().skip(at)).collect()
}

/// Typed or pasted text at the caret (FUN_101abc50): in whole only while the raw and the expanded line stay within
/// 120 bytes.
pub(super) fn insert(w: &mut World, text: &str) {
    // A selection is replaced, when the result fits.
    let (line, at) = match selection(w) {
        Some((s, e)) => (w.chat_input.chars().take(s).chain(w.chat_input.chars().skip(e)).collect(), s),
        None => (w.chat_input.clone(), caret(w)),
    };
    let next = spliced(&line, at, text);
    let (raw, plain) = lengths(w, &next);
    if raw <= MAX && plain <= MAX {
        w.chat_input = next;
        w.chat_anchor = None;
        set_caret(w, at + text.chars().count());
    }
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
pub(super) fn lengths(w: &World, line: &str) -> (usize, usize) {
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
        // These move the Japanese composition cursor (FUN_101b4fc0 / FUN_101b5020); nothing in English.
        egui::Key::ArrowLeft | egui::Key::ArrowRight | egui::Key::Home | egui::Key::End => {
            w.fep = Some(f);
            return true;
        }
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
            // The word being completed goes from the end of the line (FUN_101ad0d0), then the candidate in at the
            // caret (FUN_101b6c00).
            let keep = w.chat_input.chars().count().saturating_sub(f.word_len);
            let line: String = w.chat_input.chars().take(keep).collect();
            let at_end = caret(w) >= keep;
            let at = caret(w).min(keep);
            let put = |w: &mut World, line: String, n: usize| {
                w.chat_input = line;
                w.chat_caret = if at_end { None } else { Some(at + n) };
            };
            match c.id {
                None => put(w, spliced(&line, at, &c.text), c.text.chars().count()),
                Some(id) if id[3] == 0 && id[0] < 7 => {
                    // A category: nothing goes in; the list reopens on its phrases.
                    put(w, line, 0);
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
                    let next = spliced(&line, at, &block);
                    let (raw, plain) = lengths(w, &next);
                    if raw > MAX || plain > MAX {
                        w.sounds.push(Se::Error as u32);
                        return true;
                    }
                    put(w, next, 4);
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

/// Backspace / Delete with the list closed (FUN_101abef0 / FUN_101abf40(1 / 0)): the unit before / at the caret goes;
/// a phrase becomes its plain text minus its last / first letter.
pub(super) fn erase(w: &mut World, back: bool) {
    if delete_selection(w) {
        return;
    }
    let chars: Vec<char> = w.chat_input.chars().collect();
    let (at, b) = (caret(w), bounds(&chars));
    let unit = if back { b.windows(2).find(|u| u[1] == at) } else { b.windows(2).find(|u| u[0] == at) };
    let Some(&[s, e]) = unit else { return };
    let mut text = String::new();
    if e - s == 4 {
        text = plain(w, &chars[s..e].iter().collect::<String>());
        if back {
            text.pop();
        } else if !text.is_empty() {
            text.remove(0);
        }
    }
    let n = text.chars().count();
    w.chat_input = chars[..s].iter().collect::<String>() + &text + &chars[e..].iter().collect::<String>();
    if w.chat_caret.is_some() {
        set_caret(w, if back { s + n } else { s });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_and_splices() {
        let p = |b: u32| char::from_u32(PHRASE_BASE + b).unwrap();
        let line: Vec<char> = vec!['a', p(2), p(0x101), p(0x203), p(0x300), 'b'];
        assert_eq!(bounds(&line), vec![0, 1, 5, 6]);
        assert_eq!(spliced("abcdef", 3, "X"), "abcXdef");
        assert_eq!(spliced("ab", 2, "c"), "abc");
    }
}
