//! `event-dump`: disassembles a zone's event scripts (dat/event.rs) with dialogue text inlined.

use crate::dat::{DatIndex, event};
use anyhow::Result;

pub fn run(index: &DatIndex, zone: u16, actor: Option<u32>, only_event: Option<u16>) -> Result<()> {
    let blocks = event::load(index, zone)?;
    let dialog = crate::dat::dialog::Dialog::load(index, zone).ok();
    let text = |id: u32| dialog.as_ref().and_then(|d| d.get(id));
    println!("zone {zone}: event file {} with {} blocks", event::file_id(zone), blocks.len());
    for b in &blocks {
        if actor.is_some_and(|a| a != b.actor && a != (b.actor & 0xfff)) {
            continue;
        }
        if only_event.is_some_and(|e| !b.events.iter().any(|(id, _)| *id == e)) {
            continue;
        }
        println!("\nactor {:08x}: {} events, {} constants, {} code bytes", b.actor, b.events.len(), b.consts.len(), b.code.len());
        println!("  events: {}", b.events.iter().map(|(id, at)| format!("{id:04x}@{at:04x}")).collect::<Vec<_>>().join(" "));
        for l in event::disasm(b, &text) {
            println!("{l}");
        }
    }
    Ok(())
}

/// `event-ops`: the opcode table (name, length, wait/stop flags, description).
pub fn ops() {
    for (i, op) in crate::dat::event_ops::OPS.iter().enumerate() {
        let Some(op) = op else { continue };
        let len = match op.len {
            event::Len::Fixed(n) => n.to_string(),
            event::Len::Sub { by, .. } => format!("by byte {by}"),
        };
        println!("{i:02x} {:<20} len {len:<10} {}{} {}", op.name, if op.waits { "W" } else { "-" }, if op.stop { "S" } else { "-" }, op.desc);
    }
}

/// Event camera file of opcode 0x45 (FUN_100b26f0 with base 0x77f0, FUN_100b26c0 remaps n >= 300).
pub fn camera_file(n: u32) -> u32 {
    let n = match n {
        600.. => n + 0x9adb,
        300..600 => n + 0x6551,
        _ => n,
    };
    0x77f0 + n
}

/// `cs-shot`: renders the camera paths an event scheduler plays (event camera file `n`, tag `sched`) in a zone.
pub fn cs_shot(zone: u32, n: u32, sched: &str, steps: u32, out_dir: &std::path::Path, width: u32, height: u32) -> Result<()> {
    use crate::dat::title::{Op, Title, fov_y, sample};
    let index = DatIndex::open(&crate::config::Config::load().ffxi_dir())?;
    let file = camera_file(n);
    let evte = Title::parse(&index.read(file)?)?;
    let ops = evte.scheduler(sched)?;
    std::fs::create_dir_all(out_dir)?;
    let scene = crate::load_zone_scene(zone)?;
    let (mut cams, mut names) = (Vec::new(), Vec::new());
    for op in ops {
        println!("op {op:?}");
        if let Op::Camera { path, ticks, .. } = op {
            let p = evte.path(path)?;
            println!("camera {path}: {} keys, ease {}, flags {:#x}, attach {:#x}, {ticks} ticks", p.keys.len(), p.ease, p.flags, p.attach);
            for i in 0..steps.max(1) {
                let t = if steps <= 1 { 0.0 } else { i as f32 / (steps - 1) as f32 };
                let k = sample(p, t);
                println!("  t {t:.2}: eye {} target {} fov_y {:.1}", k.eye, k.target, fov_y(k.focal).to_degrees());
                cams.push(crate::render::Camera { eye: k.eye, target: k.target, up: glam::Vec3::NEG_Y, fov_y: fov_y(k.focal) });
                names.push(out_dir.join(format!("{sched}_{path}_{i}.png")));
            }
        }
    }
    let frames: Vec<(&crate::render::Camera, &std::path::Path)> = cams.iter().zip(names.iter()).map(|(c, n)| (c, n.as_path())).collect();
    crate::render::render_pngs(&scene, &frames, width, height)?;
    println!("file {file}: wrote {} frames to {}", frames.len(), out_dir.display());
    Ok(())
}
