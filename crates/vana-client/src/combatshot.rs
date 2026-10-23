//! `combat-shot`: LSB-built 0x028 / 0x029 / 0x02D packets replayed through the client's parsers and
//! battle message table into the chat log window, with the target window and the action menu, as
//! the in-game HUD draws them (client::world_hud).

use crate::client::combat::{self, cmd};
use crate::client::net::{battle_message, battle_message2};
use crate::config::Config;
use crate::dat::DatIndex;
use crate::dat::battle::{BattleText, Ctx, Relation, Who};
use crate::dat::font::{Font, Quality, encode_sjis};
use crate::dat::ui::{MENU_US, UiFile, UiImage, WINDOW_STYLE_BASE, norm_name};
use crate::net::action::{self, Action, ActionResult, ActionTarget, Side, category};
use crate::net::map::session::SubPacket;
use crate::render::screens::{HudState, hud_screen, push_window, window_rect};
use crate::render::ui::render_png;
use crate::uishot::{load_ui, load_ui_full};
use anyhow::{Context, Result};
use std::path::Path;

const ATLAS: &str = "\0font atlas";
const ME: u32 = 0x0001_0203;
const MOB: u32 = 0x0100_4011;

fn font_file(font: &Font) -> UiFile {
    let mut f = UiFile::default();
    let a = &font.atlas;
    f.images.insert(ATLAS.into(), UiImage { width: a.width, height: a.height, rgba: a.rgba.clone() });
    for (name, t) in &font.textures {
        f.images.insert(norm_name(name.as_bytes()), UiImage { width: t.width, height: t.height, rgba: t.rgba.clone() });
    }
    f
}

fn sub(data: Vec<u8>) -> SubPacket {
    SubPacket { id: data[0] as u16 | ((data[1] as u16 & 1) << 8), sync: 0, data }
}

/// GP_SERV_COMMAND_BATTLE_MESSAGE (0x029) / _MESSAGE2 (0x02D) laid out as LSB writes them.
fn message_packet(id: u8, cas: u32, tar: u32, data: [u32; 2], msg: u16) -> SubPacket {
    let mut b = vec![0u8; 0x1c];
    b[0] = id;
    b[1] = (0x1c / 4) << 1;
    b[4..8].copy_from_slice(&cas.to_le_bytes());
    b[8..12].copy_from_slice(&tar.to_le_bytes());
    let d = if id == 0x29 { 12 } else { 0x10 };
    b[d..d + 4].copy_from_slice(&data[0].to_le_bytes());
    b[d + 4..d + 8].copy_from_slice(&data[1].to_le_bytes());
    b[0x18..0x1a].copy_from_slice(&msg.to_le_bytes());
    sub(b)
}

/// The replayed fight, as chat lines with their colours.
fn replay(bt: &BattleText, mob: &str) -> Vec<(String, [u8; 4])> {
    let who = |id: u32| Some(if id == ME { Who { name: "Tuxedo".into(), pc: true, ..Default::default() } } else { Who { name: mob.into(), ..Default::default() } });
    let rel = |a: u32, t: u32| if t == ME { Relation::TargetIsMe } else if a == ME { Relation::CasterIsMe } else { Relation::Other };
    let say = |id: u16, a: u32, t: u32, work: [i32; 5]| combat::render(bt, id, rel(a, t), &Ctx { actor: who(a), target: who(t), me: "Tuxedo".into(), work }, &Default::default());
    let mut lines = Vec::new();
    let mut act = |a: Action| {
        let d = action::parse(&sub(action::pack(&a)).data);
        for (id, t, work) in combat::action_messages(&d) {
            lines.extend(say(id, d.actor, t, work));
        }
    };
    let atk0 = u32::from_le_bytes(*b"atk0");
    let res = |m: u16, v: u32| ActionResult { kind: 1, message: m, value: v, ..Default::default() };
    act(Action { actor: ME, category: category::BASIC_ATTACK, arg: atk0, info: 0, targets: vec![ActionTarget { id: MOB, results: vec![res(1, 12)] }] });
    let proc = ActionResult { proc: Some(Side { kind: 1, info: 0, value: 3, message: 163 }), ..res(1, 7) };
    act(Action { actor: MOB, category: category::BASIC_ATTACK, arg: atk0, info: 0, targets: vec![ActionTarget { id: ME, results: vec![proc, ActionResult { miss: 1, ..res(15, 0) }] }] });
    act(Action { actor: ME, category: category::BASIC_ATTACK, arg: atk0, info: 0, targets: vec![ActionTarget { id: MOB, results: vec![res(1, 9)] }] });
    let mut lines = lines;
    for (p, two) in [(message_packet(0x29, ME, MOB, [0, 0], 6), false), (message_packet(0x2d, ME, ME, [50, 0], 8), true), (message_packet(0x2d, ME, ME, [2, 0], 9), true)] {
        let m = if two { battle_message2(&p) } else { battle_message(&p) };
        lines.extend(say(m.message, m.actor, m.target, [m.data[0], m.data[1], 0, 0, 0]));
    }
    lines
}

pub fn combat_shot(out: &Path, mob: &str, menu_layout: usize, width: u32, height: u32) -> Result<()> {
    let cfg = Config::load();
    let index = DatIndex::open(&cfg.ffxi_dir())?;
    let dll = std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll"))?;
    let menu = load_ui_full(&index, MENU_US)?;
    let style_file = load_ui(&index, WINDOW_STYLE_BASE)?;
    let style = style_file.libs.first().context("window style file has no sprite library")?;
    let font = Font::load(&index, &dll, 1, Quality::Compressed)?;
    let bt = BattleText::load(&index, &dll)?;
    let (w, h) = (width as f32, height as f32);
    let st = HudState { party: vec![(0.62, 1.0)], party_anim: Vec::new(), target_anim: None, party_window: "menu    ptw0".into(), party_hand: None, party_away: Vec::new(), party_marks: Vec::new(), target: Some(Some(0.4)), menu_selected: None, menu_page: 0, menu_focused: true, chat_log: true };
    let (mut b, _) = hud_screen(&menu, style, w, h, &st);

    // Chat log as world_hud draws it: newest at the bottom, 16 px rows from bottom - 22, x + 4.
    let r = window_rect(&menu, "menu    logwindo", w, h).context("no logwindo")?;
    // Wrapped at spaces to the window width - 16, as GameUi::wrap does in the client.
    let mut lines = Vec::new();
    for (line, colour) in replay(&bt, mob) {
        let mut cur = String::new();
        for word in line.split(' ') {
            let next = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
            if !cur.is_empty() && font.text_width(&encode_sjis(&next)) as f32 > r[2] - 16.0 {
                lines.push((std::mem::take(&mut cur), colour));
                cur = word.to_string();
            } else {
                cur = next;
            }
        }
        lines.push((cur, colour));
    }
    let rows = ((r[3] - 12.0) / 16.0).max(1.0) as usize;
    for (k, (line, colour)) in lines.iter().rev().take(rows).enumerate() {
        println!("{:02x?} {line}", colour);
        let mut quads = Vec::new();
        font.layout_line(&encode_sjis(line), (r[0] + 4.0) as i32, (r[1] + r[3] - 22.0 - k as f32 * 16.0) as i32, *colour, [1.0, 1.0], &mut quads);
        b.push_text(&quads, ATLAS);
    }

    // Action menu for a monster target (layout 4) or another layout, first row highlighted.
    let items = combat::menu_items(menu_layout, Some(MOB));
    let name = combat::window_name(items.len());
    let m = menu.menu(&name).with_context(|| format!("no {name}"))?;
    push_window(&mut b, &menu, style, m, w, h);
    let wr = window_rect(&menu, &name, w, h).context("window rect")?;
    let keytops = menu.lib("menu    keytops3").context("no keytops3")?;
    for (k, (c, row)) in items.iter().zip(combat::rows(wr[0], wr[1], items.len())).enumerate() {
        let (_, sprite) = combat::label(*c);
        b.push_sprite(&menu, keytops, sprite, row[0], row[1], 1.0, 1.0, if k == 0 { 0x8080_5020 } else { 0x8080_8080 }, 0);
        if k == 0
            && let Some(anc) = menu.lib("anc     anc_s")
        {
            b.push_sprite(&menu, anc, 0, row[0] - 2.0, row[1], 1.0, 1.0, 0x8080_8080, 0);
        }
    }
    println!("action menu {name}: {:?} (attack = {})", items, items.first() == Some(&cmd::ATTACK));
    let fonts = font_file(&font);
    render_png(&[&menu, &fonts], &b, width, height, [0.30, 0.36, 0.28], out)?;
    println!("wrote {}", out.display());
    Ok(())
}
