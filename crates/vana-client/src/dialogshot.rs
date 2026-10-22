//! `dialogue-shot`: an NPC line from a zone dialogue file in the chat log window (logwindo), as 0x036
//! prints it ("%s : " + text, FUN_100abbb0), with the kaipage prompt arrow; optionally the query menu.

use crate::config::Config;
use crate::dat::dialog::{Ctx, Dialog, expand};
use crate::dat::font::{Font, Quality, encode_sjis};
use crate::dat::ui::{MENU_US, UiFile, UiImage, WINDOW_STYLE_BASE, norm_name};
use crate::dat::DatIndex;
use crate::render::screens::{frame_rect, push_window};
use crate::render::ui::{UiBatch, render_png};
use crate::uishot::{load_ui, load_ui_full};
use anyhow::{Context, Result};
use std::path::Path;

const ATLAS: &str = "\0font atlas";
const WHITE: [u8; 4] = [0x80; 4];
/// Inferred: one font cell per log line, text inset 8 px (the HUD's chat-log slot).
const LINE: f32 = 16.0;
const INSET: f32 = 8.0;

/// Greedy word wrap of Shift-JIS text to `width` px; single-byte spaces are the break points.
fn wrap(font: &Font, line: &[u8], width: i32) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut cur: Vec<u8> = Vec::new();
    for word in line.split(|&b| b == b' ') {
        let mut next = cur.clone();
        if !next.is_empty() {
            next.push(b' ');
        }
        next.extend_from_slice(word);
        if !cur.is_empty() && font.text_width(&next) > width {
            out.push(std::mem::take(&mut cur));
            cur = word.to_vec();
        } else {
            cur = next;
        }
    }
    out.push(cur);
    out
}

fn text(b: &mut UiBatch, font: &Font, s: &[u8], x: f32, y: f32) -> f32 {
    let mut quads = Vec::new();
    let end = font.layout_line(s, x.round() as i32, y.round() as i32, WHITE, [1.0, 1.0], &mut quads);
    b.push_text(&quads, ATLAS);
    end as f32
}

/// The font atlas and gaiji textures as a UiFile, so render_png uploads them.
fn font_file(font: &Font) -> UiFile {
    let mut f = UiFile::default();
    let a = &font.atlas;
    f.images.insert(ATLAS.into(), UiImage { width: a.width, height: a.height, rgba: a.rgba.clone() });
    for (name, t) in &font.textures {
        f.images.insert(norm_name(name.as_bytes()), UiImage { width: t.width, height: t.height, rgba: t.rgba.clone() });
    }
    f
}

#[allow(clippy::too_many_arguments)]
pub fn dialogue_shot(out: &Path, zone: u16, id: u32, speaker: &str, choice: Option<u32>, frame: usize, width: u32, height: u32) -> Result<()> {
    let cfg = Config::load();
    let index = DatIndex::open(&cfg.ffxi_dir())?;
    let menu = load_ui_full(&index, MENU_US)?;
    let style_file = load_ui(&index, WINDOW_STYLE_BASE)?;
    let style = style_file.libs.first().context("window style file has no sprite library")?;
    let font = Font::load(&index, &std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll"))?, 1, Quality::Compressed)?;
    let dialog = Dialog::load(&index, zone)?;
    let ctx = Ctx { player: "Lenon".into(), speaker: speaker.into(), ..Default::default() };
    let (w, h) = (width as f32, height as f32);
    let mut b = UiBatch::default();

    let raw = dialog.raw(id).with_context(|| format!("no message {id} in zone {zone} ({} entries)", dialog.len()))?;
    println!("{id}: {}", dialog.get(id).unwrap_or_default());
    let msg = expand(&raw, &ctx);
    let log = menu.menu("menu    logwindo").context("no logwindo menu")?;
    let (x, y) = push_window(&mut b, &menu, style, log, w, h);
    let (_, _, fw, fh) = frame_rect(log, w, h, 1.0, 1.0);
    let kai = menu.lib("anc     kaipage").context("no kaipage library")?;
    // Inferred: the arrow sits after the text, so the wrap leaves room for its 10 px.
    let wrap_w = (fw - INSET * 2.0 - 12.0) as i32;
    let lines: Vec<Vec<u8>> = {
        let mut first = encode_sjis(&format!("{speaker} : "));
        first.extend_from_slice(msg.lines.first().map_or(&[][..], |l| l));
        let mut v = wrap(&font, &first, wrap_w);
        for l in msg.lines.iter().skip(1) {
            v.extend(wrap(&font, l, wrap_w));
        }
        v
    };
    // The log fills from the bottom: the newest line is the last row.
    let rows = ((fh - INSET) / LINE).floor() as usize;
    let shown = &lines[lines.len().saturating_sub(rows)..];
    let top = y + fh - INSET / 2.0 - shown.len() as f32 * LINE;
    let mut end = (x, top);
    for (k, l) in shown.iter().enumerate() {
        let ly = top + k as f32 * LINE;
        end = (text(&mut b, &font, l, x + INSET, ly), ly);
    }
    if msg.prompt {
        b.push_sprite(&menu, kai, frame % kai.sprites.len().max(1), end.0 + 2.0, end.1 + 3.0, 1.0, 1.0, 0x8080_8080, 0);
    }

    if let Some(cid) = choice {
        let raw = dialog.raw(cid).with_context(|| format!("no message {cid}"))?;
        println!("{cid}: {}", dialog.get(cid).unwrap_or_default());
        let q = expand(&raw, &ctx);
        let at = q.choice.unwrap_or(q.lines.len());
        let m = menu.menu("menu    query").context("no query menu")?;
        let (qx, qy) = push_window(&mut b, &menu, style, m, w, h);
        // Inferred: question lines from the window's top-left inset; options at the child rows (FUN_101477a0).
        for (k, l) in q.lines[..at].iter().enumerate() {
            text(&mut b, &font, l, qx + INSET, qy + 6.0 + k as f32 * LINE);
        }
        for (k, (row, opt)) in m.elements.iter().skip(1).zip(&q.lines[at..]).enumerate() {
            let (rx, ry) = (qx + row.x as f32, qy + row.y as f32);
            text(&mut b, &font, opt, rx + 4.0, ry);
            if k == 0
                && let Some(anc) = menu.lib("anc     anc_s")
            {
                b.push_sprite(&menu, anc, frame % anc.sprites.len().max(1), rx, ry, 1.0, 1.0, 0x8080_8080, 0);
            }
        }
    }

    let fonts = font_file(&font);
    render_png(&[&menu, &fonts], &b, width, height, [0.30, 0.36, 0.28], out)?;
    println!("wrote {}", out.display());

    // The arrow's six frames, 4x, for checking the animation.
    let mut strip = UiBatch::default();
    for f in 0..kai.sprites.len() {
        strip.push_sprite(&menu, kai, f, 8.0 + f as f32 * 56.0, 8.0, 4.0, 4.0, 0x8080_8080, 0);
    }
    let strip_path = out.with_extension("arrow.png");
    render_png(&[&menu], &strip, 8 + kai.sprites.len() as u32 * 56, 60, [0.15, 0.15, 0.2], &strip_path)?;
    println!("wrote {}", strip_path.display());
    Ok(())
}
