//! Social row 6 MacroPalette (spec ~/ffxi/lab/research/social_rows_spec.md M1): "menu    mcrmenu" -> Edit Macro:
//! the book list "menu    mcrselec" with its options "menu    mcrselop", the edit palettes "menu    mcr1edit" /
//! "menu    mcr2edit" (mcr?edlo long) with "menu    mcrbedit", and the macro editor "menu    macro" with
//! "menu    mcrledit". No packets: everything is the USER files (client/macros.rs). Edit Equip. Set: client/esmenu.rs.
//! Not done: auto-translate in the fields, the palettes' flip zoom.

use super::World;
use super::game_ui::GameUi;
use crate::audio::Ui;

pub(super) const MENU: &str = "menu    mcrmenu";
pub(super) const SELEC: &str = "menu    mcrselec";
pub(super) const SELOP: &str = "menu    mcrselop";
pub(super) const BEDIT: &str = "menu    mcrbedit";
pub(super) const MACRO: &str = "menu    macro";
pub(super) const LEDIT: &str = "menu    mcrledit";
const WHITE: u32 = 0x8080_8080;
/// SE 0013 (FUN_10037e90): the focus between a palette and its options window.
const SE_FOCUS: u32 = 13;

pub struct MacroEdit {
    menu_sel: u16,
    book: Option<Book>,
    /// Edit Equip. Set's windows.
    pub(super) sets: Option<super::esmenu::SetList>,
}

/// mcrselec: the cursor child (1..20), the rename field's text, mcrselop's cursor, the palettes once a book is
/// confirmed.
struct Book {
    sel: u16,
    rename: Option<Vec<u8>>,
    /// The rename field's caret (a byte index).
    caret: usize,
    opt: Option<u16>,
    edit: Option<Edit>,
}

/// E (DAT_106627a8): the focused palette (1 Ctrl, 2 Alt) and both columns; mcrbedit's cursor while it has the focus
/// and its macro; the frames Ctrl or Alt has been held (+0x1e); the editor.
struct Edit {
    focus: u8,
    cols: [u16; 2],
    bopt: Option<u16>,
    hold: f32,
    editor: Option<Editor>,
    /// The modern page: the set strip has the cursor.
    tabs: bool,
}

/// "menu    macro": macro `i` (0..19), the cursor child (1 Prev, 2 Next, 3 title, 4..9 lines), the working copy
/// (title, six lines), the field being edited, mcrledit's cursor while it has the focus.
struct Editor {
    i: usize,
    sel: u16,
    fields: [Vec<u8>; 7],
    editing: Option<usize>,
    /// The edited field's caret (a byte index; the fields are ASCII).
    caret: usize,
    lopt: Option<u16>,
}

/// FUN_101b9760's glyphs: '1'..'9' 0..8, '0' 9, 'a'..'z' 10..35, 'A'..'Z' 36..61.
fn alpha_index(c: u8) -> Option<usize> {
    Some(match c {
        b'1'..=b'9' => (c - b'1') as usize,
        b'0' => 9,
        b'a'..=b'z' => 10 + (c - b'a') as usize,
        b'A'..=b'Z' => 36 + (c - b'A') as usize,
        _ => return None,
    })
}

/// mcralpha text: advance = sprite width + 1, other bytes skipped without advancing, stopping before `max` px.
pub(super) fn alpha(gu: &mut GameUi, s: &[u8], x: f32, y: f32, max: f32) {
    let mut at = x;
    for &c in s {
        let Some(i) = alpha_index(c) else { continue };
        let adv = gu.sprite_width("menu    mcralpha", i) + 1.0;
        if at + adv > x + max {
            break;
        }
        gu.sprite("menu    mcralpha", i, at, y, WHITE);
        at += adv;
    }
}

/// A palette cell's title (FUN_101a3b20 / FUN_101a4a80): all letters and digits in mcralpha at (x + pad, y + 4) cut at
/// max(w, 38); else the game font at (x + pad, y + 2) cut at max(w, 48) - 14.
pub(super) fn cell_title(gu: &mut GameUi, title: &str, r: [f32; 4], pad: f32) {
    if title.is_empty() {
        return;
    }
    if title.chars().all(|c| c.is_ascii_alphanumeric()) {
        alpha(gu, title.as_bytes(), r[0] + pad, r[1] + 4.0, r[2].max(38.0) - pad);
    } else {
        let max = r[2].max(48.0) - 14.0;
        let mut t = title.to_string();
        while !t.is_empty() && gu.text_width(&t) > max {
            t.pop();
        }
        gu.text(&t, r[0] + pad, r[1] + 2.0, 0, WHITE.to_le_bytes());
    }
}

/// The play and edit palettes sit 0x82 further right while gaugewin or cnttime is up (FUN_101a3840: DAT_105781ec /
/// DAT_10578184, not the cast bar).
pub(super) fn palette_dx(w: &World) -> f32 {
    if w.gauge.is_some() || w.objective.counting() { 130.0 } else { 0.0 }
}

/// Social child 6 (FUN_1014e7b0 case 6): mcrmenu over the social page (which the walk suspends), SE 0002.
pub fn open(w: &mut World) {
    if w.macros.is_none() {
        return;
    }
    w.macroedit = Some(MacroEdit { menu_sel: 1, book: None, sets: None });
    w.sounds.push(Ui::Select as u32);
}

fn book_of(w: &World, sel: u16) -> usize {
    (sel as usize - 1) + 20 * w.mcr_page
}

/// The text fields take the keyboard: the book name (mode 4, 15 bytes) and the editor's (title 8, lines 60).
pub fn typing(w: &World) -> bool {
    super::esmenu::typing(w)
        || w.macroedit.as_ref().and_then(|m| m.book.as_ref()).is_some_and(|b| b.rename.is_some() || b.edit.as_ref().and_then(|e| e.editor.as_ref()).is_some_and(|d| d.editing.is_some() && d.lopt.is_none()))
}

/// The field's limits: 15 bytes for a book name, 8 for a title, 60 for a line (FUN_101c5be0 modes 4 / 8 / 9).
fn field_max(rename: bool, field: usize) -> usize {
    if rename { 15 } else if field == 0 { 8 } else { 60 }
}

/// Text into the open field (printable ASCII; the Shift-JIS and auto-translate input are not done), Backspace; the
/// field keys: the rename commits on Enter and Escape (FUN_101a6750(1)); the editor moves between its fields.
pub fn text_events(w: &mut World, events: &[egui::Event]) {
    use egui::Key as K;
    if super::esmenu::typing(w) {
        super::esmenu::text_events(w, events);
        return;
    }
    let page = w.mcr_page;
    for ev in events {
        let Some(b) = w.macroedit.as_mut().and_then(|m| m.book.as_mut()) else { return };
        if let Some(name) = b.rename.as_mut() {
            if line_edit(name, &mut b.caret, field_max(true, 0), ev) {
                continue;
            }
            match ev {
                egui::Event::Key { key: K::Enter | K::Escape, pressed: true, .. } => {
                    let (book, raw) = ((b.sel as usize - 1) + 20 * page, b.rename.take().unwrap_or_default());
                    if let Some(m) = w.macros.as_mut() {
                        m.rename_book(book, &raw);
                    }
                    w.sounds.push(Ui::Select as u32);
                }
                _ => {}
            }
            continue;
        }
        let Some(d) = b.edit.as_mut().and_then(|e| e.editor.as_mut()) else { return };
        let Some(f) = d.editing else { return };
        if line_edit(&mut d.fields[f], &mut d.caret, field_max(false, f), ev) {
            continue;
        }
        match ev {
            // Down: the next field (none past line 6); Up: the previous (none above the title). The caret goes to the
            // end of the new field.
            egui::Event::Key { key: K::ArrowDown, pressed: true, .. } if f < 6 => {
                (d.editing, d.sel, d.caret) = (Some(f + 1), f as u16 + 4, d.fields[f + 1].len());
            }
            egui::Event::Key { key: K::ArrowUp, pressed: true, .. } if f > 0 => {
                (d.editing, d.sel, d.caret) = (Some(f - 1), f as u16 + 2, d.fields[f - 1].len());
            }
            // LAB_101a5450: committed and not line 6 -> the next field; line 6 stops editing (SE 0003).
            egui::Event::Key { key: K::Enter, pressed: true, .. } if f < 6 => {
                (d.editing, d.sel, d.caret) = (Some(f + 1), f as u16 + 4, d.fields[f + 1].len());
            }
            egui::Event::Key { key: K::Enter | K::Escape, pressed: true, .. } => {
                d.editing = None;
                w.sounds.push(Ui::Cancel as u32);
            }
            _ => {}
        }
    }
}

/// A field's unit boundaries in its Shift-JIS bytes: a phrase block (FD .. FD), a 1e / 1f code and a double-byte char
/// are one unit each, as the chat line's (FUN_1018fb00 / FUN_1018fa80).
fn units(buf: &[u8]) -> Vec<usize> {
    let mut out = vec![0];
    let mut i = 0;
    while i < buf.len() {
        let b = buf[i];
        i += match () {
            _ if b == 0xfd && buf.get(i + 5) == Some(&0xfd) => 6,
            _ if (matches!(b, 0x1e | 0x1f) || crate::dat::font::is_lead(b)) && i + 1 < buf.len() => 2,
            _ => 1,
        };
        out.push(i);
    }
    out
}

/// The text keys of a field (as the chat line's FUN_101ac4e0 / FUN_101ac520 / FUN_101abef0 / FUN_101abf40): typed or
/// pasted printable ASCII goes in at the caret up to `max` bytes, Backspace / Delete take the unit before / at it,
/// Left / Right move it one unit, Home / End to the ends. True when the event was one of these.
pub(super) fn line_edit(buf: &mut Vec<u8>, caret: &mut usize, max: usize, ev: &egui::Event) -> bool {
    use egui::Key as K;
    let b = units(buf);
    *caret = b.iter().copied().filter(|&x| x <= *caret).max().unwrap_or(0);
    match ev {
        egui::Event::Text(t) | egui::Event::Paste(t) => {
            for c in t.bytes().filter(|c| (b' '..=b'~').contains(c)) {
                if buf.len() < max {
                    buf.insert(*caret, c);
                    *caret += 1;
                }
            }
        }
        egui::Event::Key { key, pressed: true, .. } => {
            let prev = b.iter().copied().filter(|&x| x < *caret).max();
            let next = b.iter().copied().find(|&x| x > *caret);
            match key {
                K::Backspace => {
                    if let Some(s) = prev {
                        buf.drain(s..*caret);
                        *caret = s;
                    }
                }
                K::Delete => {
                    if let Some(e) = next {
                        buf.drain(*caret..e);
                    }
                }
                K::ArrowLeft => *caret = prev.unwrap_or(0),
                K::ArrowRight => *caret = next.unwrap_or(buf.len()),
                K::Home => *caret = 0,
                K::End => *caret = buf.len(),
                _ => return false,
            }
        }
        _ => return false,
    }
    true
}

/// A field as drawn: its Shift-JIS decoded, its phrases written out.
pub(super) fn shown(w: &World, buf: &[u8]) -> String {
    super::fep::plain(w, &crate::dat::font::decode_sjis(buf))
}

/// Per frame: Ctrl or Alt held (key function 0xa8) counts up in mcrbedit (+0x1e); 31 or more is the set mode
/// *(inf: frames at 30 fps)*.
pub fn update(w: &mut World, ticks: f32, held: bool) {
    if let Some(e) = w.macroedit.as_mut().and_then(|m| m.book.as_mut()).and_then(|b| b.edit.as_mut()) {
        e.hold = if held { e.hold + ticks / 2.0 } else { 0.0 };
    }
}

fn set_mode(e: &Edit) -> bool {
    e.hold >= 31.0
}

/// The macro a palette column means: Ctrl 0..9, Alt 10..19.
fn macro_of(e: &Edit) -> usize {
    e.cols[e.focus as usize - 1] as usize + if e.focus == 2 { 10 } else { 0 }
}

pub fn key(w: &mut World, gu: &GameUi, key: egui::Key) {
    use egui::Key as K;
    let Some(me) = w.macroedit.as_ref() else { return };
    if me.sets.is_some() {
        super::esmenu::key(w, gu, key);
        return;
    }
    let menu_sel = me.menu_sel;
    let Some(b) = me.book.as_ref() else {
        // mcrmenu: links wrap; Enter 1 the book list, 2 the equipment sets; Escape closes (socialme resumes).
        match key {
            K::ArrowUp | K::ArrowDown => {
                if let Some(m) = w.macroedit.as_mut() {
                    m.menu_sel = 3 - menu_sel;
                }
                w.sounds.push(Ui::Cursor as u32);
            }
            K::Escape => {
                w.sounds.push(Ui::Cancel as u32);
                w.macroedit = None;
            }
            // FUN_101a6490: Edit Equip. Set is off in Monstrosity.
            K::Enter if menu_sel == 2 && super::monstrosity::active(w) => w.sounds.push(Ui::Error as u32),
            K::Enter => {
                w.sounds.push(Ui::Select as u32);
                if menu_sel == 1 {
                    if let Some(m) = w.macroedit.as_mut() {
                        m.book = Some(Book { sel: 1, rename: None, caret: 0, opt: None, edit: None });
                    }
                } else {
                    super::esmenu::open(w);
                }
            }
            _ => {}
        }
        return;
    };
    if b.rename.is_some() {
        return;
    }
    let (sel, opt) = (b.sel, b.opt);
    if let Some(e) = b.edit.as_ref() {
        if e.editor.is_some() {
            editor_key(w, gu, key);
        } else {
            palette_key(w, gu, key);
        }
        return;
    }
    let book = book_of(w, sel);
    if let Some(o) = opt {
        // mcrselop: links wrap 1..5; every Enter closes it afterwards.
        let pasted = w.macros.as_ref().is_some_and(|m| m.book_copied());
        let b = w.macroedit.as_mut().and_then(|m| m.book.as_mut()).unwrap();
        match key {
            K::ArrowUp | K::ArrowDown => {
                b.opt = Some(if key == K::ArrowDown { o % 5 + 1 } else if o == 1 { 5 } else { o - 1 });
                w.sounds.push(Ui::Cursor as u32);
            }
            K::Escape => {
                b.opt = None;
                w.sounds.push(Ui::Cancel as u32);
            }
            K::Enter if o == 4 && !pasted => w.sounds.push(Ui::Error as u32),
            K::Enter => {
                b.opt = None;
                w.sounds.push(Ui::Select as u32);
                match o {
                    // FUN_101a2920: the book becomes the active one at the same set; the editor opens.
                    1 => {
                        if let Some(m) = w.macros.as_mut() {
                            let set = m.set;
                            m.switch(book, set);
                        }
                        let b = w.macroedit.as_mut().and_then(|m| m.book.as_mut()).unwrap();
                        b.edit = Some(Edit { focus: 1, cols: [0, 0], bopt: None, hold: 0.0, editor: None, tabs: false });
                    }
                    2 => {
                        let name = w.macros.as_ref().map_or(Vec::new(), |m| m.raw_book_title(book).to_vec());
                        let b = w.macroedit.as_mut().and_then(|m| m.book.as_mut()).unwrap();
                        b.caret = name.len();
                        b.rename = Some(name);
                    }
                    3 => {
                        if let Some(m) = w.macros.as_mut() {
                            m.copy_book(book);
                        }
                    }
                    4 => w.confirm = Some((super::Confirm::Macro(Ask::PasteBook(book as u8)), false)),
                    _ => w.confirm = Some((super::Confirm::Macro(Ask::DeleteBook(book as u8)), false)),
                }
            }
            _ => {}
        }
        return;
    }
    // mcrselec: Up / Down along the links (wrap), Left / Right flip the page, Enter mcrselop, Escape writes the ttl
    // files and closes.
    match key {
        // Ours: the modern layer lists both pages as one column; Up / Down run on through it.
        K::ArrowUp | K::ArrowDown if w.modern => {
            let at = w.mcr_page * 20 + sel as usize - 1;
            let to = if key == K::ArrowDown { (at + 1) % 40 } else { (at + 39) % 40 };
            w.mcr_page = to / 20;
            w.macroedit.as_mut().and_then(|m| m.book.as_mut()).unwrap().sel = (to % 20 + 1) as u16;
            w.sounds.push(Ui::Cursor as u32);
        }
        K::ArrowLeft | K::ArrowRight if w.modern => {}
        K::ArrowUp | K::ArrowDown => {
            let k = if key == K::ArrowUp { 2 } else { 3 };
            let to = gu.menu_links(SELEC, sel).map_or(sel, |l| l[k] as u16);
            if (1..=20).contains(&to) && to != sel {
                w.macroedit.as_mut().and_then(|m| m.book.as_mut()).unwrap().sel = to;
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        K::ArrowLeft | K::ArrowRight => {
            w.mcr_page ^= 1;
            w.sounds.push(Ui::Cursor as u32);
        }
        K::Enter => {
            w.macroedit.as_mut().and_then(|m| m.book.as_mut()).unwrap().opt = Some(1);
            w.sounds.push(Ui::Select as u32);
        }
        K::Escape => {
            if let Some(m) = w.macros.as_ref() {
                m.save_titles();
            }
            w.macroedit.as_mut().unwrap().book = None;
            w.sounds.push(Ui::Cancel as u32);
        }
        _ => {}
    }
}

/// Num+ (key function 0x11, event 8): a palette to mcrbedit and back (FUN_101a4950 / FUN_101a6180), the editor on a
/// line to mcrledit and back (FUN_101a5690 / FUN_101a6350); SE 0013.
pub fn num_plus(w: &mut World) {
    if w.macroedit.as_ref().and_then(|m| m.sets.as_ref()).is_some_and(|s| s.ed.is_some()) {
        super::esedit::num_plus(w);
        return;
    }
    let Some(e) = edit_mut(w) else { return };
    match e.editor.as_mut() {
        Some(d) if d.editing.is_some() => return,
        Some(d) if d.lopt.is_some() => d.lopt = None,
        Some(d) if (4..=9).contains(&d.sel) => d.lopt = Some(1),
        Some(_) => return,
        None if e.bopt.is_some() => e.bopt = None,
        None => e.bopt = Some(1),
    }
    w.sounds.push(SE_FOCUS);
}

/// The prompts of this chain (comyn, cursor on No).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    PasteBook(u8),
    DeleteBook(u8),
    PasteSet,
    DeleteSet,
    /// mcresmn's Paste, Delete and Overwrite on a row of the page.
    EsPaste(u8),
    EsDelete(u8),
    EsOverwrite(u8),
}

/// The question: group 9 0xef / 0xf0 (book), 0xed / 0xee (set), 0x115..0x117 (equipment set).
pub fn ask_text(w: &World, a: Ask) -> String {
    let id = match a {
        Ask::PasteBook(_) => 0xef,
        Ask::DeleteBook(_) => 0xf0,
        Ask::PasteSet => 0xed,
        Ask::DeleteSet => 0xee,
        Ask::EsPaste(_) => 0x115,
        Ask::EsDelete(_) => 0x116,
        Ask::EsOverwrite(_) => 0x117,
    };
    w.battle.as_ref().and_then(|b| b.group9(id)).unwrap_or_default()
}

/// Yes (LAB_101a6ad0 / LAB_101a6af0, FUN_101a2b60 / FUN_101a2b40).
pub fn answer(w: &mut World, a: Ask) {
    if matches!(a, Ask::EsPaste(_) | Ask::EsDelete(_) | Ask::EsOverwrite(_)) {
        super::esmenu::answer(w, a);
        return;
    }
    let Some(m) = w.macros.as_mut() else { return };
    match a {
        Ask::PasteBook(b) => m.paste_book(b as usize),
        Ask::DeleteBook(b) => m.delete_book(b as usize),
        Ask::PasteSet => m.paste_set(),
        Ask::DeleteSet => m.clear_set(),
        _ => {}
    }
}

fn edit_mut(w: &mut World) -> Option<&mut Edit> {
    w.macroedit.as_mut()?.book.as_mut()?.edit.as_mut()
}

/// The palettes (FUN_101a4950 / FUN_101a5010) and mcrbedit (FUN_101a6180).
fn palette_key(w: &mut World, gu: &GameUi, key: egui::Key) {
    use egui::Key as K;
    let Some(e) = edit_mut(w) else { return };
    if let Some(o) = e.bopt {
        // mcrbedit: links wrap 1..3; Enter acts on the palette's macro, then the focus goes back; Escape / Num+ back.
        let (i, set) = (macro_of(e), set_mode(e));
        if key == K::Escape {
            e.bopt = None;
            w.sounds.push(SE_FOCUS);
            return;
        }
        match key {
            K::ArrowUp | K::ArrowDown => {
                e.bopt = Some(if key == K::ArrowDown { o % 3 + 1 } else if o == 1 { 3 } else { o - 1 });
                w.sounds.push(Ui::Cursor as u32);
            }
            K::Enter => {
                let can_paste = w.macros.as_ref().is_some_and(|m| if set { m.set_copied } else { m.macro_copied() });
                if o == 2 && !can_paste {
                    w.sounds.push(Ui::Error as u32);
                    return;
                }
                w.sounds.push(Ui::Select as u32);
                if let Some(e) = edit_mut(w) {
                    e.bopt = None;
                }
                let Some(m) = w.macros.as_mut() else { return };
                match (set, o) {
                    (false, 1) => m.copy_macro(i),
                    (false, 2) => m.paste_macro(i),
                    (false, _) => m.clear_macro(i),
                    (true, 1) => m.copy_set(),
                    (true, 2) => w.confirm = Some((super::Confirm::Macro(Ask::PasteSet), false)),
                    (true, _) => w.confirm = Some((super::Confirm::Macro(Ask::DeleteSet), false)),
                }
            }
            _ => {}
        }
        return;
    }
    let p = e.focus as usize - 1;
    let col = e.cols[p];
    let flip = |w: &mut World, step: Option<bool>| {
        if let Some(next) = step
            && let Some(m) = w.macros.as_mut()
        {
            let (book, set) = (m.book, m.set);
            m.switch(book, if next { (set + 1) % 10 } else { (set + 9) % 10 });
        }
        if let Some(e) = edit_mut(w) {
            e.cols = [col, col];
            e.focus = 3 - e.focus;
        }
        w.sounds.push(Ui::Cursor as u32);
    };
    match (e.focus, key) {
        (_, K::ArrowLeft | K::ArrowRight) => {
            let name = if e.focus == 1 { "menu    mcr1edit" } else { "menu    mcr2edit" };
            let k = if key == K::ArrowLeft { 5 } else { 4 };
            let to = gu.menu_links(name, col + 1).map_or(col + 1, |l| l[k] as u16);
            if (1..=10).contains(&to) {
                e.cols[p] = to - 1;
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        // Ctrl Down / Alt Up: the other palette at this column; Ctrl Up / Alt Down: the set before / after (FUN_101a2a70 /
        // FUN_101a2a50 through FUN_101a2890: the old set file written, the new one read), then the other palette.
        (1, K::ArrowDown) | (2, K::ArrowUp) => flip(w, None),
        (1, K::ArrowUp) => flip(w, Some(false)),
        (2, K::ArrowDown) => flip(w, Some(true)),
        // FUN_101a45c0: the editor on this macro; the palettes and mcrbedit close.
        (_, K::Enter) => {
            let i = macro_of(e);
            w.sounds.push(Ui::Select as u32);
            let fields = load_fields(w, i);
            if let Some(e) = edit_mut(w) {
                e.editor = Some(Editor { i, sel: 4, fields, editing: None, caret: 0, lopt: None });
            }
        }
        // FUN_101a4590: back to the book list.
        (_, K::Escape) => {
            w.sounds.push(Ui::Cancel as u32);
            if let Some(b) = w.macroedit.as_mut().and_then(|m| m.book.as_mut()) {
                b.edit = None;
            }
        }
        _ => {}
    }
}

/// FUN_101a5e00: macro i into the fields.
fn load_fields(w: &World, i: usize) -> [Vec<u8>; 7] {
    let Some(m) = w.macros.as_ref().and_then(|m| m.recs.get(i)) else { return Default::default() };
    std::array::from_fn(|f| if f == 0 { m.raw_title().to_vec() } else { m.line(f - 1).to_vec() })
}

/// FUN_101a5da0: the fields stored (a title over 8 bytes or a line over 60 is refused, the old text stays).
fn store_fields(w: &mut World, d: &Editor) {
    let Some(m) = w.macros.as_mut() else { return };
    m.set_title(d.i, &d.fields[0]);
    for k in 0..6 {
        m.set_line(d.i, k, &d.fields[k + 1]);
    }
}

/// The macro editor (FUN_101a5690) and mcrledit (FUN_101a6350).
fn editor_key(w: &mut World, gu: &GameUi, key: egui::Key) {
    use egui::Key as K;
    let clip = w.mcr_line.clone();
    let Some(d) = edit_mut(w).and_then(|e| e.editor.as_mut()) else { return };
    if let Some(o) = d.lopt {
        // mcrledit: on the line (sel 4..9): copy a non-empty line, paste, delete; then the focus goes back.
        if key == K::Escape {
            d.lopt = None;
            w.sounds.push(SE_FOCUS);
            return;
        }
        let f = d.sel as usize - 3;
        let line = d.fields[f].clone();
        match key {
            K::ArrowUp | K::ArrowDown => {
                d.lopt = Some(if key == K::ArrowDown { o % 3 + 1 } else if o == 1 { 3 } else { o - 1 });
                w.sounds.push(Ui::Cursor as u32);
            }
            K::Enter if o == 2 && clip.is_none() => w.sounds.push(Ui::Error as u32),
            K::Enter => {
                d.lopt = None;
                match o {
                    2 => d.fields[f] = clip.unwrap_or_default(),
                    3 => d.fields[f].clear(),
                    _ => {}
                }
                if o == 1 && !line.is_empty() {
                    w.mcr_line = Some(line);
                }
                w.sounds.push(Ui::Select as u32);
            }
            _ => {}
        }
        return;
    }
    let sel = d.sel;
    match key {
        K::ArrowUp | K::ArrowDown | K::ArrowLeft | K::ArrowRight => {
            let k = match key {
                K::ArrowLeft => 5,
                K::ArrowRight => 4,
                K::ArrowUp => 2,
                _ => 3,
            };
            let to = gu.menu_links(MACRO, sel).map_or(sel, |l| l[k] as u16);
            if (1..=9).contains(&to) && to != sel {
                d.sel = to;
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        // FUN_101a5490: edit the field.
        K::Enter if sel >= 3 => {
            d.editing = Some(sel as usize - 3);
            d.caret = d.fields[sel as usize - 3].len();
            w.sounds.push(Ui::Select as u32);
        }
        // Prev / Next: stored, then the macro before / after across Ctrl and Alt (no wrap: SE 0004 at the ends).
        K::Enter => {
            let next = sel == 2;
            let at = d.i;
            let snapshot = Editor { i: d.i, sel: d.sel, fields: d.fields.clone(), editing: None, caret: 0, lopt: None };
            let to = if next { (at < 19).then_some(at + 1) } else { at.checked_sub(1) };
            let Some(to) = to else {
                w.sounds.push(Ui::Error as u32);
                return;
            };
            w.sounds.push(Ui::Select as u32);
            store_fields(w, &snapshot);
            let fields = load_fields(w, to);
            if let Some(d) = edit_mut(w).and_then(|e| e.editor.as_mut()) {
                (d.i, d.fields) = (to, fields);
            }
        }
        // FUN_101a4630: stored, the editor closes, the palettes and mcrbedit come back.
        K::Escape => {
            let snapshot = Editor { i: d.i, sel: d.sel, fields: d.fields.clone(), editing: None, caret: 0, lopt: None };
            w.sounds.push(Ui::Cancel as u32);
            store_fields(w, &snapshot);
            if let Some(e) = edit_mut(w) {
                e.editor = None;
            }
        }
        _ => {}
    }
}

pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let (Some(me), Some(m)) = (&w.macroedit, &w.macros) else { return };
    if let Some(s) = &me.sets {
        super::esmenu::bars(gu, w, s, bars);
        return;
    }
    let focus = |bars: &mut super::hudtop::BarText, name: &str, child: u16| {
        let [help, title] = gu.focus_text(name, child);
        (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
    };
    let Some(b) = &me.book else {
        focus(bars, MENU, me.menu_sel);
        return;
    };
    if let Some(e) = &b.edit {
        let i = e.editor.as_ref().map_or(macro_of(e), |d| d.i);
        bars.title[2] = m.recs.get(i).map_or(String::new(), |r| r.title());
        if let Some(d) = &e.editor {
            match d.lopt {
                Some(o) => focus(bars, LEDIT, o),
                None => focus(bars, MACRO, d.sel),
            }
        } else if let Some(o) = e.bopt {
            focus(bars, BEDIT, o);
            if set_mode(e) {
                bars.help[1] = gu.menu_msg([0x29c, 0x29d, 0x29e][o as usize - 1], "");
            }
        } else {
            // The palettes: "Macro Set %s (#%d)" (group 10 0x27) and the title "Edit Macros".
            let fmt = gu.lobby_msg(0x0a, 0x27, "Macro Set %s (#%d)");
            bars.help[1] = fmt.replacen("%s", &m.book_title(m.book), 1).replacen("%d", &(m.set + 1).to_string(), 1);
            bars.title[1] = gu.title_msg(75).unwrap_or_default();
        }
        return;
    }
    if let Some(o) = b.opt {
        focus(bars, SELOP, o);
        return;
    }
    // mcrselec's overrides: "Macro Book (#%d)" (group 10 0x6d) and the book's name.
    let book = book_of(w, b.sel);
    bars.help[1] = gu.lobby_msg(0x0a, 0x6d, "Macro Book (#%d)").replacen("%d", &(book + 1).to_string(), 1);
    bars.title[1] = m.book_title(book);
}

/// A text field's caret phase (40 frames on, 20 off at 60 a second, inf).
pub(super) fn caret(w: &World) -> &'static str {
    if (w.started.elapsed().as_secs_f32() * 60.0) as u32 % 60 < 40 { "|" } else { "" }
}

/// The caret drawn at byte `at` of a field shown from (x, y), kept within `max` px of x.
pub(super) fn caret_at(gu: &mut GameUi, w: &World, text: &[u8], at: usize, x: f32, y: f32, max: f32) {
    let c = caret(w);
    if c.is_empty() {
        return;
    }
    let dx = gu.text_width(&shown(w, &text[..at.min(text.len())])).min(max);
    gu.text(c, x + dx - 2.0, y, 0, WHITE.to_le_bytes());
}

/// The modern layer's view (this module keeps every rule and key): mcrmenu, the book list with mcrselop and the name
/// field, the palettes with mcrbedit, the editor with mcrledit. None while Edit Equip. Set is up.
pub(super) struct View {
    /// mcrmenu: its cursor, Edit Equip. Set off (Monstrosity), whether it has the focus.
    pub menu: (u16, bool, bool),
    pub books: Option<Books>,
    pub palettes: Option<Palettes>,
    pub editor: Option<EditorView>,
}

/// mcrselec over both pages: the 40 names, the active book, the cursor's book, mcrselop's cursor, the name field.
pub(super) struct Books {
    pub names: Vec<String>,
    pub active: usize,
    pub cur: usize,
    pub opt: Option<u16>,
    pub paste_ok: bool,
    pub rename: Option<String>,
    /// The rename field's caret (a char index into `rename`).
    pub rename_caret: usize,
    pub focused: bool,
}

/// The two palettes: the twenty titles (Ctrl 0..9, Alt 10..19), the focused palette and each one's cell, mcrbedit's
/// cursor, the book and set.
pub(super) struct Palettes {
    pub titles: Vec<String>,
    pub focus: u8,
    pub cols: [u16; 2],
    pub bopt: Option<u16>,
    pub paste_ok: bool,
    pub book: String,
    pub set: usize,
    pub focused: bool,
    /// The modern page's set strip has the cursor.
    pub tabs: bool,
}

/// "menu    macro": "Ctrl 1" or the like, the title and six lines (the edited one live), the cursor, mcrledit's.
pub(super) struct EditorView {
    pub label: String,
    pub fields: Vec<String>,
    pub sel: u16,
    pub editing: Option<usize>,
    /// The edited field's caret (a char index into its text).
    pub caret: usize,
    pub lopt: Option<u16>,
    pub paste_ok: bool,
    pub focused: bool,
}

pub(super) fn view(w: &World, gu: &GameUi) -> Option<View> {
    let (me, m) = (w.macroedit.as_ref()?, w.macros.as_ref()?);
    if me.sets.is_some() {
        return None;
    }
    let free = w.confirm.is_none();
    let menu = (me.menu_sel, super::monstrosity::active(w), free && me.book.is_none());
    let text = |b: &[u8]| shown(w, b);
    let books = me.book.as_ref().map(|b| Books {
        names: (0..40).map(|k| m.book_title(k)).collect(),
        active: m.book,
        cur: book_of(w, b.sel),
        opt: b.opt,
        paste_ok: m.book_copied(),
        rename: b.rename.as_ref().map(|t| text(t)),
        rename_caret: b.rename.as_ref().map_or(0, |t| text(&t[..b.caret.min(t.len())]).chars().count()),
        focused: free && b.opt.is_none() && b.edit.is_none() && b.rename.is_none(),
    });
    let edit = me.book.as_ref().and_then(|b| b.edit.as_ref());
    let palettes = edit.filter(|e| e.editor.is_none()).map(|e| Palettes {
        titles: m.recs.iter().take(20).map(|r| r.title()).collect(),
        focus: e.focus,
        cols: e.cols,
        bopt: e.bopt,
        paste_ok: if set_mode(e) { m.set_copied } else { m.macro_copied() },
        book: m.book_title(m.book),
        set: m.set,
        focused: free && e.bopt.is_none() && !e.tabs,
        tabs: e.tabs,
    });
    let editor = edit.and_then(|e| e.editor.as_ref()).map(|d| {
        let (alt, n) = (d.i >= 10, (d.i % 10 + 1) % 10);
        EditorView {
            label: gu.lobby_msg(0x0a, if alt { 0x5c } else { 0x5b }, "Ctrl %d").replacen("%d", &n.to_string(), 1),
            fields: d.fields.iter().map(|f| text(f)).collect(),
            sel: d.sel,
            editing: d.editing,
            caret: d.editing.map_or(0, |f| text(&d.fields[f][..d.caret.min(d.fields[f].len())]).chars().count()),
            lopt: d.lopt,
            paste_ok: w.mcr_line.is_some(),
            focused: free && d.lopt.is_none() && d.editing.is_none(),
        }
    });
    Some(View { menu, books, palettes, editor })
}

/// The modern layer's mouse: mcrmenu's cursor to `c`; true when it moved.
pub(super) fn set_menu(w: &mut World, c: u16) -> bool {
    let Some(m) = w.macroedit.as_mut().filter(|m| m.book.is_none() && m.sets.is_none()) else { return false };
    let moved = m.menu_sel != c;
    m.menu_sel = c;
    moved
}

/// The book list's cursor onto book `k` (0..39: its page and row).
pub(super) fn set_book(w: &mut World, k: usize) -> bool {
    let Some(b) = w.macroedit.as_mut().and_then(|m| m.book.as_mut()).filter(|b| b.opt.is_none() && b.edit.is_none() && b.rename.is_none()) else { return false };
    let sel = (k % 20) as u16 + 1;
    let moved = b.sel != sel || w.mcr_page != k / 20;
    b.sel = sel;
    w.mcr_page = k / 20;
    moved
}

/// mcrselop's cursor onto row `o`.
pub(super) fn set_opt(w: &mut World, o: u16) -> bool {
    let Some(b) = w.macroedit.as_mut().and_then(|m| m.book.as_mut()).filter(|b| b.opt.is_some()) else { return false };
    let moved = b.opt != Some(o);
    b.opt = Some(o);
    moved
}

/// The modern page's set strip given the cursor (or taken it).
pub(super) fn set_tabs(w: &mut World, on: bool) {
    if let Some(e) = edit_mut(w).filter(|e| e.editor.is_none()) {
        e.tabs = on;
        if on {
            e.bopt = None;
        }
    }
}

/// The palettes' cursor onto palette `p` (1 Ctrl, 2 Alt) at cell `col` (0..9), mcrbedit closed.
pub(super) fn set_cell(w: &mut World, p: u8, col: u16) -> bool {
    let Some(e) = edit_mut(w).filter(|e| e.editor.is_none()) else { return false };
    let moved = e.focus != p || e.cols[p as usize - 1] != col || e.bopt.is_some() || e.tabs;
    (e.focus, e.bopt, e.tabs) = (p, None, false);
    e.cols[p as usize - 1] = col;
    moved
}

/// mcrbedit opened (as Num+ does) with its cursor on `o`.
pub(super) fn set_bopt(w: &mut World, o: u16) {
    if let Some(e) = edit_mut(w).filter(|e| e.editor.is_none()) {
        (e.bopt, e.tabs) = (Some(o), false);
    }
}

/// Another set of the book (FUN_101a2890: the set file written, the other read).
pub(super) fn set_set(w: &mut World, set: usize) {
    if let Some(m) = w.macros.as_mut().filter(|m| m.set != set) {
        let book = m.book;
        m.switch(book, set);
        w.sounds.push(Ui::Cursor as u32);
    }
}

/// The editor's cursor onto child `c` (1 Prev, 2 Next, 3 the title, 4..9 the lines), mcrledit closed.
pub(super) fn set_field(w: &mut World, c: u16) -> bool {
    let Some(d) = edit_mut(w).and_then(|e| e.editor.as_mut()).filter(|d| d.editing.is_none()) else { return false };
    let moved = d.sel != c || d.lopt.is_some();
    (d.sel, d.lopt) = (c, None);
    moved
}

/// mcrledit opened on the cursor's line with its cursor on `o`.
pub(super) fn set_lopt(w: &mut World, o: u16) {
    if let Some(d) = edit_mut(w).and_then(|e| e.editor.as_mut()).filter(|d| d.editing.is_none() && (4..=9).contains(&d.sel)) {
        d.lopt = Some(o);
    }
}

pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    // The modern layer draws these and takes their mouse.
    if w.modern {
        return rects;
    }
    let (Some(me), Some(m)) = (&w.macroedit, &w.macros) else { return rects };
    let free = w.confirm.is_none();
    let off: &[u16] = if super::monstrosity::active(w) { &[2] } else { &[] };
    if let Some(win) = gu.lobby_window_off(MENU, sw, sh, (0.0, 0.0), Some(me.menu_sel), free && me.book.is_none() && me.sets.is_none(), off) {
        gu.disabled_children(MENU, win.frame[0], win.frame[1], off, Some(me.menu_sel));
        hit(w, MENU, &win);
        rects.push(win.frame);
    }
    if let Some(s) = &me.sets {
        rects.extend(super::esmenu::draw(gu, w, s, sw, sh, free));
        return rects;
    }
    let Some(b) = &me.book else { return rects };
    // mcrselec: the active book's red bar (keytops3 87), the names in mcralpha at (8, 2) cut before 75 px, or the
    // rename field.
    let list_focus = free && b.opt.is_none() && b.edit.is_none() && b.rename.is_none();
    if let Some(win) = gu.lobby_window_off(SELEC, sw, sh, (0.0, 0.0), Some(b.sel), list_focus, &[]) {
        hit(w, SELEC, &win);
        for &(id, r) in win.children.iter().filter(|(id, _)| (1..=20).contains(id)) {
            let book = book_of(w, id);
            if book == m.book {
                gu.sprite("menu    keytops3", 87, r[0], r[1], WHITE);
            }
            match &b.rename {
                Some(t) if id == b.sel => {
                    gu.text(&shown(w, t), r[0] + 8.0, r[1] + 2.0, 0, WHITE.to_le_bytes());
                    caret_at(gu, w, t, b.caret, r[0] + 8.0, r[1] + 2.0, f32::MAX);
                }
                _ => alpha(gu, m.raw_book_title(book), r[0] + 8.0, r[1] + 2.0, 75.0),
            }
        }
        rects.push(win.frame);
        // mcrselop left of the cursor row (FUN_10119110): x = row x - its width, y = min(row y, mcrselec bottom - its
        // height).
        if let (Some(o), Some(&(_, r)), Some([lx, ly, lw, lh])) = (b.opt, win.children.iter().find(|(id, _)| *id == b.sel), gu.window_rect(SELOP, sw, sh)) {
            let [_, fy, _, fh] = win.frame;
            let (x, y) = (r[0] - lw, r[1].min(fy + fh - lh));
            let off: Vec<u16> = if m.book_copied() { Vec::new() } else { vec![4] };
            if let Some(win) = gu.lobby_window_off(SELOP, sw, sh, (x - lx, y - ly), Some(o), free, &off) {
                hit(w, SELOP, &win);
                gu.disabled_children(SELOP, win.frame[0], win.frame[1], &off, free.then_some(o));
                rects.push(win.frame);
            }
        }
    }
    let Some(e) = &b.edit else { return rects };
    if let Some(d) = &e.editor {
        rects.extend(draw_editor(gu, w, d, sw, sh, free));
        return rects;
    }
    // The palettes, the focused one drawn last, Alt 42 lower; mcrbedit beside them.
    let long = w.game.get(0x9d) == 0 && sw >= 1024.0;
    let dx = palette_dx(w);
    let order = if e.focus == 1 { [2u8, 1] } else { [1, 2] };
    for p in order {
        let name = format!("menu    mcr{p}{}", if long { "edlo" } else { "edit" });
        let focused = free && e.focus == p && e.bopt.is_none();
        let sel = e.cols[p as usize - 1] + 1;
        let dy = if p == 2 { 42.0 } else { 0.0 };
        if let Some(win) = gu.lobby_window_off(&name, sw, sh, (dx, dy), Some(sel), focused, &[]) {
            hit(w, &format!("palette{p}"), &win);
            let pad = if win.frame[2] >= 800.0 { 4.0 } else { 2.0 };
            for (k, (_, r)) in win.children.iter().take(10).enumerate() {
                let title = m.recs[k + 10 * (p as usize - 1)].title();
                cell_title(gu, &title, *r, pad);
            }
            rects.push(win.frame);
        }
    }
    let can_paste = if set_mode(e) { m.set_copied } else { m.macro_copied() };
    let off: Vec<u16> = if can_paste { Vec::new() } else { vec![2] };
    if let Some(win) = gu.lobby_window_off(BEDIT, sw, sh, (0.0, 0.0), Some(e.bopt.unwrap_or(1)), free && e.bopt.is_some(), &off) {
        hit(w, BEDIT, &win);
        gu.disabled_children(BEDIT, win.frame[0], win.frame[1], &off, e.bopt.filter(|_| free));
        rects.push(win.frame);
    }
    rects
}

/// FUN_101a5af0: "Ctrl %d" / "Alt %d" (group 10 0x5b / 0x5c) at (0x22, 0x24); the title at child 3 (+2, +5) (the live
/// field, 120 wide, while edited); the lines at (28, 73 + 30 k) cut at 315 (the live field 325); mcrledit beside it.
fn draw_editor(gu: &mut GameUi, w: &World, d: &Editor, sw: f32, sh: f32, free: bool) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    let focused = free && d.lopt.is_none() && d.editing.is_none();
    let Some(win) = gu.lobby_window_off(MACRO, sw, sh, (0.0, 0.0), Some(d.sel), focused, &[]) else { return rects };
    hit(w, MACRO, &win);
    let [x0, y0, ..] = win.frame;
    let (alt, n) = (d.i >= 10, (d.i % 10 + 1) % 10);
    let label = gu.lobby_msg(0x0a, if alt { 0x5c } else { 0x5b }, "Ctrl %d").replacen("%d", &n.to_string(), 1);
    gu.text(&label, x0 + 34.0, y0 + 36.0, 0, WHITE.to_le_bytes());
    let text = |f: usize| shown(w, &d.fields[f]);
    if let Some(&(_, r)) = win.children.iter().find(|(id, _)| *id == 3) {
        gu.text_fit(&text(0), r[0] + 2.0, r[1] + 5.0, 120.0, WHITE);
        if d.editing == Some(0) {
            caret_at(gu, w, &d.fields[0], d.caret, r[0] + 2.0, r[1] + 5.0, 120.0);
        }
    }
    for k in 0..6 {
        let (x, y) = (x0 + 28.0, y0 + 73.0 + 30.0 * k as f32);
        if d.editing == Some(k + 1) {
            gu.text_fit(&text(k + 1), x, y, 325.0, WHITE);
            caret_at(gu, w, &d.fields[k + 1], d.caret, x, y, 325.0);
        } else {
            gu.text_fit(&text(k + 1), x, y, 315.0, WHITE);
        }
    }
    rects.push(win.frame);
    let off: Vec<u16> = if w.mcr_line.is_some() { Vec::new() } else { vec![2] };
    if let Some(win) = gu.lobby_window_off(LEDIT, sw, sh, (0.0, 0.0), Some(d.lopt.unwrap_or(1)), free && d.lopt.is_some(), &off) {
        hit(w, LEDIT, &win);
        gu.disabled_children(LEDIT, win.frame[0], win.frame[1], &off, d.lopt.filter(|_| free));
        rects.push(win.frame);
    }
    rects
}

/// A window as drawn this frame, for the mouse.
pub(super) fn hit(w: &World, name: &str, win: &super::game_ui::LobbyWin) {
    super::winhits::record(w, name, win);
}

pub(super) use super::winhits::child_under;

/// The generic window mouse with this chain's keys.
#[allow(clippy::too_many_arguments)]
pub(super) fn generic(w: &mut World, gu: &GameUi, name: &str, cursor: u16, ids: std::ops::RangeInclusive<u16>, p: (f32, f32), moved: bool, clicked: bool, right: bool, set: impl FnOnce(&mut World, u16)) {
    super::winhits::generic(w, gu, name, cursor, ids, p, moved, clicked, right, set, key);
}

/// The mouse on the chain (no typing field open). Over the other palette the focus moves there at that column
/// *(inf: retail's window focus on a click)*.
pub fn pointer(w: &mut World, gu: &GameUi, _sw: f32, _sh: f32, p: (f32, f32), moved: bool, clicked: bool, right: bool) {
    if w.modern {
        return;
    }
    let Some(me) = w.macroedit.as_ref() else { return };
    if me.sets.is_some() {
        super::esmenu::pointer(w, gu, _sw, _sh, p, moved, clicked, right);
        return;
    }
    let Some(b) = me.book.as_ref() else {
        let sel = me.menu_sel;
        generic(w, gu, MENU, sel, 1..=2, p, moved, clicked, right, |w, id| {
            if let Some(m) = w.macroedit.as_mut() {
                m.menu_sel = id;
            }
        });
        return;
    };
    if b.rename.is_some() {
        return;
    }
    let (sel, opt) = (b.sel, b.opt);
    let Some(e) = b.edit.as_ref() else {
        match opt {
            Some(o) => generic(w, gu, SELOP, o, 1..=5, p, moved, clicked, right, |w, id| {
                if let Some(b) = w.macroedit.as_mut().and_then(|m| m.book.as_mut()) {
                    b.opt = Some(id);
                }
            }),
            None => generic(w, gu, SELEC, sel, 1..=20, p, moved, clicked, right, |w, id| {
                if let Some(b) = w.macroedit.as_mut().and_then(|m| m.book.as_mut()) {
                    b.sel = id;
                }
            }),
        }
        return;
    };
    match (&e.editor, e.bopt) {
        (Some(d), _) if d.editing.is_some() => {}
        (Some(d), _) => match d.lopt {
            Some(o) => generic(w, gu, LEDIT, o, 1..=3, p, moved, clicked, right, |w, id| {
                if let Some(d) = edit_mut(w).and_then(|e| e.editor.as_mut()) {
                    d.lopt = Some(id);
                }
            }),
            None => {
                let sel = d.sel;
                generic(w, gu, MACRO, sel, 1..=9, p, moved, clicked, right, |w, id| {
                    if let Some(d) = edit_mut(w).and_then(|e| e.editor.as_mut()) {
                        d.sel = id;
                    }
                });
            }
        },
        (None, Some(o)) => generic(w, gu, BEDIT, o, 1..=3, p, moved, clicked, right, |w, id| {
            if let Some(e) = edit_mut(w) {
                e.bopt = Some(id);
            }
        }),
        (None, None) => {
            let (focus, cols) = (e.focus, e.cols);
            let other = 3 - focus;
            if let (Some(id), _) = child_under(w, &format!("palette{other}"), p, 1..=10)
                && (moved || clicked)
            {
                gu.hover.set(true);
                if let Some(e) = edit_mut(w) {
                    e.focus = other;
                    e.cols[other as usize - 1] = id - 1;
                }
                w.sounds.push(Ui::Cursor as u32);
                return;
            }
            let p_name = format!("palette{focus}");
            generic(w, gu, &p_name, cols[focus as usize - 1] + 1, 1..=10, p, moved, clicked, right, |w, id| {
                if let Some(e) = edit_mut(w) {
                    e.cols[focus as usize - 1] = id - 1;
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyphs() {
        assert_eq!([b'1', b'9', b'0', b'a', b'z', b'A', b'Z'].map(alpha_index), [Some(0), Some(8), Some(9), Some(10), Some(35), Some(36), Some(61)]);
        assert_eq!(alpha_index(b' '), None);
    }
}
