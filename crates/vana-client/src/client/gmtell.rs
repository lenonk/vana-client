//! GM prompts: s2c 0x017 kind 0x0c (FUN_100a40a0) queues a question with its answers; the frame step (FUN_100a3d00)
//! opens "menu    gmtell" on the first and answers it with a tell to its sender (group 9 0x84..0x86, FUN_100e3570).
//! LSB's custom menus (!menu, !quest, !mission, !afkcheck) come this way from "_CUSTOM_MENU".

use super::World;
use crate::client::net::NetCommand;

pub const WINDOW: &str = "menu    gmtell";
/// The queue's slots (DAT_10485a78, 0x140 bytes each); an open prompt keeps its slot.
const SLOTS: usize = 4;
/// FUN_100a3f10 reads at most 8 answers (ids 1..8).
const ANSWERS: usize = 8;
/// Rows the window shows (children 1..3).
const ROWS: usize = 3;

#[derive(Default)]
pub struct GmTell {
    /// Waiting prompts: the sender and the text.
    queue: std::collections::VecDeque<(String, String)>,
    pub open: Option<Prompt>,
}

pub struct Prompt {
    from: String,
    pub question: String,
    pub options: Vec<String>,
    pub cursor: usize,
    pub scroll: usize,
}

pub fn open(w: &World) -> bool {
    w.gmtell.open.is_some()
}

/// FUN_100a40a0: refused in an event (0x82) or with every slot taken (0x83), else queued.
pub fn receive(w: &mut World, from: &str, text: &str) {
    if super::event::active(w) {
        reply(w, from, 0x82, &[]);
    } else if w.gmtell.queue.len() + open(w) as usize >= SLOTS {
        reply(w, from, 0x83, &[]);
    } else {
        w.gmtell.queue.push_back((from.to_string(), text.to_string()));
    }
}

/// FUN_100a3d00, each frame: an event cancels the prompts (0x84); else the first waiting one opens. Retail repeats
/// the open slot's 0x84 each frame while another waits; ours answers each prompt once.
pub fn step(w: &mut World) {
    if super::event::active(w) {
        if let Some(p) = w.gmtell.open.take() {
            reply(w, &p.from, 0x84, &[&p.question]);
        }
        while let Some((from, text)) = w.gmtell.queue.pop_front() {
            let question = words(&text).into_iter().next().unwrap_or_default();
            reply(w, &from, 0x84, &[&question]);
        }
        return;
    }
    if open(w) {
        return;
    }
    let Some((from, text)) = w.gmtell.queue.pop_front() else { return };
    // FUN_100a3f10: the first word the question, up to 8 more the answers; without both it closes unanswered.
    let mut it = words(&text).into_iter();
    let question = it.next().unwrap_or_default();
    let options: Vec<String> = it.take(ANSWERS).collect();
    if question.is_empty() || options.is_empty() {
        return;
    }
    w.gmtell.open = Some(Prompt { from, question, options, cursor: 0, scroll: 0 });
}

/// FUN_100a3fe0's words: a quoted run up to its closing quote, else up to a space or tab; the spaces and tabs after
/// it skipped.
fn words(text: &str) -> Vec<String> {
    let c: Vec<char> = text.chars().collect();
    let (mut out, mut i) = (Vec::new(), 0);
    while i < c.len() {
        let quoted = c[i] == '"';
        let start = i + quoted as usize;
        let mut j = start;
        while j < c.len() && !(if quoted { c[j] == '"' } else { c[j] == ' ' || c[j] == '\t' }) {
            j += 1;
        }
        out.push(c[start..j].iter().collect());
        j += 1;
        while j < c.len() && (c[j] == ' ' || c[j] == '\t') {
            j += 1;
        }
        i = j;
    }
    out
}

/// The answer tell: group 9 `id` with the player's name, then `args`, sent to `to` (no echo in the log).
fn reply(w: &World, to: &str, id: usize, args: &[&str]) {
    let Some(f) = w.battle.as_ref().and_then(|b| b.group9(id)) else { return };
    let mut all = vec![super::monstrosity::my_name(w)];
    all.extend_from_slice(args);
    let text = super::select::printf(&f, &all);
    let _ = w.net.commands.send(NetCommand::Tell { name: to.to_string(), text });
}

/// The answer (Enter: the cursor's, 0x86) or the cancel (Escape, 0x85); the window closes.
fn answer(w: &mut World, pick: Option<usize>) {
    let Some(p) = w.gmtell.open.take() else { return };
    match pick.and_then(|k| p.options.get(k)) {
        Some(o) => reply(w, &p.from, 0x86, &[&p.question, o]),
        None => reply(w, &p.from, 0x85, &[&p.question]),
    }
}

/// FUN_1014e550: Down / Up one answer (no wrap, no pages), the view following inside 3 rows, SE 0001 when it moved;
/// Enter picks (SE 0002, the generic handler's), Escape cancels (SE 0003).
pub fn key(w: &mut World, key: egui::Key) {
    let Some(p) = w.gmtell.open.as_mut() else { return };
    let was = p.cursor;
    match key {
        egui::Key::ArrowDown => move_to(p, was + 1),
        egui::Key::ArrowUp => move_to(p, was.saturating_sub(1)),
        egui::Key::Enter => {
            w.sounds.push(crate::audio::Ui::Select as u32);
            answer(w, Some(was));
            return;
        }
        egui::Key::Escape => {
            w.sounds.push(crate::audio::Ui::Cancel as u32);
            answer(w, None);
            return;
        }
        _ => {}
    }
    if w.gmtell.open.as_ref().is_some_and(|p| p.cursor != was) {
        w.sounds.push(crate::audio::Ui::Cursor as u32);
    }
}

fn move_to(p: &mut Prompt, k: usize) {
    p.cursor = k.min(p.options.len() - 1);
    p.scroll = p.scroll.min(p.cursor).max((p.cursor + 1).saturating_sub(ROWS));
}

/// The mouse (ours, as the query's): hovering row `k` moves the cursor there, a click picks it.
pub fn pointer(w: &mut World, k: usize, click: bool) {
    let Some(p) = w.gmtell.open.as_mut().filter(|p| k < p.options.len()) else { return };
    if p.cursor != k {
        move_to(p, k);
        w.sounds.push(crate::audio::Ui::Cursor as u32);
    }
    if click {
        key(w, egui::Key::Enter);
    }
}

#[cfg(test)]
mod tests {
    use super::words;

    #[test]
    fn lsb_menu_words() {
        assert_eq!(words("\"Pick one\"\"Yes\"\"No thanks\""), ["Pick one", "Yes", "No thanks"]);
        assert_eq!(words("Title  a\tb"), ["Title", "a", "b"]);
        assert_eq!(words("\"open"), ["open"]);
        assert_eq!(words(" x"), ["", "x"]);
    }
}
