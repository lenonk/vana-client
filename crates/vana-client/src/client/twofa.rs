//! The account window over the login form (ours: retail changed the password and logged in through PlayOnline).
//! Change password (ChangePassword) takes the new one twice and, with two-factor sign-in on, a code. Two-factor sign-in:
//! CreateTotp brings the otpauth URI, shown as a QR code with its key; a code from the app (VerifyTotp) switches it on
//! and brings the recovery code. Turn off (RemoveTotp) and a new recovery code (RegenerateRecovery) take the app's code
//! or the recovery code. LSB's auth service checks the form's password on each (src/login/auth_session.cpp).

use super::game_ui::{GREY, GameUi, WHITE, YELLOW};
use super::{UiInput, inside};
use crate::net::auth::{Command, Reply};

const TITLE: &str = "Account";
const LINE: f32 = 16.0;

#[derive(Clone, Copy, PartialEq)]
pub enum Ask {
    Off,
    Recovery,
}

pub enum Step {
    Menu,
    /// The URI's QR modules (dark true, `side` a row) and the key under it.
    Scan {
        qr: Vec<bool>,
        side: usize,
        key: String,
    },
    /// A code to turn it off or to make a new recovery code.
    Code(Ask),
    /// The new password twice, and a code with two-factor sign-in on.
    Password,
    /// The outcome, and a recovery code to keep.
    Done {
        text: &'static str,
        code: Option<String>,
    },
}

pub struct TwoFa {
    pub step: Step,
    pub focus: usize,
    pub code: String,
    /// Change password's new password and its repeat.
    pub new: [String; 2],
    pub error: Option<String>,
}

/// What the window asks of the app.
pub enum Act {
    Send(Command, String),
    /// ChangePassword to `new`, with the code `otp` (empty without two-factor sign-in).
    ChangePassword {
        new: String,
        otp: String,
    },
    Close,
    /// The recovery code to the clipboard.
    Copy(String),
}

/// The window as drawn, for the modern layer (UI pixels): the frame, the text lines (rect, text, colour, centred),
/// the QR code (rect, side, modules), the fields (rect, text as shown; focus 0.. in order), the buttons (rect,
/// caption), the focus.
pub struct View {
    pub frame: [f32; 4],
    pub lines: Vec<([f32; 4], String, [u8; 4], bool)>,
    pub qr: Option<([f32; 4], usize, Vec<bool>)>,
    pub fields: Vec<([f32; 4], String)>,
    pub buttons: Vec<([f32; 4], &'static str)>,
    pub focus: usize,
    pub busy: bool,
}

impl TwoFa {
    pub fn new() -> Self {
        TwoFa { step: Step::Menu, focus: 0, code: String::new(), new: Default::default(), error: None }
    }

    /// The step's fields: their labels, focus 0.. in order (the new passwords are shown as stars).
    fn fields(&self) -> &'static [&'static str] {
        match self.step {
            Step::Scan { .. } | Step::Code(_) => &["Code"],
            Step::Password => &["New password", "Again", "Code"],
            _ => &[],
        }
    }

    fn field(&mut self, i: usize) -> &mut String {
        match (&self.step, i) {
            (Step::Password, 0 | 1) => &mut self.new[i],
            _ => &mut self.code,
        }
    }

    /// Typed or pasted text into the focused field: a code letters and digits, a password plain ASCII; 32 at most. LSB
    /// leaves a password over 32 bytes or with other bytes unanswered.
    fn type_in(&mut self, t: &str) {
        let password = matches!(self.step, Step::Password) && self.focus < 2;
        let keep = |c: &char| if password { (' '..='~').contains(c) } else { c.is_ascii_alphanumeric() };
        let v = self.field(self.focus);
        v.extend(t.chars().filter(keep));
        *v = v.chars().take(32).collect();
    }

    /// Why the step's button `k` may not send yet: the first button needs a code (the password's code is optional) or
    /// a new password given twice.
    fn refused(&self, k: usize) -> Option<&'static str> {
        match (&self.step, k) {
            (Step::Password, 0) if self.new[0].is_empty() => Some("Enter the new password."),
            (Step::Password, 0) if self.new[0] != self.new[1] => Some("The two new passwords are not the same."),
            (Step::Scan { .. } | Step::Code(_), 0) if self.code.is_empty() => Some("Enter a code first."),
            _ => None,
        }
    }

    /// A field as shown.
    fn shown(&self, i: usize) -> String {
        match (&self.step, i) {
            (Step::Password, 0 | 1) => "*".repeat(self.new[i].chars().count()),
            _ => self.code.clone(),
        }
    }

    fn buttons(&self) -> &'static [&'static str] {
        match self.step {
            Step::Menu => {
                &["Change password", "Turn on two-factor", "Turn off two-factor", "New recovery code", "Close"]
            }
            Step::Scan { .. } => &["Confirm", "Cancel"],
            Step::Code(Ask::Off) => &["Turn off", "Cancel"],
            Step::Code(Ask::Recovery) => &["Make a new code", "Cancel"],
            Step::Password => &["Change", "Cancel"],
            Step::Done { code: Some(_), .. } => &["Copy code", "OK"],
            Step::Done { .. } => &["OK"],
        }
    }

    fn text(&self) -> &'static str {
        match &self.step {
            Step::Menu => {
                "Change this account's password, or turn on two-factor sign-in: logging in then also asks for a code from \
                 an authenticator app on your phone."
            }
            Step::Password => {
                "The current password is the login form's. Enter the new one twice, and with two-factor sign-in on, the \
                 code your authenticator app shows now."
            }
            Step::Scan { .. } => {
                "Scan this with an authenticator app (or type in the key under it), then enter the 6-digit code it shows."
            }
            Step::Code(_) => "Enter the code your authenticator app shows now, or your recovery code.",
            Step::Done { text, .. } => text,
        }
    }

    /// The help line while the window is up.
    pub fn help(&self) -> &'static str {
        match (&self.step, self.focus) {
            (Step::Menu, 0) => "Change this account's password (the login form's is the current one).",
            (Step::Menu, 1) => "Turn on two-factor sign-in for this account.",
            (Step::Menu, 2) => "Turn off two-factor sign-in (needs a code or the recovery code).",
            (Step::Menu, 3) => "Replace the recovery code (needs a code or the old recovery code).",
            (Step::Menu, _) => "Back to the login form.",
            (Step::Password, 0) => "The new password: 1 to 32 plain ASCII characters.",
            (Step::Password, 1) => "The new password again.",
            (Step::Password, 2) => "With two-factor sign-in on, the code your app shows; else leave it empty.",
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
                    let key = key
                        .as_bytes()
                        .chunks(4)
                        .map(|c| String::from_utf8_lossy(c).into_owned())
                        .collect::<Vec<_>>()
                        .join(" ");
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
            Ok(Reply::TotpRemoved) => {
                (self.step, self.focus, self.code) =
                    (Step::Done { text: "Two-factor sign-in is off.", code: None }, 0, String::new())
            }
            Ok(Reply::PasswordChanged) => {
                let text = "The password is changed: log in with the new one.";
                (self.step, self.focus, self.code, self.new) =
                    (Step::Done { text, code: None }, 0, String::new(), Default::default())
            }
            // LSB answers every refusal of a change with 0x07.
            Err(e)
                if matches!(self.step, Step::Password)
                    && e.downcast_ref::<crate::net::auth::Refused>().is_some_and(|r| r.0 == 0x07) =>
            {
                self.error = Some(
                    "Not changed: check the password in the login form, and the code if two-factor sign-in is on."
                        .into(),
                )
            }
            Ok(_) => self.error = Some("Unexpected reply from the login server.".into()),
            // LSB answers every refusal with "Failed to validate credentials".
            Err(e) => {
                let e = format!("{e:#}");
                self.error = Some(match &self.step {
                    _ if !e.contains("validate credentials") => e,
                    Step::Menu => {
                        "Refused: check the password. If two-factor sign-in is on already, turn it off first.".into()
                    }
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
    fields: Vec<[f32; 4]>,
    buttons: Vec<[f32; 4]>,
}

/// Rects in UI pixels, `f` the modern layer's 1.5 (its form is half again as large).
fn layout(lu: &GameUi, size: [u32; 2], st: &TwoFa, busy: bool, f: f32) -> Lay {
    let (sw, sh) = (size[0] as f32, size[1] as f32);
    let ww = 420.0 * f;
    let cw = ww - 32.0 * f;
    let mut lines = Vec::new();
    let mut y = 12.0 * f;
    let line =
        |lines: &mut Vec<Line>, y: &mut f32, text: String, colour, centre| push(lines, y, f, cw, text, colour, centre);
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
    let mut fields = Vec::new();
    let labels = st.fields();
    if !labels.is_empty() {
        y += 8.0 * f;
        let lw = labels.iter().map(|l| lu.text_width(l)).fold(60.0, f32::max) + 4.0;
        for l in labels {
            lines.push(([16.0 * f, y + 1.0 * f, lw * f, LINE * f], (*l).into(), WHITE, false));
            fields.push([(20.0 + lw) * f, y, cw - (4.0 + lw) * f, 18.0 * f]);
            y += 22.0 * f;
        }
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
        fields: fields.into_iter().map(at).collect(),
        buttons: buttons.into_iter().map(at).collect(),
    }
}

/// The window over the login form: keys and mouse (none while `busy`), then drawn here (classic) or handed to the
/// modern layer through `modern`. Focus 0.. are the step's fields, then the buttons.
pub fn window(
    lu: &mut GameUi,
    input: &UiInput,
    size: [u32; 2],
    st: &mut TwoFa,
    busy: bool,
    sounds: &mut Vec<crate::audio::Ui>,
    modern: Option<&mut Option<View>>,
) -> Option<Act> {
    let f = if modern.is_some() { 1.5 } else { 1.0 };
    // The modern layer's rule: hovering only lights a button.
    let hover_moves = modern.is_none();
    let lay = layout(lu, size, st, busy, f);
    let first = st.fields().len();
    let items = first + lay.buttons.len();
    let was = st.focus;
    let mut press = None;
    if !busy {
        if let Some(k) = input.pointer.and_then(|p| lay.buttons.iter().position(|r| inside(r, p))) {
            lu.hover.set(true);
            if input.moved && hover_moves {
                st.focus = first + k;
            }
            if input.clicked {
                press = Some(first + k);
            }
        }
        if let Some(i) =
            input.pointer.filter(|_| input.clicked).and_then(|p| lay.fields.iter().position(|r| inside(r, p)))
        {
            st.focus = i;
        }
        for ev in &input.events {
            match ev {
                // Ctrl+C copies the recovery code.
                egui::Event::Copy if matches!(st.step, Step::Done { code: Some(_), .. }) => press = Some(0),
                egui::Event::Text(t) | egui::Event::Paste(t) if st.focus < first => st.type_in(t),
                egui::Event::Key { key, pressed: true, modifiers, .. } => match key {
                    egui::Key::Tab if modifiers.shift => st.focus = (st.focus + items - 1) % items,
                    egui::Key::Tab => st.focus = (st.focus + 1) % items,
                    egui::Key::ArrowDown if first == 0 || st.focus < first => st.focus = (st.focus + 1) % items,
                    egui::Key::ArrowUp if first == 0 => st.focus = (st.focus + items - 1) % items,
                    // Up from the buttons to the last field, then field by field.
                    egui::Key::ArrowUp => {
                        st.focus = if st.focus >= first { first - 1 } else { st.focus.saturating_sub(1) }
                    }
                    egui::Key::ArrowRight if first > 0 && st.focus >= first => {
                        st.focus = first + (st.focus - first + 1) % (items - first)
                    }
                    egui::Key::ArrowLeft if first > 0 && st.focus >= first => {
                        st.focus = first + (st.focus - first + items - first - 1) % (items - first)
                    }
                    egui::Key::Backspace if st.focus < first => {
                        st.field(st.focus).pop();
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
        // The fields' first button: a code needed (the password's code is optional), a new password checked.
        let refused = st.refused(k);
        sounds.push(if refused.is_some() {
            crate::audio::Ui::Error
        } else if k + 1 == lay.buttons.len() && first > 0 {
            crate::audio::Ui::Cancel
        } else {
            crate::audio::Ui::Select
        });
        st.error = None;
        match (&st.step, k) {
            (Step::Menu, 0) => (st.step, st.focus, st.new) = (Step::Password, 0, Default::default()),
            (Step::Menu, 1) => act = Some(Act::Send(Command::CreateTotp, String::new())),
            (Step::Menu, 2) => (st.step, st.focus) = (Step::Code(Ask::Off), 0),
            (Step::Menu, 3) => (st.step, st.focus) = (Step::Code(Ask::Recovery), 0),
            (Step::Done { code: Some(c), .. }, 0) => act = Some(Act::Copy(c.clone())),
            (Step::Menu, _) | (Step::Done { .. }, _) => act = Some(Act::Close),
            _ if refused.is_some() => st.error = refused.map(String::from),
            (Step::Password, 0) => act = Some(Act::ChangePassword { new: st.new[0].clone(), otp: st.code.clone() }),
            (Step::Scan { .. }, 0) => act = Some(Act::Send(Command::VerifyTotp, st.code.clone())),
            (Step::Code(Ask::Off), 0) => act = Some(Act::Send(Command::RemoveTotp, st.code.clone())),
            (Step::Code(Ask::Recovery), 0) => act = Some(Act::Send(Command::RegenerateRecovery, st.code.clone())),
            _ => (st.step, st.focus, st.code, st.new) = (Step::Menu, 0, String::new(), Default::default()),
        }
    } else if st.focus != was {
        sounds.push(crate::audio::Ui::Cursor);
    }
    let lay = layout(lu, size, st, busy, f);
    let first = st.fields().len();
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
            fields: lay.fields.iter().enumerate().map(|(i, r)| (*r, st.shown(i))).collect(),
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
    for (i, r) in lay.fields.iter().enumerate() {
        let (on, text) = (st.focus == i && !busy, st.shown(i));
        lu.rect(r[0], r[1], r[2], r[3], if on { [0.16, 0.2, 0.45, 0.6] } else { [0.0, 0.0, 0.0, 0.35] });
        lu.text(&text, r[0] + 4.0, r[1] + 1.0, 0, WHITE);
        if on && lu.blink() {
            lu.rect(r[0] + 4.0 + lu.text_width(&text), r[1] + 2.0, 2.0, 14.0, [1.0, 1.0, 1.0, 0.9]);
        }
    }
    let mut hand = None;
    for (k, (r, name)) in lay.buttons.iter().zip(names).enumerate() {
        let on = st.focus == first + k && !busy;
        let d = if on { -1.0 } else { 0.0 };
        let tint = if busy {
            0x8040_4040
        } else if on {
            0x8080_5020
        } else {
            0x8080_8080
        };
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::auth::Refused;

    fn password_step() -> TwoFa {
        TwoFa { step: Step::Password, ..TwoFa::new() }
    }

    /// The new password is needed, twice the same; the code is not.
    #[test]
    fn password_checks() {
        let mut t = password_step();
        assert_eq!(t.refused(0), Some("Enter the new password."));
        t.new = ["hunter22".into(), "hunter2".into()];
        assert_eq!(t.refused(0), Some("The two new passwords are not the same."));
        t.new[1] = "hunter22".into();
        assert_eq!(t.refused(0), None);
        // Cancel always goes; a two-factor code step needs its code.
        assert_eq!(password_step().refused(1), None);
        let code = TwoFa { step: Step::Code(Ask::Off), ..TwoFa::new() };
        assert_eq!(code.refused(0), Some("Enter a code first."));
    }

    /// A password field takes plain ASCII (spaces and symbols too), a code letters and digits; both stop at 32.
    #[test]
    fn typing() {
        let mut t = password_step();
        t.type_in("pa ss!word\u{e9}\u{1}");
        assert_eq!(t.new[0], "pa ss!word");
        t.type_in(&"x".repeat(40));
        assert_eq!(t.new[0].len(), 32);
        t.focus = 2;
        t.type_in("12 3-4ab");
        assert_eq!(t.code, "1234ab");
    }

    /// 0x07 on the password step says what to check; 0x06 ends it with the new passwords gone.
    #[test]
    fn password_replies() {
        let mut t = password_step();
        t.reply(Err(Refused(0x07).into()));
        assert!(t.error.as_deref().is_some_and(|e| e.starts_with("Not changed:")));
        t.new = ["hunter22".into(), "hunter22".into()];
        t.reply(Ok(Reply::PasswordChanged));
        assert!(
            matches!(t.step, Step::Done { code: None, .. })
                && t.error.is_none()
                && t.new == [String::new(), String::new()]
        );
        // Elsewhere 0x07 is the server's line.
        let mut menu = TwoFa::new();
        menu.reply(Err(Refused(0x07).into()));
        assert!(menu.error.as_deref().is_some_and(|e| e.contains("code 0x07")));
    }
}
