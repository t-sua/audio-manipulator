use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};

pub struct PlaybackState {
    pub samples: Vec<f32>,
    pub channels: usize,
    /// Playback speed multiplier (0.1 – 2.0). At 1.0 both speed and pitch are unchanged.
    pub speed: f64,
    /// Fractional frame position within `samples`.
    pub playback_pos: f64,
    pub is_playing: bool,
    pub loop_mode: bool,
    /// Selection in frames (None = full file).
    pub selection: Option<(f64, f64)>,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            samples: Vec::new(),
            channels: 2,
            speed: 1.0,
            playback_pos: 0.0,
            is_playing: false,
            loop_mode: false,
            selection: None,
        }
    }
}

impl PlaybackState {
    pub fn total_frames(&self) -> usize {
        if self.channels == 0 {
            return 0;
        }
        self.samples.len() / self.channels
    }

    pub fn play_range(&self) -> (f64, f64) {
        let total = self.total_frames() as f64;
        match self.selection {
            Some((s, e)) => (s.max(0.0), e.min(total)),
            None => (0.0, total),
        }
    }

    pub fn seek_to_start(&mut self) {
        self.playback_pos = self.play_range().0;
    }
}

pub struct AudioEngine {
    pub state: Arc<Mutex<PlaybackState>>,
    pub output_sample_rate: u32,
    pub output_channels: usize,
    _stream: cpal::Stream,
}

impl AudioEngine {
    pub fn new() -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow!("No audio output device found"))?;

        let default_cfg = device.default_output_config()?;
        let output_sample_rate = default_cfg.sample_rate().0;
        let output_channels = default_cfg.channels() as usize;

        let state: Arc<Mutex<PlaybackState>> = Arc::new(Mutex::new(PlaybackState {
            channels: output_channels,
            ..Default::default()
        }));

        let stream = build_stream(&device, &default_cfg.into(), Arc::clone(&state))?;
        stream.play()?;

        Ok(AudioEngine {
            state,
            output_sample_rate,
            output_channels,
            _stream: stream,
        })
    }
}

fn build_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    state: Arc<Mutex<PlaybackState>>,
) -> Result<cpal::Stream> {
    let out_ch = config.channels as usize;

    let stream = device.build_output_stream(
        config,
        move |output: &mut [f32], _| {
            fill_output(output, out_ch, &state);
        },
        |err| eprintln!("audio stream error: {err}"),
        None,
    )?;

    Ok(stream)
}

fn fill_output(output: &mut [f32], out_ch: usize, state: &Arc<Mutex<PlaybackState>>) {
    let mut st = match state.lock() {
        Ok(s) => s,
        Err(_) => {
            output.fill(0.0);
            return;
        }
    };

    if !st.is_playing || st.samples.is_empty() {
        output.fill(0.0);
        return;
    }

    let (play_start, play_end) = st.play_range();
    let speed = st.speed;
    let src_ch = st.channels;
    let total_frames = st.total_frames();

    let frame_count = output.len() / out_ch;
    let mut pos = st.playback_pos;
    let mut stopped = false;

    for f in 0..frame_count {
        if pos >= play_end {
            if st.loop_mode {
                pos = play_start;
            } else {
                output[f * out_ch..].fill(0.0);
                stopped = true;
                break;
            }
        }

        let p0 = pos as usize;
        let frac = (pos - p0 as f64) as f32;
        let p1 = (p0 + 1).min(total_frames.saturating_sub(1));

        for oc in 0..out_ch {
            let sc = oc.min(src_ch - 1);
            let a = st.samples.get(p0 * src_ch + sc).copied().unwrap_or(0.0);
            let b = st.samples.get(p1 * src_ch + sc).copied().unwrap_or(0.0);
            output[f * out_ch + oc] = a + (b - a) * frac;
        }

        pos += speed;
    }

    if stopped {
        st.is_playing = false;
        st.playback_pos = play_start;
    } else {
        st.playback_pos = pos;
    }
}
