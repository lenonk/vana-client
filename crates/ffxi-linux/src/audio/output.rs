//! cpal output (PipeWire, then PulseAudio, then ALSA) feeding the mixer.

use super::mixer::Mixer;
use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};

pub struct Output {
    _stream: cpal::Stream,
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

/// Opens the device with this cpal id ("host:device"), else the default output; `make` builds the
/// mixer once the rate is known.
pub fn open(device: Option<&str>, make: impl FnOnce(u32) -> Mixer) -> Result<Output> {
    let (host, device) = match device.filter(|d| !d.is_empty()) {
        Some(s) => {
            let id: cpal::DeviceId = s.parse()?;
            let host = cpal::host_from_id(id.host())?;
            let d = host.device_by_id(&id).with_context(|| format!("no audio device {s}"))?;
            (host, d)
        }
        None => {
            let host = cpal::default_host();
            let d = host.default_output_device().context("no audio output device")?;
            (host, d)
        }
    };
    let supported = device.default_output_config()?;
    let format = supported.sample_format();
    let config: StreamConfig = supported.into();
    let rate = config.sample_rate;
    let mixer = make(rate);
    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, config.clone(), mixer)?,
        SampleFormat::I16 => build::<i16>(&device, config.clone(), mixer)?,
        SampleFormat::I32 => build::<i32>(&device, config.clone(), mixer)?,
        SampleFormat::U16 => build::<u16>(&device, config.clone(), mixer)?,
        f => anyhow::bail!("unsupported output sample format {f}"),
    };
    stream.play()?;
    Ok(Output { _stream: stream, host: host.id().name().to_string(), rate })
}

fn build<T: SizedSample + FromSample<f32>>(device: &cpal::Device, config: StreamConfig, mut mixer: Mixer) -> Result<cpal::Stream> {
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
