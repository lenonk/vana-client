//! The microphone: cpal input (or a test stand-in) to 48 kHz mono frames, gated by voice activity or push-to-talk,
//! sent to everyone on the channel.

use super::{FRAME, RATE, Shared, Talk};
use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::time::Duration;

/// Voice activity stays open 400 ms past the last loud frame and sends the 30 ms before the first.
const HANG: u32 = 40;
const PRE_ROLL: usize = 3;

/// Dropped, the device closes and its threads end.
pub struct Mic {
    _stream: Option<cpal::Stream>,
    pub name: String,
    stop: Arc<AtomicBool>,
}

impl Drop for Mic {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Microphones as (cpal id, description): the default host's inputs that are not outputs too (PipeWire lists every
/// sink as recordable, ALSA its duplex plugins), monitors or its default entry; all its inputs when that leaves
/// none.
pub fn devices() -> Vec<(String, String)> {
    let Ok(devs) = cpal::default_host().input_devices() else { return Vec::new() };
    let all: Vec<(String, String, bool)> = devs
        .filter_map(|d| {
            let (Ok(id), Ok(name)) = (d.id(), d.description()) else { return None };
            Some((id.to_string(), name.to_string(), d.supports_output()))
        })
        .collect();
    let mics: Vec<(String, String)> = all
        .iter()
        .filter(|(id, _, out)| !out && !id.ends_with(".monitor") && !id.ends_with(":input_default"))
        .map(|(id, n, _)| (id.clone(), n.clone()))
        .collect();
    let mut list = if mics.is_empty() { all.into_iter().map(|(id, n, _)| (id, n)).collect() } else { mics };
    // The short names, unless two would read the same.
    let short: Vec<String> = list.iter().map(|d| tidy(&d.1)).collect();
    for (k, d) in list.iter_mut().enumerate() {
        if short.iter().filter(|s| **s == short[k]).count() == 1 && !short[k].is_empty() {
            d.1 = short[k].clone();
        }
    }
    list
}

/// PipeWire's "Name [Port] via Bus" as "Name".
fn tidy(s: &str) -> String {
    let s = s.rsplit_once(" via ").map_or(s, |(name, _)| name);
    let mut out = String::new();
    let mut depth = 0u32;
    for c in s.chars() {
        match c {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            c if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Starts the microphone: `src` ("tone", "tone:HZ" or a WAV file) in place of a device; none in a test run's null
/// audio without one.
pub fn start(shared: Arc<Shared>, input: &str, src: Option<&std::ffi::OsStr>) -> Result<Mic> {
    let (tx, rx) = sync_channel::<Vec<f32>>(64);
    let stop = Arc::new(AtomicBool::new(false));
    let (stream, rate, name) = if let Some(src) = src {
        let (rate, name) = stand_in(&shared, &stop, std::path::Path::new(src), tx)?;
        (None, rate, name)
    } else {
        anyhow::ensure!(std::env::var("FFXI_AUDIO_DEVICE").as_deref() != Ok("null"), "no microphone in a test run");
        let (s, rate, name) = open(input, tx)?;
        (Some(s), rate, name)
    };
    let (sh, st) = (shared.clone(), stop.clone());
    std::thread::Builder::new().name("voice-mic".into()).spawn(move || run(sh, st, rx, rate))?;
    Ok(Mic { _stream: stream, name, stop })
}

fn open(input: &str, tx: SyncSender<Vec<f32>>) -> Result<(cpal::Stream, u32, String)> {
    let device = if input.is_empty() {
        cpal::default_host().default_input_device().context("no microphone")?
    } else {
        let id: cpal::DeviceId = input.parse()?;
        cpal::host_from_id(id.host())?.device_by_id(&id).with_context(|| format!("no microphone {input}"))?
    };
    let name = device.description().map(|d| d.to_string()).unwrap_or_default();
    let supported = device.default_input_config()?;
    let format = supported.sample_format();
    let config: StreamConfig = supported.into();
    let rate = config.sample_rate;
    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, config, tx)?,
        SampleFormat::I16 => build::<i16>(&device, config, tx)?,
        SampleFormat::I32 => build::<i32>(&device, config, tx)?,
        SampleFormat::U16 => build::<u16>(&device, config, tx)?,
        f => anyhow::bail!("unsupported microphone sample format {f}"),
    };
    stream.play()?;
    Ok((stream, rate, name))
}

fn build<T: SizedSample>(device: &cpal::Device, config: StreamConfig, tx: SyncSender<Vec<f32>>) -> Result<cpal::Stream>
where
    f32: FromSample<T>,
{
    let ch = (config.channels as usize).max(1);
    Ok(device.build_input_stream(
        config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            let mono: Vec<f32> = data
                .chunks_exact(ch)
                .map(|f| f.iter().map(|s| s.to_sample::<f32>()).sum::<f32>() / ch as f32)
                .collect();
            let _ = tx.try_send(mono);
        },
        |e| log::warn!("voice: microphone: {e}"),
        None,
    )?)
}

/// A tone (one second on, one off) or a WAV file looped, at real time.
fn stand_in(
    shared: &Arc<Shared>,
    stop: &Arc<AtomicBool>,
    src: &std::path::Path,
    tx: SyncSender<Vec<f32>>,
) -> Result<(u32, String)> {
    let tone = src
        .to_str()
        .and_then(|s| s.strip_prefix("tone"))
        .map(|hz| hz.strip_prefix(':').and_then(|h| h.parse::<f32>().ok()).unwrap_or(440.0));
    let (samples, rate, name) = if let Some(hz) = tone {
        let s = (0..RATE * 2)
            .map(|i| if i < RATE { (i as f32 * hz * std::f32::consts::TAU / RATE as f32).sin() * 0.25 } else { 0.0 })
            .collect();
        (s, RATE, format!("test tone {hz} Hz"))
    } else {
        let (s, rate) = super::wav::read(src)?;
        anyhow::ensure!(!s.is_empty() && rate > 0, "empty WAV file");
        (s, rate, src.display().to_string())
    };
    let (sh, stop) = (shared.clone(), stop.clone());
    std::thread::Builder::new().name("voice-mic-file".into()).spawn(move || {
        let block = (rate / 100) as usize;
        let start = std::time::Instant::now();
        let (mut at, mut sent) = (0usize, 0u64);
        while !sh.stop.load(Ordering::Relaxed) && !stop.load(Ordering::Relaxed) {
            let chunk: Vec<f32> = (0..block).map(|k| samples[(at + k) % samples.len()]).collect();
            at = (at + block) % samples.len();
            sent += block as u64;
            if tx.send(chunk).is_err() {
                break;
            }
            if let Some(d) = (start + Duration::from_micros(sent * 1_000_000 / rate as u64))
                .checked_duration_since(std::time::Instant::now())
            {
                std::thread::sleep(d);
            }
        }
    })?;
    Ok((rate, name))
}

/// Linear resampling to 48 kHz.
struct Resample {
    step: f64,
    pos: f64,
    buf: Vec<f32>,
}

impl Resample {
    fn new(rate: u32) -> Self {
        Self { step: rate as f64 / RATE as f64, pos: 0.0, buf: Vec::new() }
    }

    fn feed(&mut self, input: &[f32], out: &mut Vec<f32>) {
        self.buf.extend_from_slice(input);
        while (self.pos as usize) + 1 < self.buf.len() {
            let i = self.pos as usize;
            let f = (self.pos - i as f64) as f32;
            out.push(self.buf[i] + (self.buf[i + 1] - self.buf[i]) * f);
            self.pos += self.step;
        }
        let used = (self.pos as usize).min(self.buf.len());
        self.buf.drain(..used);
        self.pos -= used as f64;
    }
}

/// RNNoise (nnnoiseless) on each 10 ms frame, in place, before the gain: the room's hiss and steady noise taken out,
/// and between words (its voice probability low) the rest faded to FLOOR, so the level reads the voice and the gate
/// opens on it rather than on noise.
struct Denoise {
    state: Box<nnnoiseless::DenoiseState<'static>>,
    buf: [f32; FRAME],
    out: [f32; FRAME],
    started: bool,
    /// The fade's level now (FLOOR .. 1) and the frames it stays open after the last speech-like one.
    gain: f32,
    hold: u32,
}

const _: () = assert!(FRAME == nnnoiseless::FRAME_SIZE);
/// Speech opens the fade at once; it holds 200 ms past the last speech-like frame, then falls to -25 dB over about
/// 150 ms (x0.68 a frame).
const VAD_OPEN: f32 = 0.6;
const VAD_HOLD: u32 = 20;
const FLOOR: f32 = 0.056;
const FALL: f32 = 0.68;

impl Denoise {
    fn new() -> Self {
        Self {
            state: nnnoiseless::DenoiseState::new(),
            buf: [0.0; FRAME],
            out: [0.0; FRAME],
            started: false,
            gain: FLOOR,
            hold: 0,
        }
    }

    fn frame(&mut self, f: &mut [f32]) {
        // It takes 16-bit sample values; its first frame fades in, so it is not used.
        for (b, x) in self.buf.iter_mut().zip(f.iter()) {
            *b = x * 32768.0;
        }
        let vad = self.state.process_frame(&mut self.out, &self.buf);
        self.hold = if vad >= VAD_OPEN { VAD_HOLD } else { self.hold.saturating_sub(1) };
        let from = self.gain;
        self.gain = if self.hold > 0 { 1.0 } else { (self.gain * FALL).max(FLOOR) };
        let started = std::mem::replace(&mut self.started, true);
        // The fade moves across the frame, so a change makes no click.
        let n = f.len().max(1) as f32;
        for (k, (x, o)) in f.iter_mut().zip(self.out.iter()).enumerate() {
            let g = from + (self.gain - from) * (k as f32 + 1.0) / n;
            *x = if started { o / 32768.0 * g } else { 0.0 };
        }
    }
}

/// The gate's state between frames.
#[derive(Default)]
pub struct Gate {
    hang: u32,
    open: bool,
    seq: u32,
    pre: VecDeque<Vec<i16>>,
}

impl Gate {
    /// One frame in: the frames to send now (the pre-roll first when the gate opens).
    pub fn frame(
        &mut self,
        f: &[f32],
        talk: Talk,
        held: bool,
        threshold_db: f32,
        muted: bool,
    ) -> (f32, Vec<(u32, Vec<i16>)>) {
        let rms = (f.iter().map(|x| x * x).sum::<f32>() / f.len().max(1) as f32).sqrt();
        let db = 20.0 * (rms + 1e-9).log10();
        let open = !muted
            && match talk {
                Talk::Push => held,
                Talk::Voice => {
                    if db >= threshold_db {
                        self.hang = HANG;
                    } else {
                        self.hang = self.hang.saturating_sub(1);
                    }
                    self.hang > 0
                }
            };
        let pcm: Vec<i16> = f.iter().map(|x| (x.clamp(-1.0, 1.0) * 32767.0) as i16).collect();
        let mut out = Vec::new();
        if open {
            if !self.open && talk == Talk::Voice {
                out.extend(self.pre.drain(..).map(|p| (0, p)));
            }
            out.push((0, pcm));
            for o in &mut out {
                o.0 = self.seq;
                self.seq = self.seq.wrapping_add(1);
            }
        } else {
            self.pre.push_back(pcm);
            while self.pre.len() > PRE_ROLL {
                self.pre.pop_front();
            }
        }
        self.open = open;
        (db, out)
    }
}

fn run(shared: Arc<Shared>, stop: Arc<AtomicBool>, rx: Receiver<Vec<f32>>, rate: u32) {
    let mut rs = Resample::new(rate);
    let mut gate = Gate::default();
    let mut denoise = Denoise::new();
    let (mut at48, mut frame) = (Vec::new(), Vec::with_capacity(FRAME));
    while !shared.stop.load(Ordering::Relaxed) && !stop.load(Ordering::Relaxed) {
        let chunk = match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(c) => c,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        };
        at48.clear();
        rs.feed(&chunk, &mut at48);
        for &s in &at48 {
            frame.push(s);
            if frame.len() < FRAME {
                continue;
            }
            let (talk, threshold, gain, muted, quiet) = {
                let c = shared.cfg.lock().unwrap();
                (c.talk, c.threshold_db as f32, c.mic_gain as f32 / 100.0, c.mic_muted, c.noise_suppression)
            };
            if quiet {
                denoise.frame(&mut frame);
            }
            for x in &mut frame {
                *x *= gain;
            }
            let (db, send) = gate.frame(&frame, talk, super::PTT_HELD.load(Ordering::Relaxed), threshold, muted);
            shared.level.store(db.to_bits(), Ordering::Relaxed);
            shared.talking.store(!send.is_empty(), Ordering::Relaxed);
            for (seq, pcm) in send {
                shared.send_audio(seq, pcm);
            }
            frame.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_names_tidied() {
        assert_eq!(tidy("Anker PowerConf C200 Analog Stereo via USB"), "Anker PowerConf C200 Analog Stereo");
        assert_eq!(
            tidy("Ryzen HD Audio Controller Stereo Microphone [Microphone] via PCI"),
            "Ryzen HD Audio Controller Stereo Microphone"
        );
        assert_eq!(tidy("WH-1000XM6"), "WH-1000XM6");
    }

    /// Steady hiss at about -40 dBFS (an idle microphone) comes out well under it.
    #[test]
    fn denoise_takes_out_hiss() {
        let mut d = Denoise::new();
        let mut seed = 0x2545_f491_u32;
        let mut noise = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0
        };
        let db = |f: &[f32]| 20.0 * ((f.iter().map(|x| x * x).sum::<f32>() / f.len() as f32).sqrt() + 1e-9).log10();
        let (mut before, mut after) = (Vec::new(), Vec::new());
        for k in 0..300 {
            let mut f: Vec<f32> = (0..FRAME).map(|_| noise() * 0.017).collect();
            if k >= 200 {
                before.extend_from_slice(&f);
            }
            d.frame(&mut f);
            if k >= 200 {
                after.extend_from_slice(&f);
            }
        }
        let (b, a) = (db(&before), db(&after));
        assert!(b > -46.0 && b < -40.0, "{b}");
        assert!(a < b - 25.0, "{b} -> {a}");
    }

    #[test]
    fn resample_keeps_the_rate() {
        let mut r = Resample::new(44_100);
        let mut out = Vec::new();
        for _ in 0..100 {
            r.feed(&[0.5; 441], &mut out);
        }
        assert!((out.len() as i64 - 48_000).abs() <= 2, "{}", out.len());
        assert!(out.iter().all(|&x| (x - 0.5).abs() < 1e-6));
    }

    #[test]
    fn voice_gate_opens_with_pre_roll_and_hangs() {
        let mut g = Gate::default();
        let quiet = vec![0.0001; FRAME];
        let loud = vec![0.3; FRAME];
        for _ in 0..5 {
            assert!(g.frame(&quiet, Talk::Voice, false, -45.0, false).1.is_empty());
        }
        let (db, sent) = g.frame(&loud, Talk::Voice, false, -45.0, false);
        assert!(db > -11.0 && db < -10.0, "{db}");
        assert_eq!(sent.iter().map(|s| s.0).collect::<Vec<_>>(), [0, 1, 2, 3]);
        // Held open 400 ms after, then shut.
        let open: usize = (0..60).map(|_| g.frame(&quiet, Talk::Voice, false, -45.0, false).1.len()).sum();
        assert_eq!(open, HANG as usize - 1);
        // Muted or push-to-talk not held sends nothing.
        assert!(g.frame(&loud, Talk::Voice, false, -45.0, true).1.is_empty());
        assert!(g.frame(&loud, Talk::Push, false, -45.0, false).1.is_empty());
        assert_eq!(g.frame(&quiet, Talk::Push, true, -45.0, false).1.len(), 1);
    }
}
