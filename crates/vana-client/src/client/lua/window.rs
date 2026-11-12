//! Mod windows (docs/lua-mods.md 4.10): a mod describes a window as elements; this module keeps its cursor and keys
//! (the same in both UIs) and draws it in the classic UI as the Config pages are drawn. The modern UI draws the same
//! model with the kit (modern/modwin.rs).

use super::super::game_ui::GameUi;
use crate::audio::Ui as Se;

/// An icon as mods name it ({kind = "item" | "status" | "spell" | "ability", id}): an item's, a status's, or a
/// spell's or an ability's (weapon skills too) as the magic and ability lists draw it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Icon {
    Item(u16),
    Status(u16),
    Spell(u16),
    Ability(u16),
}

impl Icon {
    /// By kind name; no kind (or an unknown one) is an item's.
    pub fn of(kind: Option<&str>, id: u16) -> Icon {
        match kind {
            Some("status") => Icon::Status(id),
            Some("spell") => Icon::Spell(id),
            Some("ability" | "weaponskill" | "weapon_skill") => Icon::Ability(id),
            _ => Icon::Item(id),
        }
    }

    pub fn kind(self) -> &'static str {
        match self {
            Icon::Item(_) => "item",
            Icon::Status(_) => "status",
            Icon::Spell(_) => "spell",
            Icon::Ability(_) => "ability",
        }
    }

    pub fn id(self) -> u16 {
        match self {
            Icon::Item(id) | Icon::Status(id) | Icon::Spell(id) | Icon::Ability(id) => id,
        }
    }

    /// A spell's or an ability's list row: its list and record index.
    fn row(self, w: &super::World) -> Option<(super::super::magic::Kind, u16)> {
        use super::super::magic::Kind;
        match self {
            Icon::Spell(id) => Some((Kind::Magic, id)),
            Icon::Ability(id) => {
                // The list takes a record's index; ids and indexes match, but look it up if one does not.
                let acts = w.actions.as_ref()?;
                let k = if acts.abilities.get(id as usize).is_some_and(|a| a.id == id) {
                    id
                } else {
                    acts.abilities.iter().position(|a| a.id == id)? as u16
                };
                Some((Kind::Ability, k))
            }
            _ => None,
        }
    }

    /// A spell's or an ability's list sprite (library, index).
    pub fn sprite(self, w: &super::World) -> Option<(&'static str, usize)> {
        let (kind, k) = self.row(w)?;
        super::super::magic::row_icon(w, kind, k)
    }

    /// A spell's or an ability's list picture, which picks the modern UI's art.
    pub fn picture(self, w: &super::World) -> Option<u16> {
        let (kind, k) = self.row(w)?;
        super::super::magic::picture(w, kind, k)
    }
}

/// A list row: its cells (one without columns), an icon before them, a colour.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    pub cells: Vec<String>,
    pub icon: Option<Icon>,
    pub colour: Option<[u8; 4]>,
    /// The help shown while the cursor or the pointer is on the row (a spell's description).
    pub tip: String,
}

impl Row {
    pub fn text(s: &str) -> Self {
        Row { cells: vec![s.to_string()], ..Default::default() }
    }
}

/// A timers element's group: a heading (none: the rows alone) and its rows in the mod's order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TimerGroup {
    pub name: Option<String>,
    pub rows: Vec<TimerRow>,
}

/// A timer row: seconds left when given (at or below 0: dimmed, waiting), the whole length (0: no gauge), and whether
/// the gauge fills toward ready (recasts) or drains.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TimerRow {
    pub icon: Option<Icon>,
    pub name: String,
    pub left: f32,
    pub total: f32,
    pub fills: bool,
}

/// A lines element's colour: the UI's text, muted, gold or bronze, or a custom one.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Tone {
    #[default]
    Text,
    Muted,
    Gold,
    Bronze,
    Rgba([u8; 4]),
}

impl Tone {
    pub fn of(name: &str) -> Option<Tone> {
        Some(match name {
            "text" => Tone::Text,
            "muted" => Tone::Muted,
            "gold" => Tone::Gold,
            "bronze" => Tone::Bronze,
            _ => return None,
        })
    }
}

/// A run of text in a line: its tone, the strong face (a size up), and element icons after it (0 fire .. 7 dark).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Span {
    pub text: String,
    pub tone: Tone,
    pub strong: bool,
    pub elements: Vec<u8>,
}

/// A line of a lines element. One span without icons wraps; several (or icons) make one row, with `right` at its
/// right end; `column` starts the spans after the first that far in. A `bar` is a thin gauge instead of text; a
/// `fold` line is a header with a chevron (`open`) whose click sends `fold`; a `heading` is a label (`big`: a size up).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextLine {
    pub spans: Vec<Span>,
    pub right: Option<Span>,
    pub column: f32,
    pub indent: f32,
    pub bar: Option<(f32, Tone)>,
    pub fold: Option<String>,
    pub open: bool,
    pub heading: bool,
    pub big: bool,
    /// Space after it.
    pub gap: f32,
}

impl TextLine {
    /// The line as plain text (the classic UI's one-colour lines): its spans, then `right`.
    pub fn plain(&self) -> String {
        let mut t: Vec<&str> = self.spans.iter().map(|s| s.text.as_str()).filter(|t| !t.is_empty()).collect();
        if let Some(r) = &self.right {
            t.push(&r.text);
        }
        t.join(" ")
    }
}

/// A dot on a map element: where (world x / z), its colour, ringed as the target, and the tooltip naming it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MapDot {
    pub x: f32,
    pub z: f32,
    pub colour: [u8; 3],
    pub target: bool,
    pub tip: String,
}

/// A list column: its heading, width (0: a share of what is left) and alignment.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Col {
    pub title: String,
    pub width: f32,
    pub right: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Elem {
    /// `id` (may be empty) lets ui.update find it.
    Text {
        id: String,
        text: String,
        dim: bool,
        colour: Option<[u8; 4]>,
    },
    /// A row of buttons under a heading; `marked` makes them options (the chosen one marked, Enter sets it).
    Buttons {
        id: String,
        label: String,
        items: Vec<String>,
        marked: Option<usize>,
    },
    Slider {
        id: String,
        label: String,
        n: usize,
        at: usize,
        shown: String,
    },
    /// Rows with a cursor; `height` rows show at once; columns give them headings and cells.
    List {
        id: String,
        rows: Vec<Row>,
        columns: Vec<Col>,
        at: usize,
        top: usize,
        height: usize,
    },
    /// A bar filled `value` (0..1) with its label and text; `kind` is the art pack's gauge (hp, mp, tp, cast,
    /// experience, recast).
    /// `row`: a list row (label, the text at the right, a thin bar under; `dim` when done; a value below 0: no bar
    /// known), as the training regime's rows.
    Gauge {
        id: String,
        label: String,
        value: f32,
        text: String,
        kind: String,
        row: bool,
        dim: bool,
    },
    Separator,
    /// Pages: elements with a `page` show only on theirs (of the tabs above them); Left / Right turn it.
    Tabs {
        id: String,
        items: Vec<String>,
        at: usize,
    },
    /// A line the player types: Enter starts and ends it, Escape undoes it.
    /// `filter`: typing goes into it while the window has the focus, whatever its cursor is on (a list's filter).
    /// `key` (`type = "key"`): a key combination ("ctrl+f1"); Enter or a click listens for the next key press,
    /// Escape stops, Backspace or Delete clears it.
    Input {
        id: String,
        label: String,
        text: String,
        max: usize,
        filter: bool,
        key: bool,
    },
    /// One of `items`, turned with Left / Right. `drop` (`type = "dropdown"`): Enter or a click opens the items in a
    /// list under it (Up / Down, Enter or a click picks, Escape or a click elsewhere closes).
    Choice {
        id: String,
        label: String,
        items: Vec<String>,
        at: usize,
        drop: bool,
    },
    /// An RGBA colour, edited a channel at a time after Enter.
    Colour {
        id: String,
        label: String,
        value: [u8; 4],
        alpha: bool,
    },
    /// A heading that folds what is under it (to the next section or tabs).
    Section {
        id: String,
        label: String,
        open: bool,
    },
    /// Timer rows in groups, drawn as the Timers' sections (0 recasts: rows; 1 buffs, 2 debuffs: icon cells by group).
    /// Each row counts down from `at`, when the mod gave it.
    Timers {
        id: String,
        section: usize,
        groups: Vec<TimerGroup>,
        at: std::time::Instant,
    },
    /// A region `w` x `h` the mod draws in (local units: classic pixels, modern points); a click sends where.
    Canvas {
        id: String,
        w: f32,
        h: f32,
        cmds: Vec<Cmd>,
    },
    /// The zone's map around us in the minimap panel's window (the client's map, our marker, Wide Scan's marks; the
    /// zoom is the client option minimap_span): the mod's dots; the wheel over it sends `wheel`.
    Map {
        id: String,
        dots: Vec<MapDot>,
    },
    /// Lines of text in tones, with element icons, bars and fold headers (the skillchain helper's view); a fold
    /// header's click sends `fold` with its key.
    Lines {
        id: String,
        lines: Vec<TextLine>,
    },
    /// Rows naming entities (a label, the entity's name in its colour, its HP), drawn as the focus panel's; a click
    /// targets the entity. Modern UI only.
    Targets {
        id: String,
        rows: Vec<TargetRow>,
    },
    /// A status line as the pending-action chip's: a dot, the text, a note at the right in the tone's colour, and a
    /// detail line under it. Tone 0 waiting, 1 refused, 2 quiet. Modern UI only.
    Chip {
        id: String,
        text: String,
        note: String,
        tone: u8,
        detail: String,
    },
}

/// A `targets` row: the entity by server id, a short label before its name, the tag that names it in commands.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TargetRow {
    pub entity: u32,
    pub label: String,
    pub tag: String,
}

/// A canvas drawing command, in the canvas's units.
#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    Line {
        a: [f32; 2],
        b: [f32; 2],
        width: f32,
        colour: [u8; 4],
    },
    /// Filled (from `colour` at the top to `to` at the bottom when given), or outlined `line` wide; corners rounded
    /// `round` (not with `to`).
    Rect {
        r: [f32; 4],
        colour: [u8; 4],
        to: Option<[u8; 4]>,
        line: Option<f32>,
        round: f32,
    },
    Circle {
        c: [f32; 2],
        radius: f32,
        colour: [u8; 4],
        line: Option<f32>,
    },
    Tri {
        pts: [[f32; 2]; 3],
        colour: [u8; 4],
    },
    /// `size` times the UI's small text; `align` 0 left, 1 centre, 2 right of `at`; an outline round the letters.
    Text {
        at: [f32; 2],
        text: String,
        colour: [u8; 4],
        size: f32,
        align: u8,
        outline: Option<[u8; 4]>,
        bold: bool,
    },
    /// Not drawn: the tooltip shown while the pointer rests inside `r`.
    Tip {
        r: [f32; 4],
        title: String,
        text: String,
    },
    /// Tinted by `tint` (its alpha fades it).
    Icon {
        at: [f32; 2],
        size: f32,
        icon: Icon,
        tint: [u8; 4],
    },
}

/// What every element may carry: the help it shows under the cursor, the page it is on.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Meta {
    pub tip: String,
    pub page: Option<usize>,
}

impl Elem {
    pub fn stop(&self) -> bool {
        !matches!(
            self,
            Elem::Text { .. }
                | Elem::Gauge { .. }
                | Elem::Separator
                | Elem::Canvas { .. }
                | Elem::Timers { .. }
                | Elem::Lines { .. }
                | Elem::Map { .. }
                | Elem::Targets { .. }
                | Elem::Chip { .. }
        )
    }

    pub fn id(&self) -> Option<&str> {
        match self {
            Elem::Buttons { id, .. }
            | Elem::Slider { id, .. }
            | Elem::List { id, .. }
            | Elem::Tabs { id, .. }
            | Elem::Input { id, .. }
            | Elem::Choice { id, .. }
            | Elem::Colour { id, .. }
            | Elem::Section { id, .. }
            | Elem::Canvas { id, .. } => Some(id),
            Elem::Text { id, .. }
            | Elem::Gauge { id, .. }
            | Elem::Timers { id, .. }
            | Elem::Lines { id, .. }
            | Elem::Map { id, .. }
            | Elem::Targets { id, .. }
            | Elem::Chip { id, .. }
                if !id.is_empty() =>
            {
                Some(id)
            }
            _ => None,
        }
    }
}

/// A colour channel's 32 slider steps.
pub const STEPS: usize = 32;

pub fn to_step(v: u8) -> usize {
    (v as usize * (STEPS - 1) + 127) / 255
}

pub fn from_step(s: usize) -> u8 {
    ((s.min(STEPS - 1) * 255 + (STEPS - 1) / 2) / (STEPS - 1)) as u8
}

#[derive(Clone, Debug)]
pub struct Window {
    pub id: String,
    pub title: String,
    /// Points in the modern UI, pixels of the menu layout in the classic one.
    pub width: f32,
    pub help: String,
    pub elems: Vec<Elem>,
    /// One per element.
    pub meta: Vec<Meta>,
    pub open: bool,
    pub focus: bool,
    /// The cursor's element (an index into `elems`, a stop), its column, and an element being edited (a slider, a
    /// colour, a line being typed).
    pub at: usize,
    pub col: usize,
    pub edit: bool,
    /// The typed line's caret (chars), the colour's channel, and the element as it was before the edit.
    pub caret: usize,
    pub chan: usize,
    /// An open dropdown's highlighted item.
    pub drop_at: usize,
    undo: Option<Elem>,
    /// A HUD window never takes the keys (a click still acts); drawn without a frame when `framed` is off; at
    /// `place` (classic UI pixels) when given.
    pub hud: bool,
    pub framed: bool,
    pub place: Option<(f32, f32)>,
    /// A modal window keeps the focus while open (centred unless placed); a popup also closes on a click outside
    /// it. `anchor`: its top left at this window pixel (the pointer when it opened).
    pub modal: bool,
    pub popup: bool,
    pub anchor: Option<(f32, f32)>,
    /// The anchor is the popup's bottom left (it opens upward), not its top left.
    pub above: bool,
    /// The modern UI fades it in and grows it up from its bottom edge as it opens, and folds it back as it closes.
    pub animate: bool,
    /// A canvas being dragged: its element and the last place reported; a press on one that has not moved yet (the
    /// classic UI: a press and release in place is a click, a move past 3 units starts the drag).
    pub drag: Option<(usize, f32, f32)>,
    pub press: Option<(usize, f32, f32)>,
    /// The canvas the pointer was last over (classic UI), for its 'leave'.
    pub hovered: Option<usize>,
    /// An edge window (drawn as the Timers and the training regime are): its header line, whether it can be made
    /// wider, and the built-in panel it stands in for (its place, fold, hiding).
    pub edge: bool,
    pub heading: String,
    pub resize: bool,
    /// An edge window that grows and shrinks upward: its bottom stays put (`grow = "up"`).
    pub grow_up: bool,
    /// An edge window as wide as its lines need (`fit`), at least `width`, at most half the screen (most of it on
    /// the top or the bottom edge), as tall as 60% of the screen before it scrolls.
    pub fit: bool,
    /// The edge an edge window stands on, as last told to the mod: "top", "bottom", "left", "right" or "free".
    pub dock: Option<&'static str>,
    pub panel: Option<String>,
}

/// What happened in a window, for the mod's handler.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Press {
        id: String,
        index: usize,
    },
    Set {
        id: String,
        index: usize,
    },
    Select {
        id: String,
        index: usize,
    },
    /// A typed line: as it changes (`done` false) and when Enter ends it.
    Text {
        id: String,
        text: String,
        done: bool,
    },
    Colour {
        id: String,
        value: [u8; 4],
    },
    Toggle {
        id: String,
        open: bool,
    },
    Sort {
        id: String,
        column: usize,
    },
    /// A lines element's fold header clicked: its key.
    Fold {
        id: String,
        key: String,
    },
    /// The wheel over a map element: notches up (zoom in) positive.
    Wheel {
        id: String,
        notches: f32,
    },
    /// A click on a canvas, in its units: `right` for the right button.
    Click {
        id: String,
        x: f32,
        y: f32,
        right: bool,
    },
    /// An edge window's edge changed (or was first known).
    Dock {
        side: &'static str,
    },
    /// The pointer over a canvas (when it moved), or gone off it (`x` / `y` negative).
    Hover {
        id: String,
        x: f32,
        y: f32,
    },
    /// A drag on a canvas: 0 start, 1 a move, 2 the release; where, and the move since the last report; where on
    /// screen in window pixels and the mod's other window whose canvas is under the pointer (modern UI).
    Drag {
        id: String,
        phase: u8,
        x: f32,
        y: f32,
        dx: f32,
        dy: f32,
        screen: Option<(f32, f32)>,
        over: Option<String>,
    },
    /// One of the mod's HUD elements (`from`) dragged and let go over a canvas, at (x, y) in it.
    Drop {
        id: String,
        x: f32,
        y: f32,
        from: String,
    },
    /// One of the mod's HUD elements (`from`) dragged over a canvas, at (x, y) in it; or off it (`x` / `y` negative).
    DragOver {
        id: String,
        x: f32,
        y: f32,
        from: String,
    },
    Close,
}

impl Event {
    pub fn json(&self) -> serde_json::Value {
        use serde_json::json;
        match self {
            Event::Press { id, index } => json!({"kind": "press", "id": id, "index": index + 1}),
            Event::Set { id, index } => json!({"kind": "set", "id": id, "index": index + 1}),
            Event::Select { id, index } => json!({"kind": "select", "id": id, "index": index + 1}),
            Event::Text { id, text, done } => {
                json!({"kind": if *done { "change" } else { "edit" }, "id": id, "text": text})
            }
            Event::Colour { id, value: [r, g, b, a] } => {
                json!({"kind": "change", "id": id, "value": {"r": r, "g": g, "b": b, "a": a}})
            }
            Event::Toggle { id, open } => json!({"kind": "toggle", "id": id, "open": open}),
            Event::Sort { id, column } => json!({"kind": "sort", "id": id, "column": column + 1}),
            Event::Fold { id, key } => json!({"kind": "fold", "id": id, "key": key}),
            Event::Wheel { id, notches } => json!({"kind": "wheel", "id": id, "notches": notches}),
            Event::Click { id, x, y, right } => {
                json!({"kind": "click", "id": id, "x": x.round() as i64, "y": y.round() as i64, "button": if *right { "right" } else { "left" }})
            }
            Event::Dock { side } => json!({"kind": "dock", "side": side}),
            Event::Hover { id, x, y } => {
                let kind = if *x < 0.0 { "leave" } else { "hover" };
                json!({"kind": kind, "id": id, "x": x.round() as i64, "y": y.round() as i64})
            }
            Event::Drag { id, phase, x, y, dx, dy, screen, over } => {
                let mut e = json!({"kind": (["drag_start", "drag", "drag_end"][*phase as usize % 3]), "id": id, "x": x.round() as i64, "y": y.round() as i64, "dx": dx.round() as i64, "dy": dy.round() as i64});
                if let Some((sx, sy)) = screen {
                    e["sx"] = (sx.round() as i64).into();
                    e["sy"] = (sy.round() as i64).into();
                }
                if let Some(w) = over {
                    e["window"] = w.clone().into();
                }
                e
            }
            Event::Drop { id, x, y, from } => {
                json!({"kind": "drop", "id": id, "x": x.round() as i64, "y": y.round() as i64, "from": from})
            }
            Event::DragOver { id, x, y, from } => {
                let kind = if *x < 0.0 { "drag_out" } else { "drag_over" };
                json!({"kind": kind, "id": id, "x": x.round() as i64, "y": y.round() as i64, "from": from})
            }
            Event::Close => json!({"kind": "close"}),
        }
    }
}

/// The classic page's width when none is given (the Config pages').
pub const CLASSIC_W: f32 = 366.0;
const PITCH: f32 = 34.0;
const LINE: f32 = 18.0;
const TOP: f32 = 34.0;
const SLIDER_X: f32 = 88.0;
/// A colour channel's row while it is edited.
const CHAN_H: f32 = 22.0;
const N: u32 = 0x8080_8080;
const ON: u32 = 0x8080_5020;
const DIM: u32 = 0x8060_6060;

/// Columns laid out in `w`: (x offset, width, right-aligned).
pub fn columns(cols: &[Col], w: f32) -> Vec<(f32, f32, bool)> {
    let fixed: f32 = cols.iter().map(|c| c.width).sum();
    let shares = cols.iter().filter(|c| c.width <= 0.0).count().max(1) as f32;
    let share = ((w - fixed) / shares).max(24.0);
    let mut x = 0.0;
    cols.iter()
        .map(|c| {
            let cw = if c.width > 0.0 { c.width } else { share };
            let out = (x, cw, c.right);
            x += cw;
            out
        })
        .collect()
}

pub fn has_header(cols: &[Col]) -> bool {
    cols.iter().any(|c| !c.title.is_empty())
}

/// A classic text colour (0x80 full) from RGBA.
fn tint([r, g, b, a]: [u8; 4]) -> u32 {
    let h = |v: u8| (v as u32).div_ceil(2);
    h(a) << 24 | h(r) << 16 | h(g) << 8 | h(b)
}

/// A tone as the classic UI's text colour (0x80 = 1.0): white, grey, the headings' gold, a darker grey.
pub fn classic_tone(t: Tone) -> u32 {
    match t {
        Tone::Text => N,
        Tone::Muted => 0x8058_5858,
        Tone::Gold => 0x80a0_a080,
        Tone::Bronze => 0x8070_7070,
        Tone::Rgba(c) => tint(c),
    }
}

/// A tone as a bar's vertex colour (0x80 = 1.0, alpha as the gauge fill's).
pub fn classic_bar(t: Tone) -> [u8; 4] {
    match t {
        Tone::Rgba(c) => c,
        Tone::Gold => [0x80, 0x70, 0x20, 0xa9],
        Tone::Muted | Tone::Bronze => [0x40, 0x40, 0x40, 0xa9],
        Tone::Text => [0x80, 0x80, 0x80, 0xa9],
    }
}

/// One line of a lines element on a classic page (or the Timers' dock) at (x, y), `w` wide: a heading in the
/// headings' font, a bar as the Timers' gauge (frame 38 and its fill 97), else its text in its first span's tone.
pub fn draw_classic_line(gu: &mut GameUi, l: &TextLine, x: f32, y: f32, w: f32) {
    if let Some((f, tone)) = l.bar {
        if f > 0.0 {
            gu.sprite_coloured("menu    windowps", 97, x + 4.0, y + 1.0, f.min(1.0), 1.0, classic_bar(tone), 8);
        }
        gu.sprite("menu    windowps", 38, x, y, N);
    } else if l.heading {
        gu.shape_fit(&l.plain(), x, y + 1.0, w, classic_tone(l.spans.first().map_or(Tone::Gold, |s| s.tone)));
    } else {
        let tone = l.spans.first().map_or(Tone::Text, |s| s.tone);
        gu.text_fit(&l.plain(), x + l.indent, y - 3.0, w - l.indent, classic_tone(tone));
    }
}

/// A classic line's height: a bar 10, a heading 16, text 15.
pub fn classic_line_h(l: &TextLine) -> f32 {
    if l.bar.is_some() {
        10.0
    } else if l.heading {
        16.0
    } else {
        15.0
    }
}

impl Window {
    pub fn new(id: &str) -> Self {
        Window {
            id: id.into(),
            title: String::new(),
            width: 0.0,
            help: String::new(),
            elems: Vec::new(),
            meta: Vec::new(),
            open: false,
            focus: false,
            at: 0,
            col: 0,
            edit: false,
            caret: 0,
            chan: 0,
            drop_at: 0,
            undo: None,
            hud: false,
            framed: true,
            place: None,
            modal: false,
            popup: false,
            anchor: None,
            above: false,
            animate: false,
            drag: None,
            press: None,
            edge: false,
            heading: String::new(),
            resize: false,
            grow_up: false,
            fit: false,
            dock: None,
            hovered: None,
            panel: None,
        }
    }

    /// Which elements are hidden: on another page of their tabs, or under a folded section (one on their page, or
    /// any for an element without a page).
    pub fn hidden(&self) -> Vec<bool> {
        let (mut page, mut section): (Option<usize>, Option<(bool, Option<usize>)>) = (None, None);
        self.elems
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let mine = self.meta.get(i).and_then(|m| m.page);
                let off_page = mine.is_some_and(|p| page.is_some_and(|at| at != p));
                match e {
                    Elem::Tabs { at, .. } => {
                        (page, section) = (Some(*at), None);
                        false
                    }
                    // A hidden section hides what is under it too.
                    Elem::Section { open, .. } => {
                        section = Some((!open || off_page, mine));
                        off_page
                    }
                    _ => off_page || section.is_some_and(|(folded, p)| folded && (mine.is_none() || mine == p)),
                }
            })
            .collect()
    }

    fn stops(&self) -> Vec<usize> {
        let hidden = self.hidden();
        self.elems.iter().enumerate().filter(|(i, e)| e.stop() && !hidden[*i]).map(|(i, _)| i).collect()
    }

    /// The help under the cursor: a list row's tip, else its element's, else the window's.
    pub fn help_now(&self) -> &str {
        if let Some(Elem::List { rows, at, .. }) = self.elems.get(self.at)
            && let Some(r) = rows.get(*at).filter(|r| !r.tip.is_empty())
        {
            return &r.tip;
        }
        match self.meta.get(self.at).map(|m| m.tip.as_str()) {
            Some(t) if !t.is_empty() => t,
            _ => &self.help,
        }
    }

    /// New elements (`ui.window` again): the cursor stays on its element, by id, when they still have it; else it
    /// starts on the first stop.
    pub fn set_elems(&mut self, elems: Vec<Elem>, meta: Vec<Meta>) {
        let was = self.elems.get(self.at).and_then(|e| e.id()).map(str::to_string);
        (self.elems, self.meta) = (elems, meta);
        let kept = was.and_then(|id| self.elems.iter().position(|e| e.id() == Some(id.as_str())));
        if kept != Some(self.at) {
            (self.at, self.col, self.edit, self.undo) = (kept.unwrap_or(0), 0, false, None);
        }
        self.settle();
    }

    /// The cursor on a stop (the first when it is not on one), its column on the marked option.
    pub fn settle(&mut self) {
        self.meta.resize(self.elems.len(), Meta::default());
        let stops = self.stops();
        if !stops.contains(&self.at) {
            self.at = stops.first().copied().unwrap_or(0);
            self.col = self.marked_col();
            (self.edit, self.undo) = (false, None);
        }
        match self.elems.get_mut(self.at) {
            Some(Elem::Buttons { items, .. }) => self.col = self.col.min(items.len().saturating_sub(1)),
            Some(Elem::List { rows, at, top, height, .. }) => {
                *at = (*at).min(rows.len().saturating_sub(1));
                *top = (*top).min(*at).max((*at + 1).saturating_sub(*height));
            }
            Some(Elem::Input { text, .. }) => self.caret = self.caret.min(text.chars().count()),
            _ => {}
        }
        for e in &mut self.elems {
            if let Elem::Tabs { items, at, .. } | Elem::Choice { items, at, .. } = e {
                *at = (*at).min(items.len().saturating_sub(1));
            }
        }
    }

    fn marked_col(&self) -> usize {
        match self.elems.get(self.at) {
            Some(Elem::Buttons { marked: Some(m), .. }) => *m,
            _ => 0,
        }
    }

    /// Whether a line is being typed or a key listened for (the window then takes the text and keys).
    pub fn typing(&self) -> bool {
        self.edit && matches!(self.elems.get(self.at), Some(Elem::Input { .. }))
    }

    /// The dropdown open now, if any.
    pub fn drop_open(&self) -> Option<usize> {
        (self.edit && matches!(self.elems.get(self.at), Some(Elem::Choice { drop: true, .. }))).then_some(self.at)
    }

    /// Whether a key element waits for its key.
    pub fn listening(&self) -> bool {
        self.edit && matches!(self.elems.get(self.at), Some(Elem::Input { key: true, .. }))
    }

    /// The listening key element takes the first key press: Escape stops, Backspace / Delete clear, any other key
    /// (named by `name`, with its modifiers) becomes the combination.
    pub fn listen(
        &mut self,
        events: &[egui::Event],
        name: impl Fn(&egui::Event) -> Option<String>,
    ) -> (Vec<Event>, Vec<Se>) {
        let (mut ev, mut se) = (Vec::new(), Vec::new());
        if !self.listening() {
            return (ev, se);
        }
        for e in events {
            let egui::Event::Key { key, pressed: true, .. } = e else { continue };
            let combo = match key {
                egui::Key::Escape => None,
                egui::Key::Backspace | egui::Key::Delete => Some(String::new()),
                _ => match name(e) {
                    Some(c) => Some(c),
                    None => continue,
                },
            };
            (self.edit, self.undo) = (false, None);
            match combo {
                Some(c) => {
                    if let Some(Elem::Input { id, text, .. }) = self.elems.get_mut(self.at) {
                        *text = c;
                        ev.push(Event::Text { id: id.clone(), text: text.clone(), done: true });
                    }
                    se.push(Se::Select);
                }
                None => se.push(Se::Cancel),
            }
            break;
        }
        (ev, se)
    }

    /// Its type-to-filter input (`filter = true`), if it has one.
    pub fn filter_input(&self) -> Option<usize> {
        self.elems.iter().position(|e| matches!(e, Elem::Input { filter: true, .. }))
    }

    /// Whether it takes the typing from the game: a line being typed, or (focused) a type-to-filter input.
    pub fn takes_text(&self) -> bool {
        self.typing() || (self.focus && self.filter_input().is_some())
    }

    /// In a window with a type-to-filter input, the input typing goes into: the line under the cursor, else the filter.
    pub fn type_into(&self) -> Option<usize> {
        let f = self.filter_input()?;
        Some(if matches!(self.elems.get(self.at), Some(Elem::Input { key: false, .. })) { self.at } else { f })
    }

    /// Focused with a type-to-filter input and no line being typed: text and Backspace go into the line under the
    /// cursor (else the filter), the other keys move and press as ever.
    pub fn filter_text(&mut self, events: &[egui::Event]) -> (Vec<Event>, Vec<Se>) {
        let (mut ev, mut se) = (Vec::new(), Vec::new());
        if self.filter_input().is_none() {
            return (ev, se);
        }
        for e in events {
            if self.typing() {
                // Enter on the filter itself started a typed line: the rest goes there.
                let (a, b) = self.text(std::slice::from_ref(e));
                ev.extend(a);
                se.extend(b);
                continue;
            }
            let Some(i) = self.type_into() else { break };
            let Some(Elem::Input { id, text, max, .. }) = self.elems.get_mut(i) else { break };
            match e {
                egui::Event::Text(t) | egui::Event::Paste(t) => {
                    let before = text.chars().count();
                    text.extend(t.chars().filter(|c| !c.is_control()).take(max.saturating_sub(before)));
                    if text.chars().count() != before {
                        ev.push(Event::Text { id: id.clone(), text: text.clone(), done: false });
                    }
                }
                egui::Event::Key { key: egui::Key::Backspace, pressed: true, .. } => {
                    if text.pop().is_some() {
                        ev.push(Event::Text { id: id.clone(), text: text.clone(), done: false });
                    }
                }
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    let (a, b) = self.key_mod(*key, modifiers.shift);
                    ev.extend(a);
                    se.extend(b);
                }
                _ => {}
            }
        }
        (ev, se)
    }

    /// Ends an edit as Enter would (a typed line is kept and reported; a key element still listening is left as it was).
    pub fn end_edit(&mut self) -> Vec<Event> {
        let mut ev = Vec::new();
        // A key element stopped before a key press is unchanged, as Escape leaves it (no 'change').
        if self.edit
            && let Some(Elem::Input { id, text, key: false, .. }) = self.elems.get(self.at)
        {
            ev.push(Event::Text { id: id.clone(), text: text.clone(), done: true });
        }
        (self.edit, self.undo) = (false, None);
        ev
    }

    /// Turns tabs or a choice by `d` (wrapping).
    fn turn(&mut self, i: usize, d: isize) -> (Vec<Event>, Vec<Se>) {
        if let Some(Elem::Tabs { id, items, at } | Elem::Choice { id, items, at, .. }) = self.elems.get_mut(i)
            && items.len() > 1
        {
            let n = items.len() as isize;
            *at = ((*at as isize + d).rem_euclid(n)) as usize;
            return (vec![Event::Set { id: id.clone(), index: *at }], vec![Se::Cursor]);
        }
        (Vec::new(), Vec::new())
    }

    /// Sets a colour channel to step `s`.
    fn set_channel(&mut self, i: usize, chan: usize, s: usize) -> Vec<Event> {
        if let Some(Elem::Colour { id, value, .. }) = self.elems.get_mut(i) {
            let v = from_step(s);
            if value[chan] != v {
                value[chan] = v;
                return vec![Event::Colour { id: id.clone(), value: *value }];
            }
        }
        Vec::new()
    }

    /// A key with Shift's state: Tab moves as Down, Shift+Tab as Up.
    pub fn key_mod(&mut self, key: egui::Key, shift: bool) -> (Vec<Event>, Vec<Se>) {
        match key {
            egui::Key::Tab if !self.edit => self.key(if shift { egui::Key::ArrowUp } else { egui::Key::ArrowDown }),
            _ => self.key(key),
        }
    }

    /// A key while the window has the focus: the events for the mod and the sounds.
    pub fn key(&mut self, key: egui::Key) -> (Vec<Event>, Vec<Se>) {
        use egui::Key as K;
        let (mut ev, mut se) = (Vec::new(), Vec::new());
        self.settle();
        if self.edit {
            match self.elems.get_mut(self.at) {
                Some(Elem::Slider { id, n, at, .. }) => match key {
                    K::ArrowRight if *at + 1 < *n => {
                        *at += 1;
                        ev.push(Event::Set { id: id.clone(), index: *at });
                        se.push(Se::Cursor);
                    }
                    K::ArrowLeft if *at > 0 => {
                        *at -= 1;
                        ev.push(Event::Set { id: id.clone(), index: *at });
                        se.push(Se::Cursor);
                    }
                    K::Enter | K::Escape => {
                        self.edit = false;
                        se.push(if key == K::Enter { Se::Select } else { Se::Cancel });
                    }
                    _ => {}
                },
                Some(Elem::Colour { value, alpha, .. }) => {
                    let n = if *alpha { 4 } else { 3 };
                    let s = to_step(value[self.chan]);
                    match key {
                        K::ArrowUp | K::ArrowDown => {
                            self.chan = if key == K::ArrowDown { (self.chan + 1) % n } else { (self.chan + n - 1) % n };
                            se.push(Se::Cursor);
                        }
                        K::ArrowRight | K::ArrowLeft => {
                            let to = if key == K::ArrowRight { (s + 1).min(STEPS - 1) } else { s.saturating_sub(1) };
                            let more = self.set_channel(self.at, self.chan, to);
                            if !more.is_empty() {
                                se.push(Se::Cursor);
                            }
                            ev.extend(more);
                        }
                        K::Enter => {
                            (self.edit, self.undo) = (false, None);
                            se.push(Se::Select);
                        }
                        K::Escape => {
                            if let Some(Elem::Colour { value: old, .. }) = self.undo.take()
                                && let Some(Elem::Colour { id, value, .. }) = self.elems.get_mut(self.at)
                                && *value != old
                            {
                                *value = old;
                                ev.push(Event::Colour { id: id.clone(), value: old });
                            }
                            self.edit = false;
                            se.push(Se::Cancel);
                        }
                        _ => {}
                    }
                }
                Some(Elem::Choice { id, items, at, drop: true, .. }) => match key {
                    K::ArrowUp | K::ArrowDown if !items.is_empty() => {
                        let n = items.len();
                        let d = if key == K::ArrowDown { 1 } else { n - 1 };
                        self.drop_at = (self.drop_at + d) % n;
                        se.push(Se::Cursor);
                    }
                    K::Enter => {
                        if self.drop_at < items.len() && self.drop_at != *at {
                            *at = self.drop_at;
                            ev.push(Event::Set { id: id.clone(), index: *at });
                        }
                        self.edit = false;
                        se.push(Se::Select);
                    }
                    K::Escape => {
                        self.edit = false;
                        se.push(Se::Cancel);
                    }
                    _ => {}
                },
                // A typed line takes its keys through `text`.
                Some(Elem::Input { .. }) => {}
                _ => self.edit = false,
            }
            return (ev, se);
        }
        let stops = self.stops();
        let pos = stops.iter().position(|s| *s == self.at);
        match key {
            K::ArrowUp | K::ArrowDown => {
                let down = key == K::ArrowDown;
                // A list moves its cursor until its end.
                if let Some(Elem::List { rows, at, top, height, .. }) = self.elems.get_mut(self.at) {
                    let inside = if down { *at + 1 < rows.len() } else { *at > 0 };
                    if inside {
                        *at = if down { *at + 1 } else { *at - 1 };
                        *top = (*top).min(*at).max((*at + 1).saturating_sub(*height));
                        se.push(Se::Cursor);
                        return (ev, se);
                    }
                }
                if let Some(p) = pos
                    && stops.len() > 1
                {
                    let n = stops.len();
                    self.at = stops[if down { (p + 1) % n } else { (p + n - 1) % n }];
                    self.col = self.marked_col();
                    if let Some(Elem::List { rows, at, top, height, .. }) = self.elems.get_mut(self.at) {
                        *at = if down { 0 } else { rows.len().saturating_sub(1) };
                        *top = (*top).min(*at).max((*at + 1).saturating_sub(*height));
                    }
                    se.push(Se::Cursor);
                }
            }
            K::ArrowLeft | K::ArrowRight => match self.elems.get(self.at) {
                Some(Elem::Buttons { items, .. }) if items.len() > 1 => {
                    let n = items.len();
                    self.col = if key == K::ArrowRight { (self.col + 1) % n } else { (self.col + n - 1) % n };
                    se.push(Se::Cursor);
                }
                Some(Elem::Tabs { .. } | Elem::Choice { .. }) => {
                    return self.turn(self.at, if key == K::ArrowRight { 1 } else { -1 });
                }
                _ => {}
            },
            K::Enter => match self.elems.get_mut(self.at) {
                Some(Elem::Buttons { id, marked, .. }) => {
                    match marked {
                        Some(m) if *m == self.col => {}
                        Some(m) => {
                            *m = self.col;
                            ev.push(Event::Set { id: id.clone(), index: self.col });
                        }
                        None => ev.push(Event::Press { id: id.clone(), index: self.col }),
                    }
                    se.push(Se::Select);
                }
                Some(Elem::Slider { .. }) => {
                    self.edit = true;
                    se.push(Se::Select);
                }
                Some(Elem::List { id, rows, at, .. }) if !rows.is_empty() => {
                    ev.push(Event::Select { id: id.clone(), index: *at });
                    se.push(Se::Select);
                }
                Some(e @ Elem::Input { .. }) => {
                    let Elem::Input { text, .. } = &*e else { unreachable!() };
                    (self.caret, self.undo, self.edit) = (text.chars().count(), Some(e.clone()), true);
                    se.push(Se::Select);
                }
                Some(e @ Elem::Colour { .. }) => {
                    (self.chan, self.undo, self.edit) = (0, Some(e.clone()), true);
                    se.push(Se::Select);
                }
                Some(Elem::Choice { items, at, drop: true, .. }) if !items.is_empty() => {
                    (self.drop_at, self.edit) = (*at, true);
                    se.push(Se::Select);
                }
                Some(Elem::Choice { drop: false, .. }) => return (self.turn(self.at, 1).0, vec![Se::Select]),
                Some(Elem::Section { id, open, .. }) => {
                    *open = !*open;
                    ev.push(Event::Toggle { id: id.clone(), open: *open });
                    se.push(Se::Select);
                }
                _ => {}
            },
            K::Escape => {
                self.open = false;
                self.focus = false;
                ev.push(Event::Close);
                se.push(Se::Cancel);
            }
            _ => {}
        }
        (ev, se)
    }

    /// Typed text and the editing keys while a line is typed: the events (the line as it changes, then Enter's).
    pub fn text(&mut self, events: &[egui::Event]) -> (Vec<Event>, Vec<Se>) {
        use egui::Key as K;
        let (mut ev, mut se) = (Vec::new(), Vec::new());
        if !self.typing() || self.listening() {
            return (ev, se);
        }
        let Some(Elem::Input { id, text, max, .. }) = self.elems.get_mut(self.at) else { return (ev, se) };
        let before = text.clone();
        let mut chars: Vec<char> = text.chars().collect();
        let mut caret = self.caret.min(chars.len());
        let mut end = None;
        for e in events {
            match e {
                egui::Event::Text(t) | egui::Event::Paste(t) => {
                    for c in t.chars().filter(|c| !c.is_control()) {
                        if chars.len() < *max {
                            chars.insert(caret, c);
                            caret += 1;
                        }
                    }
                }
                egui::Event::Key { key, pressed: true, .. } => match key {
                    K::Backspace if caret > 0 => {
                        caret -= 1;
                        chars.remove(caret);
                    }
                    K::Delete if caret < chars.len() => {
                        chars.remove(caret);
                    }
                    K::ArrowLeft => caret = caret.saturating_sub(1),
                    K::ArrowRight => caret = (caret + 1).min(chars.len()),
                    K::Home => caret = 0,
                    K::End => caret = chars.len(),
                    K::Enter => {
                        end = Some(true);
                        break;
                    }
                    K::Escape => {
                        end = Some(false);
                        break;
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        *text = chars.into_iter().collect();
        self.caret = caret;
        match end {
            Some(true) => {
                ev.push(Event::Text { id: id.clone(), text: text.clone(), done: true });
                (self.edit, self.undo) = (false, None);
                se.push(Se::Select);
            }
            Some(false) => {
                let old = match self.undo.take() {
                    Some(Elem::Input { text, .. }) => text,
                    _ => before.clone(),
                };
                // The mod last saw `before`.
                if before != old {
                    ev.push(Event::Text { id: id.clone(), text: old.clone(), done: false });
                }
                *text = old;
                self.edit = false;
                se.push(Se::Cancel);
            }
            None if *text != before => ev.push(Event::Text { id: id.clone(), text: text.clone(), done: false }),
            None => {}
        }
        (ev, se)
    }

    /// The pointer on element `i`: the focus and cursor go there, then a button is pressed (as Enter), a slider set,
    /// a list row picked, a line started.
    pub fn click_button(&mut self, i: usize, col: usize) -> (Vec<Event>, Vec<Se>) {
        if self.edit && self.at == i && matches!(self.elems.get(i), Some(Elem::Input { .. })) {
            return (Vec::new(), Vec::new());
        }
        // A click on an open dropdown's field closes it.
        if self.drop_open() == Some(i) {
            self.edit = false;
            return (Vec::new(), vec![Se::Cancel]);
        }
        let mut ev = if self.at != i { self.end_edit() } else { Vec::new() };
        // A click on a colour being edited ends the edit, as Enter.
        let colour_open = self.edit && self.at == i && matches!(self.elems.get(i), Some(Elem::Colour { .. }));
        (self.at, self.col) = (i, col);
        if !colour_open {
            (self.edit, self.undo) = (false, None);
        }
        let (more, se) = self.key(egui::Key::Enter);
        ev.extend(more);
        (ev, se)
    }

    pub fn slide(&mut self, i: usize, stop: usize) -> (Vec<Event>, Vec<Se>) {
        let mut ev = if self.at != i { self.end_edit() } else { Vec::new() };
        self.edit = false;
        self.at = i;
        if let Some(Elem::Slider { id, n, at, .. }) = self.elems.get_mut(i)
            && stop < *n
            && stop != *at
        {
            *at = stop;
            ev.push(Event::Set { id: id.clone(), index: stop });
            return (ev, vec![Se::Cursor]);
        }
        (ev, Vec::new())
    }

    pub fn pick(&mut self, i: usize, row: usize) -> (Vec<Event>, Vec<Se>) {
        let mut ev = self.end_edit();
        self.at = i;
        if let Some(Elem::List { at, .. }) = self.elems.get_mut(i) {
            *at = row;
        }
        let (more, se) = self.key(egui::Key::Enter);
        ev.extend(more);
        (ev, se)
    }

    /// An open dropdown's item `k` picked by the pointer.
    pub fn drop_pick(&mut self, i: usize, k: usize) -> (Vec<Event>, Vec<Se>) {
        if self.drop_open() != Some(i) {
            return (Vec::new(), Vec::new());
        }
        self.drop_at = k;
        self.key(egui::Key::Enter)
    }

    /// The pointer over an open dropdown's item `k`: the highlight goes there.
    pub fn drop_hover(&mut self, i: usize, k: usize) -> (Vec<Event>, Vec<Se>) {
        if self.drop_open() == Some(i) && self.drop_at != k {
            self.drop_at = k;
            return (Vec::new(), vec![Se::Cursor]);
        }
        (Vec::new(), Vec::new())
    }

    /// The pointer pressed off an open dropdown: it closes.
    pub fn drop_close(&mut self, i: usize) -> (Vec<Event>, Vec<Se>) {
        if self.drop_open() == Some(i) {
            self.edit = false;
        }
        (Vec::new(), Vec::new())
    }

    /// Tabs or a choice turned by the pointer: by `d`, or straight to `to`.
    pub fn turn_to(&mut self, i: usize, d: isize, to: Option<usize>) -> (Vec<Event>, Vec<Se>) {
        let mut ev = if self.at != i { self.end_edit() } else { Vec::new() };
        (self.at, self.edit) = (i, false);
        let (more, se) = match (to, self.elems.get(i)) {
            (Some(k), Some(Elem::Tabs { at, .. } | Elem::Choice { at, .. })) => {
                let d = k as isize - *at as isize;
                if d == 0 { (Vec::new(), Vec::new()) } else { self.turn(i, d) }
            }
            _ => self.turn(i, d),
        };
        ev.extend(more);
        (ev, se)
    }

    /// A colour set by the pointer (the modern picker): `rgb`, and alpha as a step when given.
    pub fn set_colour(&mut self, i: usize, rgb: Option<[u8; 3]>, alpha: Option<usize>) -> (Vec<Event>, Vec<Se>) {
        self.at = i;
        if let Some(Elem::Colour { id, value, .. }) = self.elems.get_mut(i) {
            let old = *value;
            if let Some([r, g, b]) = rgb {
                (value[0], value[1], value[2]) = (r, g, b);
            }
            if let Some(s) = alpha {
                value[3] = from_step(s);
            }
            if *value != old {
                return (vec![Event::Colour { id: id.clone(), value: *value }], Vec::new());
            }
        }
        (Vec::new(), Vec::new())
    }

    /// A column heading clicked.
    pub fn sort(&mut self, i: usize, column: usize) -> (Vec<Event>, Vec<Se>) {
        let mut ev = if self.at != i { self.end_edit() } else { Vec::new() };
        self.at = i;
        if let Some(Elem::List { id, .. }) = self.elems.get(i) {
            ev.push(Event::Sort { id: id.clone(), column });
        }
        (ev, vec![Se::Select])
    }

    /// A lines element's fold header (its line) clicked.
    pub fn fold(&mut self, i: usize, line: usize) -> (Vec<Event>, Vec<Se>) {
        match self.elems.get(i) {
            Some(Elem::Lines { id, lines }) => match lines.get(line).and_then(|l| l.fold.clone()) {
                Some(key) => (vec![Event::Fold { id: id.clone(), key }], vec![Se::Cursor]),
                None => (Vec::new(), Vec::new()),
            },
            _ => (Vec::new(), Vec::new()),
        }
    }

    /// A canvas clicked at (x, y) in its units.
    pub fn canvas_click(&mut self, i: usize, x: f32, y: f32, right: bool) -> (Vec<Event>, Vec<Se>) {
        let mut ev = self.end_edit();
        if let Some(Elem::Canvas { id, .. }) = self.elems.get(i) {
            ev.push(Event::Click { id: id.clone(), x, y, right });
        }
        (ev, Vec::new())
    }

    /// The pointer gone off the canvas it was over (classic UI): its 'leave'.
    pub fn hover_off(&mut self) -> Vec<Event> {
        self.hovered.take().map_or_else(Vec::new, |i| self.canvas_hover(i, -1.0, -1.0).0)
    }

    /// An edge window now stands on `side` (the `dock` event).
    pub fn docked(&mut self, side: &'static str) -> (Vec<Event>, Vec<Se>) {
        self.dock = Some(side);
        (vec![Event::Dock { side }], Vec::new())
    }

    /// The pointer over a canvas at (x, y) in its units, or off it (negative).
    pub fn canvas_hover(&mut self, i: usize, x: f32, y: f32) -> (Vec<Event>, Vec<Se>) {
        match self.elems.get(i) {
            Some(Elem::Canvas { id, .. }) => (vec![Event::Hover { id: id.clone(), x, y }], Vec::new()),
            _ => (Vec::new(), Vec::new()),
        }
    }

    /// A canvas drag at (x, y) in its units: 0 starts it, 1 moves (reported when it moved), 2 ends it.
    pub fn canvas_drag(&mut self, i: usize, phase: u8, x: f32, y: f32) -> (Vec<Event>, Vec<Se>) {
        let Some(Elem::Canvas { id, .. }) = self.elems.get(i) else { return (Vec::new(), Vec::new()) };
        let id = id.clone();
        let (lx, ly) = match (phase, self.drag) {
            (0, _) => (x, y),
            (_, Some((j, lx, ly))) if j == i => (lx, ly),
            _ => return (Vec::new(), Vec::new()),
        };
        if phase == 1 && lx == x && ly == y {
            return (Vec::new(), Vec::new());
        }
        self.drag = (phase != 2).then_some((i, x, y));
        (vec![Event::Drag { id, phase, x, y, dx: x - lx, dy: y - ly, screen: None, over: None }], Vec::new())
    }

    /// The classic list's text width and its columns.
    fn classic_list_w(&self, rows: usize, height: usize) -> f32 {
        let (w, _) = self.classic_size();
        w - 64.0 - if rows > height { 12.0 } else { 0.0 }
    }

    /// The classic page's controls at (x, y), as drawn: (element, part, rect). Parts: a button's column, a list's
    /// row; tabs and choices 0 back, 1 on, 2 the name; a colour 0 its box, 1.. a channel; a heading column + 1000.
    fn classic_controls(&self, gu: &GameUi, x: f32, y: f32) -> Vec<(usize, usize, [f32; 4])> {
        let (w, _) = self.classic_size();
        let hidden = self.hidden();
        // An open dropdown's rows (parts 1..) lie over what is under them: they come first.
        let mut out = Vec::new();
        if let Some((i, b)) = self.drop_box(x, y)
            && let Some(Elem::Choice { items, .. }) = self.elems.get(i)
        {
            for k in 0..items.len() {
                out.push((i, 1 + k, [b[0], b[1] + LINE * k as f32, b[2], LINE]));
            }
        }
        let mut ey = y + TOP;
        for (i, e) in self.elems.iter().enumerate() {
            if hidden[i] {
                continue;
            }
            match e {
                Elem::Buttons { items, .. } => {
                    let bw = button_w(gu, items);
                    for c in 0..items.len() {
                        out.push((i, c, [x + 32.0 + (bw + 8.0) * c as f32, ey + 14.0, bw, 16.0]));
                    }
                }
                Elem::Slider { .. } => {
                    out.push((i, 0, [x + SLIDER_X, ey + 14.0, (w - SLIDER_X - 48.0).max(96.0), 16.0]))
                }
                Elem::List { rows, columns, top, height, .. } => {
                    let mut ry = ey;
                    if has_header(columns) {
                        for (c, (cx, cw, _)) in
                            self::columns(columns, self.classic_list_w(rows.len(), *height)).into_iter().enumerate()
                        {
                            out.push((i, 1000 + c, [x + 32.0 + cx, ey, cw, LINE]));
                        }
                        ry += LINE;
                    }
                    for k in (*top..rows.len()).take(*height) {
                        out.push((i, k, [x + 32.0, ry + LINE * (k - *top) as f32, w - 64.0, LINE]));
                    }
                }
                Elem::Tabs { .. } => {
                    out.push((i, 0, [x + 5.0, ey - 3.0, 26.0, 17.0]));
                    out.push((i, 1, [x + w - 27.0, ey - 3.0, 26.0, 17.0]));
                    out.push((i, 2, [x + 40.0, ey, w - 80.0, LINE]));
                }
                Elem::Choice { drop: true, .. } => out.push((i, 0, [x + 32.0, ey + 14.0, w - 64.0, 16.0])),
                Elem::Choice { .. } => {
                    let bw = w - 64.0;
                    out.push((i, 0, [x + 32.0, ey + 14.0, 24.0, 16.0]));
                    out.push((i, 1, [x + 32.0 + bw - 24.0, ey + 14.0, 24.0, 16.0]));
                    out.push((i, 2, [x + 56.0, ey + 14.0, bw - 48.0, 16.0]));
                }
                Elem::Input { .. } => out.push((i, 0, [x + 32.0, ey + 14.0, w - 64.0, 16.0])),
                Elem::Colour { alpha, .. } => {
                    out.push((i, 0, [x + 32.0, ey + 14.0, 64.0, 16.0]));
                    if self.edit && self.at == i {
                        for c in 0..if *alpha { 4 } else { 3 } {
                            out.push((
                                i,
                                1 + c,
                                [
                                    x + SLIDER_X,
                                    ey + PITCH + CHAN_H * c as f32 + 4.0,
                                    (w - SLIDER_X - 48.0).max(96.0),
                                    16.0,
                                ],
                            ));
                        }
                    }
                }
                Elem::Section { .. } => out.push((i, 0, [x + 20.0, ey, w - 40.0, LINE])),
                Elem::Canvas { w: cw, h: ch, .. } => out.push((i, 0, [x + 20.0, ey, *cw, *ch])),
                _ => {}
            }
            ey += self.elem_h(i);
        }
        out
    }

    /// The pointer on the classic page at (x, y): the cursor follows it over controls, a click acts. Whether it is
    /// over a control, and what it did.
    pub fn pointer_classic(
        &mut self,
        gu: &GameUi,
        x: f32,
        y: f32,
        p: (f32, f32),
        moved: bool,
        clicked: bool,
        right: bool,
        held: (bool, bool, bool),
    ) -> (bool, Vec<Event>, Vec<Se>) {
        let inside = |r: &[f32; 4]| p.0 >= r[0] && p.1 >= r[1] && p.0 < r[0] + r[2] && p.1 < r[1] + r[3];
        let (pressed, down, released) = held;
        // A drag keeps the pointer, inside the canvas or not, until the button comes up.
        if let Some((di, ..)) = self.drag {
            if let Some(r) = self.classic_controls(gu, x, y).into_iter().find(|c| c.0 == di).map(|c| c.2) {
                let (cx, cy) = (p.0 - r[0], p.1 - r[1]);
                let ev = if released || !down {
                    self.canvas_drag(di, 2, cx, cy).0
                } else if moved {
                    self.canvas_drag(di, 1, cx, cy).0
                } else {
                    Vec::new()
                };
                return (true, ev, Vec::new());
            }
            self.drag = None;
        }
        if let Some((pi, px, py)) = self.press {
            if released || !down {
                self.press = None;
            } else if let Some(r) = self.classic_controls(gu, x, y).into_iter().find(|c| c.0 == pi).map(|c| c.2) {
                let (cx, cy) = (p.0 - r[0], p.1 - r[1]);
                if (cx - px).abs() + (cy - py).abs() > 3.0 {
                    self.press = None;
                    let mut ev = self.canvas_drag(pi, 0, px, py).0;
                    ev.extend(self.canvas_drag(pi, 1, cx, cy).0);
                    return (true, ev, Vec::new());
                }
                return (true, Vec::new(), Vec::new());
            } else {
                self.press = None;
            }
        }
        let Some((i, k, r)) = self.classic_controls(gu, x, y).into_iter().find(|c| inside(&c.2)) else {
            if (clicked || right)
                && let Some(d) = self.drop_open()
            {
                self.drop_close(d);
            }
            return (false, self.hover_off(), Vec::new());
        };
        let mut se = Vec::new();
        #[derive(PartialEq)]
        enum K {
            Slider(usize),
            List,
            Buttons,
            Turn,
            Drop,
            Colour,
            Canvas,
            Other,
        }
        let e = match self.elems.get(i) {
            Some(Elem::Slider { n, .. }) => K::Slider(*n),
            Some(Elem::List { .. }) => K::List,
            Some(Elem::Buttons { .. }) => K::Buttons,
            Some(Elem::Choice { drop: true, .. }) => K::Drop,
            Some(Elem::Tabs { .. } | Elem::Choice { .. }) => K::Turn,
            Some(Elem::Colour { .. }) => K::Colour,
            Some(Elem::Canvas { .. }) => K::Canvas,
            _ => K::Other,
        };
        let (list, buttons) = (e == K::List, e == K::Buttons);
        if e == K::Canvas {
            let (cx, cy) = (p.0 - r[0], p.1 - r[1]);
            let mut ev = if self.hovered.is_some_and(|h| h != i) { self.hover_off() } else { Vec::new() };
            if moved || self.hovered.is_none() {
                self.hovered = Some(i);
                ev.extend(self.canvas_hover(i, cx, cy).0);
            }
            if clicked || right {
                ev.extend(self.canvas_click(i, cx, cy, !clicked).0);
            }
            if pressed {
                self.press = Some((i, cx, cy));
            }
            return (false, ev, Vec::new());
        }
        // Off a canvas onto another control: the canvas's 'leave'.
        let mut ev = self.hover_off();
        // An open dropdown's highlight follows the pointer over its rows.
        if e == K::Drop && k > 0 && moved && self.drop_at != k - 1 {
            self.drop_at = k - 1;
            se.push(Se::Cursor);
        }
        if (moved || clicked) && !self.edit && (self.at != i || (buttons && self.col != k)) {
            (self.at, self.col) = (i, if buttons { k } else { 0 });
            se.push(Se::Cursor);
        }
        if list
            && k < 1000
            && let Some(Elem::List { at, .. }) = self.elems.get_mut(i)
            && (moved || clicked)
        {
            *at = k;
        }
        if !clicked {
            return (true, ev, se);
        }
        let (more_ev, more) = match e {
            K::Slider(n) => {
                let stop = (((p.0 - r[0]) / r[2]).clamp(0.0, 1.0) * (n - 1) as f32).round() as usize;
                self.slide(i, stop)
            }
            K::List if k >= 1000 => self.sort(i, k - 1000),
            K::List => self.pick(i, k),
            K::Turn => self.turn_to(i, if k == 0 { -1 } else { 1 }, None),
            K::Drop if k > 0 => self.drop_pick(i, k - 1),
            K::Colour if k > 0 => {
                self.chan = k - 1;
                let stop = (((p.0 - r[0]) / r[2]).clamp(0.0, 1.0) * (STEPS - 1) as f32).round() as usize;
                (self.set_channel(i, k - 1, stop), vec![Se::Cursor])
            }
            _ => self.click_button(i, k),
        };
        ev.extend(more_ev);
        se.extend(more);
        (true, ev, se)
    }

    /// Element `i`'s height on the classic page (0 hidden).
    fn elem_h(&self, i: usize) -> f32 {
        if self.hidden().get(i).copied().unwrap_or(false) {
            return 0.0;
        }
        match &self.elems[i] {
            // The Timers' panels draw their sections (timers.rs, modern/extras.rs); target rows are modern only.
            Elem::Timers { .. } | Elem::Map { .. } | Elem::Targets { .. } | Elem::Chip { .. } => 0.0,
            Elem::Lines { lines, .. } => lines.iter().map(classic_line_h).sum::<f32>() + 4.0,
            Elem::Text { .. } => LINE,
            Elem::Buttons { .. } | Elem::Slider { .. } | Elem::Input { .. } | Elem::Choice { .. } => PITCH,
            Elem::List { height, columns, .. } => LINE * (*height + usize::from(has_header(columns))) as f32 + 6.0,
            Elem::Gauge { .. } => LINE + 2.0,
            Elem::Separator => 8.0,
            Elem::Tabs { .. } => LINE + 6.0,
            Elem::Colour { alpha, .. } => {
                PITCH + if self.edit && self.at == i { CHAN_H * if *alpha { 4.0 } else { 3.0 } + 4.0 } else { 0.0 }
            }
            Elem::Section { .. } => LINE + 4.0,
            Elem::Canvas { h, .. } => h + 6.0,
        }
    }

    pub fn classic_size(&self) -> (f32, f32) {
        let w = if self.width > 0.0 { self.width } else { CLASSIC_W };
        (w, TOP + (0..self.elems.len()).map(|i| self.elem_h(i)).sum::<f32>() + 12.0)
    }

    /// The open dropdown and its list's rect on the classic page at (x, y): the rows under its field, as the login
    /// form's combo fields'.
    fn drop_box(&self, x: f32, y: f32) -> Option<(usize, [f32; 4])> {
        let i = self.drop_open()?;
        let Some(Elem::Choice { items, .. }) = self.elems.get(i) else { return None };
        let (w, _) = self.classic_size();
        let ey = y + TOP + (0..i).map(|j| self.elem_h(j)).sum::<f32>();
        Some((i, [x + 32.0, ey + 30.0, w - 64.0, LINE * items.len() as f32]))
    }

    /// What the classic page covers: its size, taller while an open dropdown's list hangs below it.
    pub fn classic_reach(&self) -> (f32, f32) {
        let (w, h) = self.classic_size();
        (w, self.drop_box(0.0, 0.0).map_or(h, |(_, b)| h.max(b[1] + b[3])))
    }

    /// The classic page at (x, y): the Config pages' frame, bullets, buttons, sliders and the hand.
    pub fn draw_classic(&self, gu: &mut GameUi, world: Option<&super::World>, x: f32, y: f32) -> [f32; 4] {
        let (w, h) = self.classic_size();
        if self.framed {
            gu.frame(x, y, w, h);
        }
        gu.text(&self.title, x + 16.0, y + 10.0, 0, [0x80; 4]);
        let hidden = self.hidden();
        let mut ey = y + TOP;
        let mut hand = None;
        let bullet = |gu: &mut GameUi, ey: f32| {
            if let Some(b) = gu.cell_bounds("menu    windowps", 465, 2) {
                gu.sprite_cells("menu    windowps", 465, 2..3, x + 20.0 - b[0], ey + 2.0 - b[1], N);
            }
        };
        let slider = |gu: &mut GameUi, sx: f32, sy: f32, f: f32, lit: bool| {
            gu.sprite_cells("menu    windowps", 465, 8..11, sx - 88.0, sy - 80.0, N);
            if f > 0.0 {
                gu.sprite_scaled("menu    windowps", 384, sx, sy, f, 1.0, if lit { 0xa0a0_a0a0 } else { N });
            }
        };
        for (i, e) in self.elems.iter().enumerate() {
            if hidden[i] {
                continue;
            }
            let here = self.focus && i == self.at;
            match e {
                Elem::Timers { .. } | Elem::Map { .. } | Elem::Targets { .. } | Elem::Chip { .. } => {}
                Elem::Lines { lines, .. } => {
                    let mut ly = ey;
                    for l in lines {
                        draw_classic_line(gu, l, x + 20.0, ly, w - 40.0);
                        ly += classic_line_h(l);
                    }
                }
                Elem::Text { text, dim, colour, .. } => {
                    gu.text_fit(text, x + 20.0, ey, w - 40.0, colour.map_or(if *dim { DIM } else { N }, tint));
                }
                Elem::Buttons { label, items, marked, .. } => {
                    bullet(gu, ey);
                    gu.label(label, x + 32.0, ey, N, false);
                    let bw = button_w(gu, items);
                    for (c, s) in items.iter().enumerate() {
                        let bx = x + 32.0 + (bw + 8.0) * c as f32;
                        let by = ey + 14.0;
                        let on = here && c == self.col && !self.edit;
                        let d = if on { -1.0 } else { 0.0 };
                        super::super::settings::button(gu, [bx + d, by + d, bw, 16.0], if on { ON } else { N });
                        gu.label(s, bx + 5.0 + d, by + 1.0 + d, if on { ON } else { N }, true);
                        if *marked == Some(c) {
                            super::super::settings::marker(gu, bx, by, bw);
                        }
                        if on {
                            hand = Some((bx, by));
                        }
                    }
                }
                Elem::Slider { label, n, at, shown, .. } => {
                    bullet(gu, ey);
                    gu.label(label, x + 32.0, ey, N, false);
                    let sw = (w - SLIDER_X - 48.0).max(96.0);
                    let vw = gu.label_width(shown);
                    gu.label(shown, x + SLIDER_X + sw - vw, ey, N, false);
                    let (sx, sy) = (x + SLIDER_X, ey + 14.0);
                    let f = if *n > 1 { *at as f32 / (*n - 1) as f32 } else { 1.0 };
                    slider(gu, sx, sy, f, self.edit && here);
                    if here && !self.edit {
                        hand = Some((sx, sy));
                    }
                }
                Elem::List { rows, columns, at, top, height, .. } => {
                    let lw = self.classic_list_w(rows.len(), *height);
                    let cols = self::columns(columns, lw);
                    let mut ry0 = ey;
                    if has_header(columns) {
                        for (c, &(cx, cw, right)) in cols.iter().enumerate() {
                            let t = &columns[c].title;
                            let tx = if right {
                                (x + 32.0 + cx + cw - gu.text_width(t) - 4.0).max(x + 32.0 + cx)
                            } else {
                                x + 32.0 + cx
                            };
                            gu.text_fit(t, tx, ey, cw, DIM);
                        }
                        ry0 += LINE;
                    }
                    for (k, row) in rows.iter().enumerate().skip(*top).take(*height) {
                        let ry = ry0 + LINE * (k - *top) as f32;
                        let ink = row.colour.map_or(N, tint);
                        let mut ix = 0.0;
                        if let Some(i) = row.icon {
                            icon(gu, world, i, x + 32.0, ry + 1.0, 16.0, [255; 4]);
                            ix = 18.0;
                        }
                        if cols.is_empty() {
                            gu.text_fit(&row.cells.join("  "), x + 32.0 + ix, ry, lw - ix, ink);
                        } else {
                            for (c, &(cx, cw, right)) in cols.iter().enumerate() {
                                let Some(cell) = row.cells.get(c) else { continue };
                                let (cx, cw) = if c == 0 { (cx + ix, cw - ix) } else { (cx, cw) };
                                if right {
                                    let tx = (x + 32.0 + cx + cw - gu.text_width(cell) - 4.0).max(x + 32.0 + cx);
                                    gu.text_fit(cell, tx, ry, x + 32.0 + cx + cw - tx + 16.0, ink);
                                } else {
                                    gu.text_fit(cell, x + 32.0 + cx, ry, cw - 4.0, ink);
                                }
                            }
                        }
                        if here && k == *at {
                            hand = Some((x + 32.0, ry));
                        }
                    }
                    if rows.len() > *height {
                        gu.scroll_bar(x + w - 20.0, ry0, LINE * *height as f32, rows.len(), *height, *top);
                    }
                }
                Elem::Gauge { label, value, text, kind, .. } => {
                    // The Timers window's gauge: frame 38 and its fill 97, coloured by kind.
                    let colour = match kind.as_str() {
                        "mp" => [0x80, 0x50, 0x30, 0x80],
                        "tp" => [0x30, 0x60, 0x80, 0x80],
                        "cast" => [0x30, 0x80, 0x80, 0x80],
                        "experience" => [0x80, 0x40, 0x80, 0x80],
                        "recast" => [0x60, 0x60, 0x60, 0x80],
                        _ => [0x30, 0x80, 0x30, 0x80],
                    };
                    let gx = x + w - 92.0;
                    gu.text_fit(label, x + 20.0, ey, gx - x - 80.0, N);
                    gu.shape_text(text, gx - 8.0, ey + 1.0, N, true);
                    let f = value.clamp(0.0, 1.0);
                    if f > 0.0 {
                        gu.sprite_coloured("menu    windowps", 97, gx + 4.0, ey + 7.0, f, 1.0, colour, 8);
                    }
                    gu.sprite("menu    windowps", 38, gx, ey + 6.0, N);
                }
                Elem::Separator => {}
                Elem::Tabs { items, at, .. } => {
                    // The Settings pages' arrows (menu frames 25 / 26) round the page name.
                    gu.sprite("menu    frames", 25, x + 9.0, ey + 1.0, N);
                    gu.sprite("menu    frames", 26, x + w - 23.0, ey + 1.0, N);
                    let name = items.get(*at).map_or("", String::as_str);
                    let nw = gu.text_width(name);
                    gu.text(name, x + (w - nw) / 2.0, ey, 0, [0x80; 4]);
                    if here {
                        hand = Some((x + (w - nw) / 2.0 - 4.0, ey));
                    }
                }
                Elem::Input { label, text, key: true, .. } => {
                    bullet(gu, ey);
                    gu.label(label, x + 32.0, ey, N, false);
                    let (bx, by, bw) = (x + 32.0, ey + 14.0, w - 64.0);
                    super::super::settings::button(gu, [bx, by, bw, 16.0], N);
                    let shown = if here && self.edit {
                        "Press a key...".to_string()
                    } else if text.is_empty() {
                        "No key".to_string()
                    } else {
                        super::input::combo_label(text)
                    };
                    gu.text_fit(&shown, bx + 5.0, by + 1.0, bw - 10.0, N);
                    if here {
                        hand = Some((bx, by));
                    }
                }
                Elem::Input { label, text, .. } => {
                    bullet(gu, ey);
                    gu.label(label, x + 32.0, ey, N, false);
                    let (bx, by, bw) = (x + 32.0, ey + 14.0, w - 64.0);
                    super::super::settings::button(gu, [bx, by, bw, 16.0], N);
                    if here && self.edit {
                        let chars: Vec<char> = text.chars().collect();
                        let k = self.caret.min(chars.len());
                        let left: String = chars[..k].iter().collect();
                        let lw = gu.label_width(&left);
                        // Scrolled left to keep the caret in the box.
                        let shift = (lw - (bw - 14.0)).max(0.0);
                        gu.text_fit(text, bx + 5.0 - shift, by + 1.0, bw - 10.0 + shift, N);
                        let blink = (std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis()
                            / 500)
                            .is_multiple_of(2);
                        if blink {
                            gu.text("|", bx + 4.0 + lw - shift, by + 1.0, 0, [0x80; 4]);
                        }
                    } else {
                        gu.text_fit(text, bx + 5.0, by + 1.0, bw - 10.0, N);
                        if here {
                            hand = Some((bx, by));
                        }
                    }
                }
                Elem::Choice { label, items, at, drop: true, .. } => {
                    bullet(gu, ey);
                    gu.label(label, x + 32.0, ey, N, false);
                    let (bx, by, bw) = (x + 32.0, ey + 14.0, w - 64.0);
                    let ink = if here { ON } else { N };
                    super::super::settings::button(gu, [bx, by, bw, 16.0], ink);
                    let v = items.get(*at).map_or("", String::as_str);
                    gu.text_fit(v, bx + 5.0, by + 1.0, bw - 26.0, ink);
                    gu.combo_arrow([bx + bw - 16.0, by, 16.0, 16.0], self.drop_open() == Some(i));
                    if here && self.drop_open().is_none() {
                        hand = Some((bx, by));
                    }
                }
                Elem::Choice { label, items, at, .. } => {
                    bullet(gu, ey);
                    gu.label(label, x + 32.0, ey, N, false);
                    let (bx, by, bw) = (x + 32.0, ey + 14.0, w - 64.0);
                    super::super::settings::button(gu, [bx, by, bw, 16.0], if here { ON } else { N });
                    gu.sprite("menu    frames", 25, bx + 3.0, by + 4.0, N);
                    gu.sprite("menu    frames", 26, bx + bw - 21.0, by + 4.0, N);
                    let v = items.get(*at).map_or("", String::as_str);
                    let vw = gu.text_width(v).min(bw - 48.0);
                    gu.text_fit(v, bx + (bw - vw) / 2.0, by + 1.0, bw - 48.0, if here { ON } else { N });
                    if here {
                        hand = Some((bx, by));
                    }
                }
                Elem::Colour { label, value, alpha, .. } => {
                    bullet(gu, ey);
                    gu.label(label, x + 32.0, ey, N, false);
                    let (bx, by) = (x + 32.0, ey + 14.0);
                    super::super::settings::button(gu, [bx, by, 64.0, 16.0], N);
                    let [r, g, b, a] = value.map(|v| v as f32 / 255.0);
                    gu.rect(bx + 3.0, by + 3.0, 58.0, 10.0, [r, g, b, a.max(0.15)]);
                    let shown = if *alpha {
                        format!("{} {} {} {}", value[0], value[1], value[2], value[3])
                    } else {
                        format!("{} {} {}", value[0], value[1], value[2])
                    };
                    gu.text_fit(&shown, bx + 72.0, by + 1.0, w - 112.0, DIM);
                    let open = self.edit && here;
                    if here && !open {
                        hand = Some((bx, by));
                    }
                    if open {
                        let names = ["R", "G", "B", "A"];
                        for c in 0..if *alpha { 4 } else { 3 } {
                            let cy = ey + PITCH + CHAN_H * c as f32;
                            gu.label(names[c], x + 48.0, cy + 2.0, if c == self.chan { ON } else { N }, false);
                            let v = value[c].to_string();
                            let sw = (w - SLIDER_X - 48.0).max(96.0);
                            gu.label(&v, x + SLIDER_X + sw + 6.0, cy + 2.0, N, false);
                            slider(
                                gu,
                                x + SLIDER_X,
                                cy + 4.0,
                                to_step(value[c]) as f32 / (STEPS - 1) as f32,
                                c == self.chan,
                            );
                            if c == self.chan {
                                hand = Some((x + SLIDER_X, cy + 4.0));
                            }
                        }
                    }
                }
                Elem::Canvas { w: cw, h: ch, cmds, .. } => {
                    draw_canvas(gu, world, cmds, x + 20.0, ey, *cw, *ch);
                }
                Elem::Section { label, open, .. } => {
                    let mark = if *open { "-" } else { "+" };
                    gu.text(mark, x + 20.0, ey + 2.0, 0, [0x80; 4]);
                    gu.label(label, x + 32.0, ey + 2.0, if here { ON } else { N }, false);
                    if here {
                        hand = Some((x + 32.0, ey + 2.0));
                    }
                }
            }
            ey += self.elem_h(i);
        }
        // An open dropdown's list over what is under it, as the login form's: the cursor's row lit, the hand on it.
        if let Some((i, b)) = self.drop_box(x, y)
            && let Some(Elem::Choice { items, .. }) = self.elems.get(i)
        {
            for (k, item) in items.iter().enumerate() {
                let r = [b[0], b[1] + LINE * k as f32, b[2], LINE];
                let shown = super::super::fit_tail(gu, item, r[2] - 10.0);
                gu.combo_row(r, &shown, k == self.drop_at);
            }
            hand = Some((b[0], b[1] + LINE * self.drop_at as f32));
        }
        if let Some((hx, hy)) = hand {
            gu.cursor(hx, hy);
        }
        let (_, reach) = self.classic_reach();
        [x, y, w, h.max(reach)]
    }
}

fn f4(c: [u8; 4]) -> [f32; 4] {
    c.map(|v| v as f32 / 255.0)
}

/// A line `width` wide from a to b as a quad.
fn line(gu: &mut GameUi, a: [f32; 2], b: [f32; 2], width: f32, c: [f32; 4]) {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = (dx * dx + dy * dy).sqrt().max(1e-3);
    let (nx, ny) = (-dy / len * width / 2.0, dx / len * width / 2.0);
    gu.quad([[a[0] + nx, a[1] + ny], [b[0] + nx, b[1] + ny], [a[0] - nx, a[1] - ny], [b[0] - nx, b[1] - ny]], [c; 4]);
}

/// The points round a circle (32 segments).
fn ring(c: [f32; 2], r: f32) -> Vec<[f32; 2]> {
    (0..=32)
        .map(|k| k as f32 / 32.0 * std::f32::consts::TAU)
        .map(|t| [c[0] + r * t.cos(), c[1] + r * t.sin()])
        .collect()
}

/// A canvas's commands at (x, y), clipped to it.
fn draw_canvas(gu: &mut GameUi, world: Option<&super::World>, cmds: &[Cmd], x: f32, y: f32, w: f32, h: f32) {
    gu.clip(Some([x, y, w, h]));
    let at = |p: [f32; 2]| [x + p[0], y + p[1]];
    for c in cmds {
        match c {
            Cmd::Line { a, b, width, colour } => line(gu, at(*a), at(*b), *width, f4(*colour)),
            Cmd::Rect { r, colour, to, line: None, .. } => {
                let (top, bottom) = (f4(*colour), f4(to.unwrap_or(*colour)));
                let [rx, ry] = at([r[0], r[1]]);
                gu.quad(
                    [[rx, ry], [rx + r[2], ry], [rx, ry + r[3]], [rx + r[2], ry + r[3]]],
                    [top, top, bottom, bottom],
                );
            }
            Cmd::Rect { r, colour, line: Some(lw), .. } => {
                let [rx, ry] = at([r[0], r[1]]);
                let c = f4(*colour);
                gu.rect(rx, ry, r[2], *lw, c);
                gu.rect(rx, ry + r[3] - lw, r[2], *lw, c);
                gu.rect(rx, ry + lw, *lw, r[3] - 2.0 * lw, c);
                gu.rect(rx + r[2] - lw, ry + lw, *lw, r[3] - 2.0 * lw, c);
            }
            Cmd::Circle { c, radius, colour, line: lw } => {
                let pts = ring(at(*c), *radius);
                let col = f4(*colour);
                for p in pts.windows(2) {
                    match lw {
                        Some(lw) => line(gu, p[0], p[1], *lw, col),
                        None => {
                            let m = at(*c);
                            gu.quad([m, p[0], p[1], p[1]], [col; 4]);
                        }
                    }
                }
            }
            Cmd::Tri { pts, colour } => {
                let p = pts.map(at);
                gu.quad([p[0], p[1], p[2], p[2]], [f4(*colour); 4]);
            }
            // The classic UI's game font: one size, left at `at` (centred / right by its width).
            Cmd::Text { at: p, text, colour, align, .. } => {
                let [mut tx, ty] = at(*p);
                match align {
                    1 => tx -= gu.label_width(text) / 2.0,
                    2 => tx -= gu.label_width(text),
                    _ => {}
                }
                gu.text_fit(text, tx, ty, x + w - tx + 16.0, tint(*colour));
            }
            Cmd::Tip { .. } => {}
            Cmd::Icon { at: p, size, icon: i, tint: t } => {
                let [ix, iy] = at(*p);
                icon(gu, world, *i, ix, iy, *size, *t);
            }
        }
    }
    gu.clip(None);
}

/// The Config pages' button width for these captions.
fn button_w(gu: &GameUi, items: &[String]) -> f32 {
    let need = items.iter().map(|s| gu.label_width(s) + 12.0).fold(0.0, f32::max).ceil();
    if need <= 64.0 && items.len() >= 3 {
        64.0
    } else if need <= 88.0 {
        88.0
    } else {
        need
    }
}

/// An icon at (x, y), `s` square.
/// `colour` multiplies its colours (white: as drawn; its alpha fades it).
pub(super) fn icon(gu: &mut GameUi, w: Option<&super::World>, i: Icon, x: f32, y: f32, s: f32, colour: [u8; 4]) {
    let mul = colour.map(|c| f32::from(c) / 255.0);
    match i {
        Icon::Item(id) => {
            if let Some(cache) = w.and_then(|w| w.item_info.as_deref()) {
                super::super::items::upload_icons(gu, cache, std::iter::once(id));
                let name = crate::render::invwin::icon_texture(id);
                if gu.has_image(&name) {
                    gu.image_rect(&name, x, y, s, s, mul);
                }
            }
        }
        Icon::Status(id) => {
            if let Some(name) = gu.status_icon(false, id) {
                gu.image_rect(&name, x, y, s, s, mul);
            }
        }
        Icon::Spell(_) | Icon::Ability(_) => {
            let Some(w) = w else { return };
            // The modern UI's art for the spell, as its lists draw it.
            if w.modern
                && i.picture(w)
                    .and_then(super::super::modern::action_art)
                    .is_some_and(|art| pack_icon(gu, art, x, y, s, mul))
            {
                return;
            }
            if let Some((lib, k)) = i.sprite(w) {
                let (sw, sh) = (gu.sprite_width(lib, k), gu.sprite_height(lib, k));
                if sw > 0.0 && sh > 0.0 {
                    gu.sprite_scaled(lib, k, x, y, s / sw, s / sh, tint(colour));
                }
            }
        }
    }
}

/// The modern pack's sprite `id` at (x, y), `s` square, uploaded on first use; false when the pack has none.
fn pack_icon(gu: &mut GameUi, id: &str, x: f32, y: f32, s: f32, mul: [f32; 4]) -> bool {
    let key = format!("modern {id}");
    if !gu.has_image(&key) {
        let Some((iw, ih, rgba)) = super::super::modern::sprite_pixels(id) else { return false };
        gu.image(&key, iw, ih, rgba);
    }
    gu.image_rect(&key, x, y, s, s, mul);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Key as K;

    fn win() -> Window {
        let mut w = Window::new("w");
        w.elems = vec![
            Elem::Text { id: String::new(), text: "Hello".into(), dim: false, colour: None },
            Elem::Buttons { id: "go".into(), label: "Go".into(), items: vec!["A".into(), "B".into()], marked: None },
            Elem::Buttons {
                id: "pick".into(),
                label: "Pick".into(),
                items: vec!["X".into(), "Y".into()],
                marked: Some(0),
            },
            Elem::Slider { id: "s".into(), label: "S".into(), n: 3, at: 0, shown: "0".into() },
            Elem::List {
                id: "l".into(),
                rows: ["r1", "r2", "r3"].map(Row::text).to_vec(),
                columns: Vec::new(),
                at: 0,
                top: 0,
                height: 2,
            },
        ];
        w.open = true;
        w.focus = true;
        w.settle();
        w
    }

    #[test]
    fn keys() {
        let mut w = win();
        // The first stop is the Go buttons.
        w.key(K::ArrowRight);
        assert_eq!(w.key(K::Enter).0, vec![Event::Press { id: "go".into(), index: 1 }]);
        // Options set once.
        w.key(K::ArrowDown);
        w.key(K::ArrowRight);
        assert_eq!(w.key(K::Enter).0, vec![Event::Set { id: "pick".into(), index: 1 }]);
        assert!(w.key(K::Enter).0.is_empty());
        // The slider edits after Enter.
        w.key(K::ArrowDown);
        w.key(K::Enter);
        assert_eq!(w.key(K::ArrowRight).0, vec![Event::Set { id: "s".into(), index: 1 }]);
        w.key(K::Enter);
        // The list moves inside, scrolls, and selects.
        w.key(K::ArrowDown);
        w.key(K::ArrowDown);
        w.key(K::ArrowDown);
        let Elem::List { at, top, .. } = &w.elems[4] else { panic!() };
        assert_eq!((*at, *top), (2, 1));
        assert_eq!(w.key(K::Enter).0, vec![Event::Select { id: "l".into(), index: 2 }]);
        // Past its end the cursor wraps to the first stop.
        w.key(K::ArrowDown);
        assert_eq!(w.at, 1);
        assert_eq!(w.key(K::Escape).0, vec![Event::Close]);
        assert!(!w.open);
    }

    /// A dropdown: Enter opens its list on the item held, Up / Down move, Enter picks, Escape closes it unchanged; a
    /// click on its field opens and closes it, one on an item picks it, one off it closes it.
    #[test]
    fn dropdown() {
        let mut w = Window::new("w");
        let items = vec!["Spell".to_string(), "Job ability".into(), "Item".into()];
        w.elems = vec![Elem::Choice { id: "t".into(), label: "Type".into(), items, at: 0, drop: true }];
        (w.open, w.focus) = (true, true);
        w.settle();
        w.key(K::Enter);
        assert_eq!((w.drop_open(), w.drop_at), (Some(0), 0));
        w.key(K::ArrowUp);
        assert_eq!(w.drop_at, 2);
        assert_eq!(w.key(K::Enter).0, vec![Event::Set { id: "t".into(), index: 2 }]);
        assert_eq!(w.drop_open(), None);
        w.key(K::Enter);
        w.key(K::ArrowDown);
        assert!(w.key(K::Escape).0.is_empty());
        assert!(w.open && w.drop_open().is_none());
        // Left / Right still turn it, as a choice.
        assert_eq!(w.key(K::ArrowRight).0, vec![Event::Set { id: "t".into(), index: 0 }]);
        w.click_button(0, 0);
        assert_eq!(w.drop_open(), Some(0));
        w.click_button(0, 0);
        assert_eq!(w.drop_open(), None);
        w.click_button(0, 0);
        w.drop_hover(0, 1);
        assert_eq!(w.drop_pick(0, 1).0, vec![Event::Set { id: "t".into(), index: 1 }]);
        w.click_button(0, 0);
        w.drop_close(0);
        assert_eq!(w.drop_open(), None);
    }

    fn key_ev(k: K) -> egui::Event {
        egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() }
    }

    #[test]
    fn pages_sections_and_typing() {
        let mut w = Window::new("w");
        w.elems = vec![
            Elem::Tabs { id: "t".into(), items: vec!["One".into(), "Two".into()], at: 0 },
            Elem::Input {
                id: "name".into(),
                label: "Name".into(),
                text: "ab".into(),
                max: 4,
                filter: false,
                key: false,
            },
            Elem::Section { id: "sec".into(), label: "More".into(), open: false },
            Elem::Choice { id: "c".into(), label: "C".into(), items: vec!["x".into(), "y".into()], at: 0, drop: false },
            Elem::Colour { id: "col".into(), label: "Col".into(), value: [0, 0, 0, 255], alpha: false },
        ];
        w.meta = vec![
            Meta::default(),
            Meta { page: Some(0), ..Default::default() },
            Meta { page: Some(1), ..Default::default() },
            Meta::default(),
            Meta::default(),
        ];
        (w.open, w.focus) = (true, true);
        w.settle();
        // Page one: the tabs and the line; the section is on page two.
        assert_eq!(w.stops(), vec![0, 1]);
        assert_eq!(w.key(K::ArrowRight).0, vec![Event::Set { id: "t".into(), index: 1 }]);
        // Page two: the folded section hides what is under it.
        assert_eq!(w.stops(), vec![0, 2]);
        w.key(K::ArrowDown);
        assert_eq!(w.key(K::Enter).0, vec![Event::Toggle { id: "sec".into(), open: true }]);
        assert_eq!(w.stops(), vec![0, 2, 3, 4]);
        // An element on another page than the section is not under it.
        let mut v = w.clone();
        v.elems.push(Elem::Input {
            id: "p2".into(),
            label: String::new(),
            text: String::new(),
            max: 4,
            filter: false,
            key: false,
        });
        v.meta.push(Meta { page: Some(0), ..Default::default() });
        (v.at, v.elems[0]) = (0, Elem::Tabs { id: "t".into(), items: vec!["One".into(), "Two".into()], at: 0 });
        assert_eq!(v.stops(), vec![0, 1, 5]);
        // The choice turns; the colour edits a channel at a time and Escape undoes it.
        w.key(K::ArrowDown);
        assert_eq!(w.key(K::ArrowLeft).0, vec![Event::Set { id: "c".into(), index: 1 }]);
        w.key(K::ArrowDown);
        w.key(K::Enter);
        w.key(K::ArrowDown);
        assert_eq!(w.key(K::ArrowRight).0, vec![Event::Colour { id: "col".into(), value: [0, 8, 0, 255] }]);
        assert_eq!(w.key(K::Escape).0, vec![Event::Colour { id: "col".into(), value: [0, 0, 0, 255] }]);
        // Back to page one and the line: typed up to its limit, edited, ended with Enter.
        w.at = 0;
        w.key(K::ArrowLeft);
        w.key(K::ArrowDown);
        w.key(K::Enter);
        assert!(w.typing());
        let (ev, _) = w.text(&[egui::Event::Text("cde".into())]);
        assert_eq!(ev, vec![Event::Text { id: "name".into(), text: "abcd".into(), done: false }]);
        let (ev, _) = w.text(&[key_ev(K::Home), key_ev(K::Delete), key_ev(K::Enter)]);
        assert_eq!(ev, vec![Event::Text { id: "name".into(), text: "bcd".into(), done: true }]);
        assert!(!w.typing());
        // Escape puts the line back as it was.
        w.key(K::Enter);
        w.text(&[key_ev(K::Backspace)]);
        let (ev, _) = w.text(&[key_ev(K::Escape)]);
        assert_eq!(ev, vec![Event::Text { id: "name".into(), text: "bcd".into(), done: false }]);
        let Elem::Input { text, .. } = &w.elems[1] else { panic!() };
        assert_eq!(text, "bcd");
    }

    #[test]
    fn filter_window_walk() {
        let input = |id: &str, filter| Elem::Input {
            id: id.into(),
            label: id.into(),
            text: String::new(),
            max: 9,
            filter,
            key: false,
        };
        let mut w = Window::new("w");
        w.elems = vec![
            Elem::Text { id: String::new(), text: "Hi".into(), dim: false, colour: None },
            input("filter", true),
            Elem::List {
                id: "l".into(),
                rows: ["r1", "r2"].map(Row::text).to_vec(),
                columns: Vec::new(),
                at: 0,
                top: 0,
                height: 2,
            },
            Elem::Text { id: "count".into(), text: "2".into(), dim: true, colour: None },
            input("btn", false),
            Elem::Buttons { id: "go".into(), label: String::new(), items: vec!["A".into(), "B".into()], marked: None },
        ];
        (w.open, w.focus) = (true, true);
        w.settle();
        let mut seen = vec![w.at];
        for _ in 0..5 {
            w.filter_text(&[key_ev(K::ArrowDown)]);
            seen.push(w.at);
        }
        assert_eq!(seen, vec![1, 2, 2, 4, 5, 1]);
        // Tab moves as Down, Shift+Tab as Up.
        let tab = |shift| egui::Event::Key {
            key: K::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers { shift, ..Default::default() },
        };
        w.filter_text(&[tab(false), tab(false), tab(false)]);
        assert_eq!(w.at, 4);
        w.filter_text(&[tab(true)]);
        assert_eq!(w.at, 2);
        // Typing goes into the line under the cursor, else the filter.
        w.filter_text(&[tab(false), egui::Event::Text("Ichi".into())]);
        w.filter_text(&[tab(false), egui::Event::Text("u".into())]);
        let text = |w: &Window, i| match &w.elems[i] {
            Elem::Input { text, .. } => text.clone(),
            _ => panic!(),
        };
        assert_eq!((text(&w, 1), text(&w, 4)), ("u".to_string(), "Ichi".to_string()));
    }

    #[test]
    fn key_element() {
        let mut w = Window::new("w");
        w.elems = vec![Elem::Input {
            id: "k".into(),
            label: "Key".into(),
            text: "1".into(),
            max: 64,
            filter: false,
            key: true,
        }];
        (w.open, w.focus) = (true, true);
        w.settle();
        let ctrl_q = egui::Event::Key {
            key: K::Q,
            physical_key: Some(K::Q),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers { ctrl: true, ..Default::default() },
        };
        let name = |e: &egui::Event| super::super::input::combo_of(e, None);
        // Not listening: nothing taken.
        assert!(w.listen(std::slice::from_ref(&ctrl_q), name).0.is_empty());
        w.key(K::Enter);
        assert!(w.listening() && w.takes_text());
        assert_eq!(
            w.listen(&[ctrl_q], name).0,
            vec![Event::Text { id: "k".into(), text: "ctrl+q".into(), done: true }]
        );
        assert!(!w.listening());
        // Escape stops without a change; Backspace clears.
        w.key(K::Enter);
        assert!(w.listen(&[key_ev(K::Escape)], name).0.is_empty());
        w.key(K::Enter);
        assert_eq!(
            w.listen(&[key_ev(K::Backspace)], name).0,
            vec![Event::Text { id: "k".into(), text: String::new(), done: true }]
        );
        // Stopped by a click elsewhere before a key press: no change.
        w.key(K::Enter);
        assert!(w.listening());
        assert!(w.end_edit().is_empty());
        assert!(!w.listening());
        assert_eq!(super::super::input::combo_label("ctrl+shift+numpad+"), "Ctrl+Shift+Num +");
        assert_eq!(super::super::input::combo_label("alt+f1"), "Alt+F1");
    }

    /// The help under the cursor is a list row's tip (a spell's description) as the cursor moves down the list; a
    /// row without one shows the list's tip.
    #[test]
    fn list_row_tip_in_help() {
        let mut w = Window::new("w");
        let row = |s: &str, tip: &str| Row { cells: vec![s.into()], tip: tip.into(), ..Default::default() };
        let rows = vec![row("Dia", "Lowers defense."), row("Cure", "")];
        w.elems = vec![Elem::List { id: "l".into(), rows, columns: Vec::new(), at: 0, top: 0, height: 5 }];
        w.meta = vec![Meta { tip: "Pick one.".into(), page: None }];
        (w.open, w.focus) = (true, true);
        w.settle();
        assert_eq!(w.help_now(), "Lowers defense.");
        w.key(K::ArrowDown);
        assert_eq!(w.help_now(), "Pick one.");
    }

    /// New elements (a mod's other layout): the cursor stays on its element by id, else starts on the first stop.
    #[test]
    fn set_elems_keeps_cursor_by_id() {
        let mut w = win();
        (w.open, w.focus) = (true, true);
        w.settle();
        w.key(K::ArrowDown);
        let at = w.at;
        assert_eq!(w.elems[at].id(), Some("pick"));
        let (elems, meta) = (w.elems.clone(), w.meta.clone());
        w.set_elems(elems, meta);
        assert_eq!(w.at, at);
        let other = vec![
            Elem::Text { id: String::new(), text: "Other".into(), dim: false, colour: None },
            Elem::Buttons { id: "save".into(), label: String::new(), items: vec!["Save".into()], marked: None },
            Elem::Buttons { id: "go".into(), label: String::new(), items: vec!["Go".into()], marked: None },
        ];
        w.set_elems(other, vec![Meta::default(); 3]);
        assert_eq!(w.elems[w.at].id(), Some("save"));
    }
}
