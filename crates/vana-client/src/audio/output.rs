//! cpal output (PipeWire, then PulseAudio, then ALSA) feeding the mixer.

use super::mixer::Mixer;
use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, FromSample, SampleFormat, SizedSample, StreamConfig, SupportedBufferSize};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub struct Output {
    _stream: Option<cpal::Stream>,
    _null: Option<NullOut>,
    pub host: String,
    pub rate: u32,
}

/// Output devices as "id  description" lines, for picking one in the config.
pub fn list() -> Vec<String> {
    let mut out = Vec::new();
    for id in cpal::available_hosts() {
        let Ok(host) = cpal::host_from_id(id) else { continue };
        let Ok(devs) = host.output_devices() else { continue };
        for d in devs {
            let name = d.description().map(|x| x.to_string()).unwrap_or_default();
            out.push(format!("{}  {name}", d.id().map(|i| i.to_string()).unwrap_or_default()));
        }
    }
    out
}

/// What an output stream plays: interleaved stereo f32 frames, filled whole.
pub trait Render: Send + 'static {
    fn render(&mut self, out: &mut [f32]);
}

impl Render for Mixer {
    fn render(&mut self, out: &mut [f32]) {
        Mixer::render(self, out);
    }
}

/// Stops the null output's thread when dropped.
struct NullOut(Arc<AtomicBool>);

impl Drop for NullOut {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// Opens the device with this cpal id ("host:device"), "null" (silent, mixed at real time, for test runs: ALSA's
/// null device is not paced, so sounds there end at once), else the default output; `make` builds the renderer once
/// the rate is known.
pub fn open<R: Render>(device: Option<&str>, make: impl FnOnce(u32) -> R) -> Result<Output> {
    if device == Some("null") {
        return open_null(make);
    }
    let (host, device) = match device.filter(|d| !d.is_empty()) {
        Some(s) => {
            // A device saved by a build with PipeWire, in one built without it (--no-default-features): this build
            // has no such host, so the id doesn't parse (or the host isn't there) and the default output plays
            // instead of no sound at all. A malformed id or a missing device on a known host still fails.
            let found = s.parse::<cpal::DeviceId>().and_then(|id| Ok((cpal::host_from_id(id.host())?, id)));
            match found {
                Ok((host, id)) => {
                    let d = host.device_by_id(&id).with_context(|| format!("no audio device {s}"))?;
                    (host, d)
                }
                Err(e)
                    if e.kind() == cpal::ErrorKind::UnsupportedOperation
                        || e.kind() == cpal::ErrorKind::HostUnavailable =>
                {
                    log::warn!("audio device {s}: {e}; using the default output");
                    let host = cpal::default_host();
                    let d = host.default_output_device().context("no audio output device")?;
                    (host, d)
                }
                Err(e) => return Err(e.into()),
            }
        }
        None => {
            let host = cpal::default_host();
            let d = host.default_output_device().context("no audio output device")?;
            (host, d)
        }
    };
    let supported = device.default_output_config()?;
    let format = supported.sample_format();
    let range = supported.buffer_size().clone();
    let mut config: StreamConfig = supported.into();
    let rate = config.sample_rate;
    // PulseAudio with the default buffer leaves every buffer attribute to the server and sets no ADJUST_LATENCY,
    // so the server's 2 s default target length applies: sounds play long after they are triggered (worst on
    // slower ARM boards, where the queue stays full). Ask for 20 ms callbacks instead, which cpal turns into a
    // 40 ms target with ADJUST_LATENCY. PipeWire and ALSA already pick a period-sized buffer by default.
    if host.id().name() == "PulseAudio" {
        let want = (rate / 50).max(1);
        if let SupportedBufferSize::Range { min, max } = range
            && (min..=max).contains(&want)
        {
            config.buffer_size = BufferSize::Fixed(want);
        }
    }
    let mixer = make(rate);
    let stream = match format {
        SampleFormat::F32 => build::<f32, R>(&device, config.clone(), mixer)?,
        SampleFormat::I16 => build::<i16, R>(&device, config.clone(), mixer)?,
        SampleFormat::I32 => build::<i32, R>(&device, config.clone(), mixer)?,
        SampleFormat::U16 => build::<u16, R>(&device, config.clone(), mixer)?,
        f => anyhow::bail!("unsupported output sample format {f}"),
    };
    stream.play()?;
    Ok(Output { _stream: Some(stream), _null: None, host: host.id().name().to_string(), rate })
}

fn open_null<R: Render>(make: impl FnOnce(u32) -> R) -> Result<Output> {
    const RATE: u32 = 48000;
    const BLOCK: usize = 480;
    let mut mixer = make(RATE);
    let stop = Arc::new(AtomicBool::new(false));
    let s = stop.clone();
    std::thread::Builder::new().name("audio-null".into()).spawn(move || {
        let mut buf = vec![0.0f32; BLOCK * 2];
        let (start, mut done) = (Instant::now(), 0u64);
        while !s.load(Ordering::Relaxed) {
            mixer.render(&mut buf);
            done += BLOCK as u64;
            if let Some(d) =
                (start + Duration::from_micros(done * 1_000_000 / RATE as u64)).checked_duration_since(Instant::now())
            {
                std::thread::sleep(d);
            }
        }
    })?;
    Ok(Output { _stream: None, _null: Some(NullOut(stop)), host: "null".into(), rate: RATE })
}

fn build<T: SizedSample + FromSample<f32>, R: Render>(
    device: &cpal::Device,
    config: StreamConfig,
    mut mixer: R,
) -> Result<cpal::Stream> {
    let channels = config.channels as usize;
    let mut buf: Vec<f32> = Vec::new();
    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            let frames = data.len() / channels.max(1);
            buf.resize(frames * 2, 0.0);
            mixer.render(&mut buf);
            for (f, s) in data.chunks_exact_mut(channels).zip(buf.chunks_exact(2)) {
                match f.len() {
                    1 => f[0] = T::from_sample((s[0] + s[1]) * 0.5),
                    _ => {
                        f[0] = T::from_sample(s[0]);
                        f[1] = T::from_sample(s[1]);
                        for x in &mut f[2..] {
                            *x = T::from_sample(0.0);
                        }
                    }
                }
            }
        },
        |e| log::warn!("audio stream: {e}"),
        None,
    )?;
    Ok(stream)
}
