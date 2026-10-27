//! The modern top bar: retail's titlewin, helpwind and netstat in their places, run by their rules (client/hudtop.rs
//! steps them and fills a TopView), drawn in the pack's frames on a layer that takes no input.

use super::super::hudtop::TopView;
use super::{Modern, STRONG, paint};
use egui::{Align2, Color32, Context, FontFamily, FontId, Id, LayerId, Order, Painter, Rect, pos2, vec2};

/// Edge margin, bar height, the side boxes' width and the gaps, in points (retail: 16, 30, 112, 2 px).
const PAD: f32 = 16.0;
const H: f32 = 46.0;
const SIDE: f32 = 200.0;
const NET: f32 = 168.0;
const GAP: f32 = 4.0;
/// The legends' and the network numbers' size: small, but readable at any Interface Size.
const SMALL: f32 = 14.0;

/// The help line's marquee (FUN_10217640): the text, ticks left before it moves, its offset, the last frame's time.
#[derive(Default)]
pub struct Marquee {
    text: String,
    wait: f32,
    off: f32,
    last: Option<f64>,
}

pub fn draw(m: &Modern, ctx: &Context, v: &TopView) {
    let p = ctx.layer_painter(LayerId::new(Order::Background, Id::unique("modern top bar")));
    let screen = ctx.content_rect();
    let frame = |r: Rect| {
        if v.alpha > 0.0 {
            p.add(paint::faded(super::borders::window(m, "window_normal", r), v.alpha));
        }
    };
    // titlewin: the title, clipped to the box; the content is drawn whatever the fade.
    if v.title_up {
        let r = Rect::from_min_size(pos2(screen.min.x + PAD, screen.min.y + PAD), vec2(SIDE, H));
        frame(r);
        let font = FontId::new(m.theme.ui, FontFamily::Name(STRONG.into()));
        p.with_clip_rect(r.shrink2(vec2(10.0, 0.0))).text(pos2(r.min.x + 12.0, r.center().y), Align2::LEFT_CENTER, &v.title, font, m.theme.color("gold"));
    }
    // helpwind between the two, its "Help" legend only while fully shown.
    let r = Rect::from_min_max(pos2(screen.min.x + PAD + SIDE + GAP, screen.min.y + PAD), pos2(screen.max.x - PAD - NET - GAP, screen.min.y + PAD + H));
    frame(r);
    if v.shown {
        legend(&p, m, r, "Help", true);
    }
    help(m, ctx, &p, r, v);
    if v.net_up {
        let r = Rect::from_min_size(pos2(screen.max.x - PAD - NET, screen.min.y + PAD), vec2(NET, H));
        frame(r);
        if v.alpha > 0.0 {
            legend(&p, m, r, "Network", false);
        }
        network(&p, m, r, v);
    }
}

/// A small caption on a box's top border, at its right or left end (retail's "Help" and "Network" sprites).
fn legend(p: &Painter, m: &Modern, r: Rect, text: &str, right: bool) {
    let font = FontId::proportional(SMALL);
    let w = p.layout_no_wrap(text.to_string(), font.clone(), Color32::WHITE).size().x;
    let at = pos2(if right { r.max.x - 28.0 - w } else { r.min.x + 28.0 }, r.min.y + 1.0);
    p.rect_filled(Rect::from_min_size(at - vec2(6.0, 9.0), vec2(w + 12.0, 18.0)), 4.0, m.theme.color("charcoal"));
    p.text(at, Align2::LEFT_CENTER, text, font, m.theme.color("muted"));
}

/// The help line: the busy strip in place of the text while a request is pending, else the text, which waits 60 ticks
/// and then scrolls a point a tick when wider than its box.
fn help(m: &Modern, ctx: &Context, p: &Painter, r: Rect, v: &TopView) {
    let mut q = m.marquee.borrow_mut();
    let now = ctx.input(|i| i.time);
    let ticks = q.last.map_or(0.0, |t| ((now - t) as f32 * 60.0).min(30.0));
    q.last = Some(now);
    let shown = if v.busy.is_some() { q.text.clone() } else { v.help.clone() };
    if shown != q.text {
        (q.text, q.wait, q.off) = (shown, 60.0, 0.0);
    }
    let cy = r.center().y;
    if let Some(f) = v.busy {
        // keytops3 165..169's five steps as five dots, the lit one walking.
        for k in 0..5 {
            let c = if k == f { m.theme.color("gold") } else { m.theme.color("bronze").gamma_multiply(0.4) };
            p.circle_filled(pos2(r.center().x + (k as f32 - 2.0) * 12.0, cy), 3.5, c);
        }
        return;
    }
    if q.text.is_empty() {
        return;
    }
    let bx = r.shrink2(vec2(12.0, 0.0));
    let font = FontId::proportional(m.theme.ui);
    let col = m.theme.color("text");
    let clip = p.with_clip_rect(bx);
    let width = p.layout_no_wrap(q.text.clone(), font.clone(), col).size().x;
    if width <= bx.width() {
        clip.text(pos2(bx.min.x, cy), Align2::LEFT_CENTER, &q.text, font, col);
        return;
    }
    let looped = format!("{}    ", q.text);
    let cycle = p.layout_no_wrap(looped.clone(), font.clone(), col).size().x.max(1.0);
    if q.wait > 0.0 {
        q.wait -= ticks;
    } else {
        q.off = (q.off + ticks) % cycle;
    }
    clip.text(pos2(bx.min.x - q.off, cy), Align2::LEFT_CENTER, &looped, font.clone(), col);
    clip.text(pos2(bx.min.x - q.off + cycle, cy), Align2::LEFT_CENTER, &looped, font, col);
}

/// netstat: the meter (four bars by level, the lit step walking as retail's frames do), the percentage, S / R.
fn network(p: &Painter, m: &Modern, r: Rect, v: &TopView) {
    let (step, level) = v.meter;
    // Good green, fair yellow, poor red.
    let lit = match level {
        3 => Color32::from_rgb(0x58, 0xd0, 0x68),
        2 => Color32::from_rgb(0xd8, 0xb0, 0x40),
        1 => m.theme.color("hp"),
        _ => m.theme.color("muted"),
    };
    let x0 = r.min.x + 14.0;
    for k in 0..4 {
        let h = 7.0 + k as f32 * 5.0;
        let bar = Rect::from_min_size(pos2(x0 + k as f32 * 7.0, r.center().y + 11.0 - h), vec2(5.0, h));
        let on = level > 0 && (k as u8) < level + 1;
        let c = if !on {
            m.theme.color("bronze").gamma_multiply(0.3)
        } else if k == step {
            lit
        } else {
            lit.gamma_multiply(0.6)
        };
        p.rect_filled(bar, 1.0, c);
    }
    let small = FontId::proportional(SMALL);
    p.text(pos2(x0 + 36.0, r.center().y + 2.0), Align2::LEFT_CENTER, format!("{}%", v.pct), FontId::proportional(m.theme.ui + 2.0), m.theme.color("text"));
    // S and R in a column, their numbers right-aligned beside them.
    let (lx, nx) = (r.max.x - 66.0, r.max.x - 14.0);
    for (k, (label, n)) in [("S", v.sent), ("R", v.recv)].into_iter().enumerate() {
        let y = r.center().y - 6.0 + 16.0 * k as f32;
        p.text(pos2(lx, y), Align2::LEFT_CENTER, label, small.clone(), m.theme.color("muted"));
        p.text(pos2(nx, y), Align2::RIGHT_CENTER, n.to_string(), small.clone(), m.theme.color("muted"));
    }
}
