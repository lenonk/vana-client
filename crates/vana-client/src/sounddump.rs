//! `sound-dump` (decode to WAV) and `audio-test` (play through the device).

use crate::audio::{self, format};
use crate::config::Config;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub fn dump(id: Option<u32>, music: bool, file: Option<PathBuf>, out: &Path, seconds: Option<f32>) -> Result<()> {
    let dir = Config::load().ffxi_dir();
    let path = match (file, id) {
        (Some(f), _) => f,
        (None, Some(i)) if music => format::music_path(&dir, i).with_context(|| format!("no music{i:03}.bgw"))?,
        (None, Some(i)) => format::se_path(&dir, i).with_context(|| format!("no se{i:06}.spw"))?,
        _ => anyhow::bail!("give --id or --file"),
    };
    let bytes: Arc<[u8]> = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?.into();
    let mut dec = format::Decoder::new(bytes)?;
    let h = dec.header.clone();
    println!(
        "{}: {} id {} {:?}, {} ch, {} Hz, {} frames ({:.2} s), loop {:?}, volume {}, flags {:#x}",
        path.display(),
        if h.music { "BGMStream" } else { "SeWave" },
        h.id,
        h.codec,
        h.channels,
        h.rate,
        h.frames(),
        h.frames() as f64 / h.rate as f64,
        h.loop_frame(),
        h.volume,
        h.flags
    );
    let total = match seconds {
        Some(s) => (s as f64 * h.rate as f64) as u64,
        None => {
            dec.loops = false;
            h.frames()
        }
    };
    let ch = h.channels as usize;
    let mut samples = vec![0i16; total as usize * ch];
    let got = dec.read(&mut samples);
    samples.truncate(got * ch);
    let looped = h.loop_frame().map(|l| (l, h.frames()));
    format::write_wav(out, &samples, ch as u16, h.rate, looped)?;
    let peak = samples.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
    let rms = (samples.iter().map(|&s| (s as f64).powi(2)).sum::<f64>() / samples.len().max(1) as f64).sqrt();
    println!("wrote {} ({} frames, peak {peak}, rms {rms:.0})", out.display(), got);
    Ok(())
}

pub fn audio_test(id: u32, seconds: f32, se: bool, fade: i32, device: Option<String>, list: bool) -> Result<()> {
    if list {
        for d in audio::output::list() {
            println!("{d}");
        }
        return Ok(());
    }
    let cfg = Config::load();
    let device = device.unwrap_or(cfg.audio_device.clone());
    let mut a = audio::Audio::open(cfg.ffxi_dir(), Some(&device), 1.0, 1.0).context("no audio output")?;
    if se {
        a.play_se(id, 127);
    } else {
        a.change_music(id, 0x7f, 0x7f, 0, 0x1e, false);
    }
    let start = Instant::now();
    let mut last = start;
    let mut faded = false;
    while start.elapsed().as_secs_f32() < seconds {
        std::thread::sleep(Duration::from_millis(16));
        let now = Instant::now();
        a.update((now - last).as_secs_f32());
        last = now;
        if !se && !faded && seconds - start.elapsed().as_secs_f32() < fade as f32 / 60.0 {
            a.stop_music(fade);
            faded = true;
        }
    }
    use std::sync::atomic::Ordering;
    println!("played {} for {seconds} s (mixer saw music {:?})", if se { "effect" } else { "music" }, a.status.bgm.load(Ordering::Relaxed).checked_sub(1));
    Ok(())
}
