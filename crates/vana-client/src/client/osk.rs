//! The on-screen keyboard for text fields while a gamepad is in use (ours, not retail's software keyboard, whose
//! keypad windows are not built). Ported from Blizz127/vana-client (develop, af02adc / b491cd0), with the pad side
//! moved onto client/gamepad.rs (`gamepad::osk_keys`).
//!
//! Under Steam (the `SteamAppId` / `SteamGameId` it sets for the games it starts, a non-Steam shortcut included, so Game
//! Mode and the Steam Deck too) a request opens Steam's own keyboard with the `steam://open/keyboard` URL, through the
//! system's URL handler (xdg-open on Linux, ShellExecute on Windows), without waiting for it and ignoring a failure.
//! Anywhere else it is ours: a small egui keyboard (`Keyboard`) over the field, with a fixed layout, driven by the pad
//! (d-pad moves, A types, B / LB erase, X turns the key page, Y spaces, RB / Start send Enter, Back closes) and by the
//! mouse. It feeds the game the events a keyboard would (`Text` per character, key presses for Backspace / Enter, Space
//! as key plus text), so every text field takes it without knowing it is there: the fields read the raw input events,
//! not egui widgets, so clicking a key does not take the focus from the field.
//!
//! It goes up when a text field takes the keys with the pad the last device used (`Edge`), or on RB in a text field.

use egui::{Event, Key, Modifiers};

/// The field that took the focus: where the keyboard sits (the chat line is at the bottom, so the keyboard goes to the
/// top for it) and what a richer keyboard would lay out for.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Field {
    /// The login screen's fields.
    Login,
    /// The chat line.
    Chat,
    /// The character-creation name entry.
    Name,
    /// Any other field (searches, event inputs).
    #[default]
    Other,
}

/// Steam's keyboard URL (Big Picture / Steam Deck text entry).
const STEAM_KEYBOARD: &str = "steam://open/keyboard";

/// Whether the game was started by Steam: `env` reads an environment variable.
pub fn under_steam(env: impl Fn(&str) -> Option<String>) -> bool {
    ["SteamAppId", "SteamGameId"].iter().any(|k| env(k).is_some_and(|v| !v.trim().is_empty()))
}

/// Which keyboard a request raised.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Raised {
    /// Steam's, opened through its URL: nothing more to do here.
    Steam,
    /// None yet: the caller opens ours (`Keyboard::open`).
    Builtin,
}

/// Steam's keyboard where Steam started the game, else ours.
fn choose(steam: bool) -> Raised {
    if steam { Raised::Steam } else { Raised::Builtin }
}

/// Raises the on-screen keyboard for `field`: Steam's under Steam (opened here; never blocks, never fails), else it says
/// so and the caller opens its `Keyboard`.
pub fn request(field: Field) -> Raised {
    let raised = choose(under_steam(|k| std::env::var(k).ok()));
    match raised {
        Raised::Steam => {
            log::info!("osk: Steam keyboard for {field:?}");
            open_url(STEAM_KEYBOARD);
        }
        Raised::Builtin => log::debug!("osk: built-in keyboard for {field:?}"),
    }
    raised
}

#[cfg(windows)]
fn open_url(url: &str) {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let wide: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
    let verb: Vec<u16> = "open".encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: both strings are NUL-terminated and outlive the call; the rest are null (no window, parameters or
    // directory). ShellExecuteW starts the handler without waiting.
    unsafe {
        ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), wide.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL);
    }
}

#[cfg(not(windows))]
fn open_url(url: &str) {
    use std::process::{Command, Stdio};
    let started = Command::new("xdg-open").arg(url).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    match started {
        // Reaped off-thread so the child does not linger as a zombie.
        Ok(mut child) => {
            std::thread::spawn(move || _ = child.wait());
        }
        Err(e) => log::debug!("osk: xdg-open: {e}"),
    }
}

/// The key pages. The shift is one-shot (one character, then back), as on a phone.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Page {
    #[default]
    Lower,
    Upper,
    Symbols,
}

/// What a key does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cap {
    Char(char),
    Space,
    Backspace,
    /// Sends Enter to the field (the chat line sends, the login moves on).
    Enter,
    /// Lower <-> upper (from the symbols: upper).
    Shift,
    /// Letters <-> symbols.
    Symbols,
}

const DIGITS: &str = "1234567890";

/// The character rows of a page: every row has ten keys, the digits on every page (the world pass takes only those).
fn char_rows(page: Page) -> [&'static str; 4] {
    match page {
        Page::Lower => [DIGITS, "qwertyuiop", "asdfghjkl'", "zxcvbnm,./"],
        Page::Upper => [DIGITS, "QWERTYUIOP", "ASDFGHJKL'", "ZXCVBNM,./"],
        Page::Symbols => [DIGITS, "!@#$%^&*()", "-_=+[]{};:", "\\|<>?/~`'\""],
    }
}

/// The keys of a page, row by row, with their widths in key units: four rows of ten, then Shift, 123 / abc, Space,
/// Backspace and Enter (ten units).
pub fn rows(page: Page) -> Vec<Vec<(Cap, f32)>> {
    let mut rows: Vec<Vec<(Cap, f32)>> = char_rows(page).iter().map(|r| r.chars().map(|c| (Cap::Char(c), 1.0)).collect()).collect();
    rows.push(vec![(Cap::Shift, 1.5), (Cap::Symbols, 1.5), (Cap::Space, 4.0), (Cap::Backspace, 1.5), (Cap::Enter, 1.5)]);
    rows
}

/// A key's label on `page`.
pub fn label(cap: Cap, page: Page) -> String {
    match cap {
        Cap::Char(c) => c.to_string(),
        Cap::Space => "space".into(),
        Cap::Backspace => "bksp".into(),
        Cap::Enter => "enter".into(),
        Cap::Shift => if page == Page::Upper { "abc" } else { "ABC" }.into(),
        Cap::Symbols => if page == Page::Symbols { "abc" } else { "123" }.into(),
    }
}

/// Where a key's middle is along its row, in key units.
fn centre(row: &[(Cap, f32)], col: usize) -> f32 {
    row[..col].iter().map(|k| k.1).sum::<f32>() + row[col].1 / 2.0
}

/// The key of `row` whose middle is nearest `x` (the first on a tie).
fn nearest(row: &[(Cap, f32)], x: f32) -> usize {
    (0..row.len()).min_by(|&a, &b| (centre(row, a) - x).abs().total_cmp(&(centre(row, b) - x).abs())).unwrap_or(0)
}

fn key_event(key: Key, pressed: bool) -> Event {
    Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE }
}

/// The built-in keyboard: its state, the pad's and the mouse's input, and its drawing. Closed until `open`.
#[derive(Default)]
pub struct Keyboard {
    open: bool,
    field: Field,
    page: Page,
    /// The key under the pad's cursor.
    row: usize,
    col: usize,
    /// Where it was drawn last frame (the game's windows get no clicks there), and whether the pointer is over it.
    rect: Option<egui::Rect>,
    hovered: bool,
}

impl Keyboard {
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens over `field` on the letters, the cursor on Q.
    pub fn open(&mut self, field: Field) {
        *self = Self { open: true, field, row: 1, ..Self::default() };
    }

    pub fn close(&mut self) {
        self.open = false;
        self.rect = None;
        self.hovered = false;
    }

    /// Where it was drawn last frame, for the game's click-through test.
    #[cfg(test)]
    pub fn rect(&self) -> Option<egui::Rect> {
        self.rect
    }

    /// The pointer is over the keyboard (the system cursor shows then: the game's drawn one would be under it).
    pub fn hovered(&self) -> bool {
        self.hovered
    }

    #[cfg(test)]
    pub fn page(&self) -> Page {
        self.page
    }

    /// The key under the cursor.
    pub fn under_cursor(&self) -> Cap {
        rows(self.page)[self.row][self.col].0
    }

    /// Moves the cursor: along the row, wrapping, or to the row above / below (wrapping) at the key nearest the
    /// column it left.
    pub fn step(&mut self, dx: i32, dy: i32) {
        let rows = rows(self.page);
        if dx != 0 {
            self.col = (self.col as i32 + dx).rem_euclid(rows[self.row].len() as i32) as usize;
        }
        if dy != 0 {
            let x = centre(&rows[self.row], self.col);
            self.row = (self.row as i32 + dy).rem_euclid(rows.len() as i32) as usize;
            self.col = nearest(&rows[self.row], x);
        }
    }

    /// The next key page (X): letters, shifted letters, symbols. The cursor keeps its place (all the pages have the
    /// same shape).
    pub fn next_page(&mut self) {
        self.page = match self.page {
            Page::Lower => Page::Upper,
            Page::Upper => Page::Symbols,
            Page::Symbols => Page::Lower,
        };
    }

    /// A key struck by the pad or the mouse: the events a keyboard would send for it.
    pub fn press(&mut self, cap: Cap) -> Vec<Event> {
        let tap = |k: Key| vec![key_event(k, true), key_event(k, false)];
        match cap {
            Cap::Char(c) => {
                // The shift is for one character.
                if self.page == Page::Upper {
                    self.page = Page::Lower;
                }
                vec![Event::Text(c.to_string())]
            }
            // winit sends the space as text too.
            Cap::Space => {
                let mut v = tap(Key::Space);
                v.push(Event::Text(" ".into()));
                v
            }
            Cap::Backspace => tap(Key::Backspace),
            Cap::Enter => tap(Key::Enter),
            Cap::Shift => {
                self.page = if self.page == Page::Upper { Page::Lower } else { Page::Upper };
                Vec::new()
            }
            Cap::Symbols => {
                self.page = if self.page == Page::Symbols { Page::Lower } else { Page::Symbols };
                Vec::new()
            }
        }
    }

    /// One of the pad's events (`gamepad::osk_keys`; presses and repeats only: the release of a button held before the
    /// keyboard opened lands here too and is dropped): the events to give the game.
    pub fn pad_event(&mut self, ev: &Event) -> Vec<Event> {
        let Event::Key { key, pressed: true, .. } = ev else { return Vec::new() };
        if !self.open {
            return Vec::new();
        }
        match key {
            Key::ArrowLeft => self.step(-1, 0),
            Key::ArrowRight => self.step(1, 0),
            Key::ArrowUp => self.step(0, -1),
            Key::ArrowDown => self.step(0, 1),
            Key::Enter => return self.press(self.under_cursor()),
            Key::Backspace => return self.press(Cap::Backspace),
            Key::Space => return self.press(Cap::Space),
            Key::Tab => self.next_page(),
            Key::F2 => return self.press(Cap::Enter),
            Key::Escape => self.close(),
            _ => {}
        }
        Vec::new()
    }

    /// Draws it (nothing when closed) over everything and returns the events its mouse clicks make.
    pub fn show(&mut self, ctx: &egui::Context) -> Vec<Event> {
        if !self.open {
            self.rect = None;
            self.hovered = false;
            return Vec::new();
        }
        let screen = ctx.content_rect();
        // Ten keys and a margin across, six rows of height at most half the screen.
        let unit = (screen.width() / 12.0).clamp(28.0, 60.0).min(screen.height() * 0.5 / 6.5);
        let gap = 4.0;
        let (page, cursor) = (self.page, (self.row, self.col));
        let rows = rows(page);
        let top = self.field == Field::Chat;
        let mut clicked = None;
        let mut close = false;
        let area = egui::Area::new(egui::Id::unique("vana_osk"))
            .order(egui::Order::Tooltip)
            .anchor(if top { egui::Align2::CENTER_TOP } else { egui::Align2::CENTER_BOTTOM }, [0.0, if top { 8.0 } else { -8.0 }])
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
                    ui.horizontal(|ui| {
                        ui.add(egui::Label::new(egui::RichText::new("A type   B bksp   X page   Y space   Start enter   Back close").size((unit * 0.3).max(11.0))).truncate());
                        if ui.small_button("Close").clicked() {
                            close = true;
                        }
                    });
                    for (r, row) in rows.iter().enumerate() {
                        ui.horizontal(|ui| {
                            for (c, &(cap, w)) in row.iter().enumerate() {
                                let size = egui::vec2(w * unit + (w - 1.0) * gap, unit);
                                let text = egui::RichText::new(label(cap, page)).size(unit * 0.42);
                                if ui.add_sized(size, egui::Button::new(text).selected((r, c) == cursor)).clicked() {
                                    clicked = Some((r, c, cap));
                                }
                            }
                        });
                    }
                });
            });
        self.rect = Some(area.response.rect);
        self.hovered = ctx.input(|i| i.pointer.hover_pos()).is_some_and(|p| area.response.rect.contains(p));
        if close {
            self.close();
            return Vec::new();
        }
        match clicked {
            Some((r, c, cap)) => {
                (self.row, self.col) = (r, c);
                self.press(cap)
            }
            None => Vec::new(),
        }
    }
}

/// A text field opening, once: the keyboard goes up on the frame a field gains focus with the pad the last device
/// used, never again while it stays open or when the keyboard and mouse were used.
#[derive(Default)]
pub struct Edge {
    was: bool,
}

impl Edge {
    /// `open`: a text field has the keys now; `pad`: the pad was the last device. True on the frame to raise it.
    pub fn update(&mut self, open: bool, pad: bool) -> bool {
        let rose = open && !self.was;
        self.was = open;
        rose && pad
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steam_is_told_by_its_variables() {
        let with = |name: &'static str, v: &'static str| move |k: &str| (k == name).then(|| v.to_string());
        assert!(under_steam(with("SteamAppId", "480")));
        assert!(under_steam(with("SteamGameId", "12345678")));
        assert!(!under_steam(with("SteamAppId", "")));
        assert!(!under_steam(with("SteamAppId", "  ")));
        assert!(!under_steam(|_| None));
        assert!(!under_steam(with("Steam", "1")));
    }

    #[test]
    fn steam_or_ours() {
        assert_eq!(choose(true), Raised::Steam);
        assert_eq!(choose(false), Raised::Builtin);
    }

    fn chars(page: Page) -> String {
        rows(page).iter().flatten().filter_map(|k| if let Cap::Char(c) = k.0 { Some(c) } else { None }).collect()
    }

    #[test]
    fn the_grid_is_regular() {
        for page in [Page::Lower, Page::Upper, Page::Symbols] {
            let rows = rows(page);
            // Every row is ten units wide, so the rows line up; the same shape on every page, so the cursor keeps its
            // place across a page turn.
            for r in &rows {
                assert_eq!(r.iter().map(|k| k.1).sum::<f32>(), 10.0, "{page:?}");
            }
            assert_eq!(rows.iter().map(Vec::len).collect::<Vec<_>>(), [10, 10, 10, 10, 5]);
            // The digits are on every page (the world pass takes only those).
            assert!(chars(page).starts_with(DIGITS));
            // No character twice on a page.
            let mut all: Vec<char> = chars(page).chars().collect();
            let n = all.len();
            all.sort_unstable();
            all.dedup();
            assert_eq!(all.len(), n, "{page:?}");
        }
        // Every letter in both cases; the symbols cover the usual password ones.
        for c in 'a'..='z' {
            assert!(chars(Page::Lower).contains(c) && chars(Page::Upper).contains(c.to_ascii_uppercase()));
        }
        for c in "!@#$%^&*()-_=+[]{};:'\"\\|<>?/~`".chars() {
            assert!(chars(Page::Symbols).contains(c), "{c}");
        }
        assert_eq!(label(Cap::Shift, Page::Lower), "ABC");
        assert_eq!(label(Cap::Symbols, Page::Symbols), "abc");
    }

    fn opened() -> Keyboard {
        let mut kb = Keyboard::default();
        kb.open(Field::Other);
        kb
    }

    #[test]
    fn cursor_moves_and_wraps() {
        let mut kb = opened();
        assert!(kb.is_open());
        assert_eq!(kb.under_cursor(), Cap::Char('q'));
        kb.step(0, 1);
        assert_eq!(kb.under_cursor(), Cap::Char('a'));
        kb.step(1, 0);
        assert_eq!(kb.under_cursor(), Cap::Char('s'));
        // Left from the first key wraps to the row's last.
        kb.step(0, -1);
        kb.step(-1, 0);
        kb.step(-1, 0);
        assert_eq!(kb.under_cursor(), Cap::Char('p'));
        // Up from the top row wraps to the bottom row, at the key nearest (Enter under the right edge); and back.
        kb.step(0, -1);
        assert_eq!(kb.under_cursor(), Cap::Char('0'));
        kb.step(0, -1);
        assert_eq!(kb.under_cursor(), Cap::Enter);
        kb.step(0, 1);
        assert_eq!(kb.under_cursor(), Cap::Char('0'));
    }

    #[test]
    fn vertical_moves_keep_the_column_across_unequal_rows() {
        let mut kb = opened();
        for _ in 0..2 {
            kb.step(0, 1);
        }
        assert_eq!(kb.under_cursor(), Cap::Char('z'));
        kb.step(0, 1);
        assert_eq!(kb.under_cursor(), Cap::Shift);
        kb.step(1, 0);
        kb.step(1, 0);
        assert_eq!(kb.under_cursor(), Cap::Space);
        // Space's middle is 5.0 units along: between 'b' (4.5) and 'n' (5.5); the first wins the tie.
        kb.step(0, -1);
        assert_eq!(kb.under_cursor(), Cap::Char('b'));
        kb.step(0, 1);
        assert_eq!(kb.under_cursor(), Cap::Space);
        // Backspace's middle is 7.75 units along: the comma's (7.5) is nearest.
        kb.step(1, 0);
        kb.step(0, -1);
        assert_eq!(kb.under_cursor(), Cap::Char(','));
    }

    #[test]
    fn typing_events_are_a_keyboards() {
        let mut kb = opened();
        assert_eq!(kb.press(Cap::Char('h')), vec![Event::Text("h".into())]);
        assert_eq!(kb.press(Cap::Backspace), vec![key_event(Key::Backspace, true), key_event(Key::Backspace, false)]);
        assert_eq!(kb.press(Cap::Enter), vec![key_event(Key::Enter, true), key_event(Key::Enter, false)]);
        // Space: the key and its text, as the window sends it.
        assert_eq!(kb.press(Cap::Space), vec![key_event(Key::Space, true), key_event(Key::Space, false), Event::Text(" ".into())]);
        // No modifiers on any key event (the Ctrl / Alt shortcuts match on them).
        for ev in kb.press(Cap::Enter) {
            if let Event::Key { modifiers, .. } = ev {
                assert_eq!(modifiers, Modifiers::NONE);
            }
        }
    }

    #[test]
    fn shift_is_one_shot_and_pages_turn() {
        let mut kb = opened();
        assert!(kb.press(Cap::Shift).is_empty());
        assert_eq!(kb.page(), Page::Upper);
        assert_eq!(kb.under_cursor(), Cap::Char('Q'));
        assert_eq!(kb.press(Cap::Char('Q')), vec![Event::Text("Q".into())]);
        assert_eq!(kb.page(), Page::Lower);
        // Shift twice cancels; the symbols key toggles.
        kb.press(Cap::Shift);
        kb.press(Cap::Shift);
        assert_eq!(kb.page(), Page::Lower);
        kb.press(Cap::Symbols);
        assert_eq!(kb.page(), Page::Symbols);
        // A symbol does not drop the page (only the shift is one-shot).
        kb.press(Cap::Char('!'));
        assert_eq!(kb.page(), Page::Symbols);
        kb.press(Cap::Symbols);
        assert_eq!(kb.page(), Page::Lower);
        // X cycles all three and comes round; the cursor keeps its place.
        let seen: Vec<Page> = (0..3).map(|_| { kb.next_page(); kb.page() }).collect();
        assert_eq!(seen, [Page::Upper, Page::Symbols, Page::Lower]);
        assert_eq!(kb.under_cursor(), Cap::Char('q'));
    }

    fn down(key: Key) -> Event {
        key_event(key, true)
    }

    /// The pad's keys (gamepad::osk_keys): d-pad moves, A types, B backspace, Y space, X page, Start Enter, Back
    /// closes.
    #[test]
    fn pad_buttons() {
        let mut kb = opened();
        assert!(kb.pad_event(&down(Key::ArrowDown)).is_empty());
        assert_eq!(kb.pad_event(&down(Key::Enter)), vec![Event::Text("a".into())]);
        assert_eq!(kb.pad_event(&down(Key::Backspace)), vec![key_event(Key::Backspace, true), key_event(Key::Backspace, false)]);
        assert_eq!(kb.pad_event(&down(Key::Space)).len(), 3);
        assert_eq!(kb.pad_event(&down(Key::F2)), vec![key_event(Key::Enter, true), key_event(Key::Enter, false)]);
        assert!(kb.pad_event(&down(Key::Tab)).is_empty());
        assert_eq!(kb.page(), Page::Upper);
        assert_eq!(kb.pad_event(&down(Key::Enter)), vec![Event::Text("A".into())]);
        // Releases are dropped.
        assert!(kb.pad_event(&key_event(Key::Enter, false)).is_empty());
        assert!(kb.is_open());
        assert!(kb.pad_event(&down(Key::Escape)).is_empty());
        assert!(!kb.is_open());
        // Closed: nothing acts.
        assert!(kb.pad_event(&down(Key::Enter)).is_empty());
    }

    #[test]
    fn opening_resets_and_closing_clears() {
        let mut kb = Keyboard::default();
        kb.open(Field::Login);
        kb.press(Cap::Symbols);
        kb.step(1, 1);
        kb.close();
        assert!(!kb.is_open() && kb.rect().is_none() && !kb.hovered());
        kb.open(Field::Name);
        assert_eq!((kb.page(), kb.under_cursor()), (Page::Lower, Cap::Char('q')));
    }

    #[test]
    fn raised_once_per_opening_and_only_for_the_pad() {
        let mut e = Edge::default();
        assert!(!e.update(false, true));
        assert!(e.update(true, true));
        // Still open: not again, every frame.
        assert!(!e.update(true, true));
        assert!(!e.update(false, true));
        // The keyboard opened it: no on-screen keyboard.
        assert!(!e.update(true, false));
        assert!(!e.update(false, false));
        assert!(e.update(true, true));
    }
}
