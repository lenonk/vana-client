//! Two-factor sign-in (ours: retail logged in through PlayOnline), a window over the login form. Turn on: CreateTotp
//! brings the otpauth URI, shown as a QR code with its key; a code from the app (VerifyTotp) switches it on and brings
//! the recovery code. Turn off (RemoveTotp) and a new recovery code (RegenerateRecovery) take the app's code or the
//! recovery code. LSB's auth service checks the password on each (src/login/auth_session.cpp).

use super::game_ui::{GREY, GameUi, WHITE, YELLOW};
use super::{UiInput, inside};
use crate::net::auth::{Command, Reply};

const TITLE: &str = "Two-factor sign-in";
const LINE: f32 = 16.0;

#[derive(Clone, Copy, PartialEq)]
pub enum Ask {
    Off,
    Recovery,
}

pub enum Step {
    Menu,
    /// The URI's QR modules (dark true, `side` a row) and the key under it.
    Scan { qr: Vec<bool>, side: usize, key: String },
    /// A code to turn it off or to make a new recovery code.
    Code(Ask),
    /// The outcome, and a recovery code to keep.
    Done { text: &'static str, code: Option<String> },
}

pub struct TwoFa {
    pub step: Step,
    pub focus: usize,
    pub code: String,
    pub error: Option<String>,
}

/// What the window asks of the app.
pub enum Act {
    Send(Command, String),
    Close,
}

/// The window as drawn, for the modern layer (UI pixels): the frame, the text lines (rect, text, colour, centred),
/// the QR code (rect, side, modules), the code field (rect, text), the buttons (rect, caption), the focus.
pub struct View {
    pub frame: [f32; 4],
    pub lines: Vec<([f32; 4], String, [u8; 4], bool)>,
    pub qr: Option<([f32; 4], usize, Vec<bool>)>,
    pub field: Option<([f32; 4], String)>,
    pub buttons: Vec<([f32; 4], &'static str)>,
    pub focus: usize,
    pub busy: bool,
}

impl TwoFa {
    pub fn new() -> Self {
        TwoFa { step: Step::Menu, focus: 0, code: String::new(), error: None }
    }

    fn has_field(&self) -> bool {
        matches!(self.step, Step::Scan { .. } | Step::Code(_))
    }

    fn buttons(&self) -> &'static [&'static str] {
        match self.step {
            Step::Menu => &["Turn on", "Turn off", "New recovery code", "Close"],
            Step::Scan { .. } => &["Confirm", "Cancel"],
            Step::Code(Ask::Off) => &["Turn off", "Cancel"],
            Step::Code(Ask::Recovery) => &["Make a new code", "Cancel"],
            Step::Done { .. } => &["OK"],
        }
    }

    fn text(&self) -> &'static str {
        match &self.step {
            Step::Menu => "With two-factor sign-in on, logging in also asks for a code from an authenticator app on your phone.",
            Step::Scan { .. } => "Scan this with an authenticator app (or type in the key under it), then enter the 6-digit code it shows.",
            Step::Code(_) => "Enter the code your authenticator app shows now, or your recovery code.",
            Step::Done { text, .. } => text,
        }
    }

    /// The help line while the window is up.
    pub fn help(&self) -> &'static str {
        match (&self.step, self.focus) {
            (Step::Menu, 0) => "Turn on two-factor sign-in for this account.",
            (Step::Menu, 1) => "Turn off two-factor sign-in (needs a code or the recovery code).",
            (Step::Menu, 2) => "Replace the recovery code (needs a code or the old recovery code).",
            (Step::Menu, _) => "Back to the login form.",
            (Step::Scan { .. } | Step::Code(_), 0) => "Type the code.",
            (Step::Done { code: Some(_), .. }, _) => "Write the recovery code down before closing this window.",
            _ => "",
        }
    }

    /// The server's answer to the last request.
    pub fn reply(&mut self, r: anyhow::Result<Reply>) {
        self.error = None;
        match r {
            Ok(Reply::TotpCreated { uri }) => match qr(&uri) {
                Some((qr, side)) => {
                    let key = uri.split(['?', '&']).find_map(|p| p.strip_prefix("secret=")).unwrap_or_default();
                    let key = key.as_bytes().chunks(4).map(|c| String::from_utf8_lossy(c).into_owned()).collect::<Vec<_>>().join(" ");
                    (self.step, self.focus, self.code) = (Step::Scan { qr, side, key }, 0, String::new());
                }
                None => self.error = Some("The server's key could not be shown as a QR code.".into()),
            },
            Ok(Reply::TotpVerified { recovery_code }) => {
                let text = if matches!(self.step, Step::Scan { .. }) {
                    "Two-factor sign-in is on: enter the app's code in One-time code when you log in. Keep this recovery code somewhere safe; it turns two-factor sign-in off if you lose the app."
                } else {
                    "Your new recovery code (the old one no longer works):"
                };
                (self.step, self.focus, self.code) = (Step::Done { text, code: recovery_code }, 0, String::new());
            }
            Ok(Reply::TotpRemoved) => (self.step, self.focus, self.code) = (Step::Done { text: "Two-factor sign-in is off.", code: None }, 0, String::new()),
            Ok(_) => self.error = Some("Unexpected reply from the login server.".into()),
            // LSB answers every refusal with "Failed to validate credentials".
            Err(e) => {
                let e = format!("{e:#}");
                self.error = Some(match &self.step {
                    _ if !e.contains("validate credentials") => e,
                    Step::Menu => "Refused: check the password. If two-factor sign-in is on already, turn it off first.".into(),
                    _ => "That code was not accepted (or the password is wrong).".into(),
                });
            }
        }
    }
}

/// The URI as QR modules (error correction M) and their side.
fn qr(uri: &str) -> Option<(Vec<bool>, usize)> {
    let code = qrcode::QrCode::with_error_correction_level(uri, qrcode::EcLevel::M).ok()?;
    Some((code.to_colors().into_iter().map(|c| c == qrcode::Color::Dark).collect(), code.width()))
}

type Line = ([f32; 4], String, [u8; 4], bool);

/// A text line at `y` (window-relative), then `y` below it.
fn push(lines: &mut Vec<Line>, y: &mut f32, f: f32, cw: f32, text: String, colour: [u8; 4], centre: bool) {
    lines.push(([16.0 * f, *y, cw, LINE * f], text, colour, centre));
    *y += LINE * f;
}

struct Lay {
    frame: [f32; 4],
    lines: Vec<Line>,
    qr: Option<[f32; 4]>,
    field: Option<[f32; 4]>,
    buttons: Vec<[f32; 4]>,
}

/// Rects in UI pixels, `f` the modern layer's 1.5 (its form is half again as large).
fn layout(lu: &GameUi, size: [u32; 2], st: &TwoFa, busy: bool, f: f32) -> Lay {
    let (sw, sh) = (size[0] as f32, size[1] as f32);
    let ww = 420.0 * f;
    let cw = ww - 32.0 * f;
    let mut lines = Vec::new();
    let mut y = 12.0 * f;
    let line = |lines: &mut Vec<Line>, y: &mut f32, text: String, colour, centre| push(lines, y, f, cw, text, colour, centre);
    line(&mut lines, &mut y, TITLE.into(), YELLOW, true);
    y += 4.0 * f;
    for l in lu.wrap(st.text(), cw / f) {
        line(&mut lines, &mut y, l, WHITE, false);
    }
    let mut qr = None;
    if let Step::Scan { side, key, .. } = &st.step {
        y += 6.0 * f;
        let m = (3.0 * f).floor();
        let q = (*side as f32 + 8.0) * m;
        qr = Some([(ww - q) / 2.0, y, q, q]);
        y += q + 4.0 * f;
        line(&mut lines, &mut y, key.clone(), WHITE, true);
    }
    if let Step::Done { code: Some(c), .. } = &st.step {
        y += 6.0 * f;
        line(&mut lines, &mut y, c.clone(), YELLOW, true);
    }
    let mut field = None;
    if st.has_field() {
        y += 8.0 * f;
        lines.push(([16.0 * f, y + 1.0 * f, 60.0 * f, LINE * f], "Code".into(), WHITE, false));
        field = Some([80.0 * f, y, cw - 64.0 * f, 18.0 * f]);
        y += 22.0 * f;
    }
    if let Some(e) = &st.error {
        y += 4.0 * f;
        for l in lu.wrap(e, cw / f) {
            line(&mut lines, &mut y, l, YELLOW, false);
        }
    } else if busy {
        y += 4.0 * f;
        line(&mut lines, &mut y, "Asking the login server...".into(), GREY, false);
    }
    y += 10.0 * f;
    let names = st.buttons();
    let widths: Vec<f32> = names.iter().map(|n| ((lu.label_width(n) + 14.0).max(88.0) * f).round()).collect();
    let mut buttons = Vec::new();
    if matches!(st.step, Step::Menu) {
        // A column, as a menu.
        let w = widths.iter().copied().fold(0.0, f32::max);
        for _ in names {
            buttons.push([(ww - w) / 2.0, y, w, 16.0 * f]);
            y += 22.0 * f;
        }
        y -= 6.0 * f;
    } else {
        let mut x = (ww - widths.iter().sum::<f32>() - 24.0 * f * (names.len() - 1) as f32) / 2.0;
        for w in &widths {
            buttons.push([x, y, *w, 16.0 * f]);
            x += w + 24.0 * f;
        }
        y += 16.0 * f;
    }
    let wh = y + 14.0 * f;
    let (wx, wy) = (((sw - ww) / 2.0).round(), ((sh - wh) / 2.0).round());
    let at = |r: [f32; 4]| [wx + r[0], wy + r[1], r[2], r[3]];
    Lay {
        frame: [wx, wy, ww, wh],
        lines: lines.into_iter().map(|(r, t, c, centre)| (at(r), t, c, centre)).collect(),
        qr: qr.map(at),
        field: field.map(at),
        buttons: buttons.into_iter().map(at).collect(),
    }
}

/// The window over the login form: keys and mouse (none while `busy`), then drawn here (classic) or handed to the
/// modern layer through `modern`. Focus 0 is the code field where there is one, then the buttons.
#[allow(clippy::too_many_arguments)]
pub fn window(lu: &mut GameUi, input: &UiInput, size: [u32; 2], st: &mut TwoFa, busy: bool, sounds: &mut Vec<crate::audio::Ui>, modern: Option<&mut Option<View>>) -> Option<Act> {
    let f = if modern.is_some() { 1.5 } else { 1.0 };
    let lay = layout(lu, size, st, busy, f);
    let first = st.has_field() as usize;
    let items = first + lay.buttons.len();
    let was = st.focus;
    let mut press = None;
    if !busy {
        if let Some(k) = input.pointer.and_then(|p| lay.buttons.iter().position(|r| inside(r, p))) {
            lu.hover.set(true);
            if input.moved {
                st.focus = first + k;
            }
            if input.clicked {
                press = Some(first + k);
            }
        }
        if input.clicked && lay.field.is_some_and(|r| input.pointer.is_some_and(|p| inside(&r, p))) {
            st.focus = 0;
        }
        for ev in &input.events {
            match ev {
                egui::Event::Text(t) | egui::Event::Paste(t) if first == 1 && st.focus == 0 => {
                    st.code.extend(t.chars().filter(|c| c.is_ascii_alphanumeric()));
                    st.code = st.code.chars().take(32).collect();
                }
                egui::Event::Key { key, pressed: true, modifiers, .. } => match key {
                    egui::Key::Tab if modifiers.shift => st.focus = (st.focus + items - 1) % items,
                    egui::Key::Tab => st.focus = (st.focus + 1) % items,
                    egui::Key::ArrowDown if first == 0 || st.focus == 0 => st.focus = (st.focus + 1) % items,
                    egui::Key::ArrowUp if first == 0 => st.focus = (st.focus + items - 1) % items,
                    egui::Key::ArrowUp => st.focus = 0,
                    egui::Key::ArrowRight if first == 1 && st.focus > 0 => st.focus = (st.focus % (items - 1)) + 1,
                    egui::Key::ArrowLeft if first == 1 && st.focus > 0 => st.focus = if st.focus == 1 { items - 1 } else { st.focus - 1 },
                    egui::Key::Backspace if first == 1 && st.focus == 0 => {
                        st.code.pop();
                    }
                    // The field's Enter is the first button's.
                    egui::Key::Enter => press = Some(st.focus.max(first)),
                    egui::Key::Escape => press = Some(items - 1),
                    _ => {}
                },
                _ => {}
            }
        }
    }
    let mut act = None;
    if let Some(i) = press {
        st.focus = i;
        let k = i - first;
        let needs_code = first == 1 && k == 0 && st.code.is_empty();
        sounds.push(if needs_code { crate::audio::Ui::Error } else if k + 1 == lay.buttons.len() && first == 1 { crate::audio::Ui::Cancel } else { crate::audio::Ui::Select });
        st.error = None;
        match (&st.step, k) {
            (Step::Menu, 0) => act = Some(Act::Send(Command::CreateTotp, String::new())),
            (Step::Menu, 1) => (st.step, st.focus) = (Step::Code(Ask::Off), 0),
            (Step::Menu, 2) => (st.step, st.focus) = (Step::Code(Ask::Recovery), 0),
            (Step::Menu, _) | (Step::Done { .. }, _) => act = Some(Act::Close),
            _ if needs_code => st.error = Some("Enter a code first.".into()),
            (Step::Scan { .. }, 0) => act = Some(Act::Send(Command::VerifyTotp, st.code.clone())),
            (Step::Code(Ask::Off), 0) => act = Some(Act::Send(Command::RemoveTotp, st.code.clone())),
            (Step::Code(Ask::Recovery), 0) => act = Some(Act::Send(Command::RegenerateRecovery, st.code.clone())),
            _ => (st.step, st.focus, st.code) = (Step::Menu, 0, String::new()),
        }
    } else if st.focus != was {
        sounds.push(crate::audio::Ui::Cursor);
    }
    let lay = layout(lu, size, st, busy, f);
    let first = st.has_field() as usize;
    let names = st.buttons();
    if let Some(out) = modern {
        let qr = match (&st.step, lay.qr) {
            (Step::Scan { qr, side, .. }, Some(r)) => Some((r, *side, qr.clone())),
            _ => None,
        };
        *out = Some(View {
            frame: lay.frame,
            lines: lay.lines,
            qr,
            field: lay.field.map(|r| (r, st.code.clone())),
            buttons: lay.buttons.iter().zip(names).map(|(r, n)| (*r, *n)).collect(),
            focus: st.focus,
            busy,
        });
        return act;
    }
    let [wx, wy, ww, wh] = lay.frame;
    // Opaque under the translucent frame: the logo and the form would show through.
    lu.rect(wx + 2.0, wy + 2.0, ww - 4.0, wh - 4.0, [0.02, 0.02, 0.06, 0.9]);
    lu.frame(wx, wy, ww, wh);
    for (r, text, colour, centre) in &lay.lines {
        let x = if *centre { r[0] + ((r[2] - lu.text_width(text)) / 2.0).round() } else { r[0] };
        lu.text(text, x, r[1], 0, *colour);
    }
    if let (Step::Scan { qr, side, .. }, Some(r)) = (&st.step, lay.qr) {
        lu.rect(r[0], r[1], r[2], r[3], [1.0, 1.0, 1.0, 1.0]);
        let m = r[2] / (*side as f32 + 8.0);
        for (i, _) in qr.iter().enumerate().filter(|(_, d)| **d) {
            let (x, y) = ((i % side) as f32, (i / side) as f32);
            lu.rect(r[0] + (x + 4.0) * m, r[1] + (y + 4.0) * m, m, m, [0.0, 0.0, 0.0, 1.0]);
        }
    }
    if let Some(r) = lay.field {
        let on = st.focus == 0 && !busy;
        lu.rect(r[0], r[1], r[2], r[3], if on { [0.16, 0.2, 0.45, 0.6] } else { [0.0, 0.0, 0.0, 0.35] });
        lu.text(&st.code, r[0] + 4.0, r[1] + 1.0, 0, WHITE);
        if on && lu.blink() {
            lu.rect(r[0] + 4.0 + lu.text_width(&st.code), r[1] + 2.0, 2.0, 14.0, [1.0, 1.0, 1.0, 0.9]);
        }
    }
    let mut hand = None;
    for (k, (r, name)) in lay.buttons.iter().zip(names).enumerate() {
        let on = st.focus == first + k && !busy;
        let d = if on { -1.0 } else { 0.0 };
        let tint = if busy { 0x8040_4040 } else if on { 0x8080_5020 } else { 0x8080_8080 };
        super::settings::button(lu, [r[0] + d, r[1] + d, r[2], r[3]], tint);
        let lx = r[0] + ((r[2] - lu.label_width(name)) / 2.0).round();
        lu.label(name, lx + d, r[1] + 1.0 + d, tint, true);
        if on {
            hand = Some((r[0] + d, r[1] + d));
        }
    }
    if let Some((hx, hy)) = hand {
        lu.cursor(hx, hy);
    }
    act
}
