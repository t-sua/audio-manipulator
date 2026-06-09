use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

// ── OLA constants ────────────────────────────────────────────────────────────
const OLA_FRAME: usize = 2048;
const OLA_HOP: usize = OLA_FRAME / 2; // 50% overlap → periodic Hann is COLA

/// Periodic Hann window: w(n) = 0.5 * (1 - cos(2π·n/N))
/// At 50% overlap the sum of successive windows is exactly 1.0 (COLA).
fn hann(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| {
            0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos())
        })
        .collect()
}

// ── OLA state ─────────────────────────────────────────────────────────────────

struct Ola {
    /// Interleaved overlap-accumulation buffer, length = OLA_FRAME * channels.
    overlap: Vec<f32>,
    /// Interleaved output samples ready to consume.
    queue: VecDeque<f32>,
    /// Fractional source-frame position (what the audio thread is reading).
    pub pos: f64,
    window: Vec<f32>,
    channels: usize,
}

impl Ola {
    fn new(channels: usize) -> Self {
        Self {
            overlap: vec![0.0; OLA_FRAME * channels],
            queue: VecDeque::with_capacity(OLA_FRAME * channels * 8),
            pos: 0.0,
            window: hann(OLA_FRAME),
            channels,
        }
    }

    fn reset(&mut self, pos: f64) {
        self.pos = pos;
        self.queue.clear();
        self.overlap.fill(0.0);
    }

    /// Read one analysis frame from `src`, overlap-add into the accumulator,
    /// push OLA_HOP output samples to the queue, then advance `self.pos`.
    fn generate_hop(&mut self, src: &[f32], src_ch: usize, speed: f64) {
        let ch = self.channels;
        let total = src.len() / src_ch;

        for i in 0..OLA_FRAME {
            let sp = self.pos + i as f64;
            if sp >= total as f64 {
                break;
            }
            let p0 = sp as usize;
            let frac = (sp - p0 as f64) as f32;
            let p1 = (p0 + 1).min(total - 1);
            let w = self.window[i];

            for c in 0..ch {
                let sc = c.min(src_ch - 1);
                let a = src[p0 * src_ch + sc];
                let b = src[p1 * src_ch + sc];
                self.overlap[i * ch + c] += (a + (b - a) * frac) * w;
            }
        }

        for i in 0..OLA_HOP {
            for c in 0..ch {
                self.queue.push_back(self.overlap[i * ch + c]);
            }
        }

        // Shift accumulator left by one hop.
        let hop_s = OLA_HOP * ch;
        let frame_s = OLA_FRAME * ch;
        self.overlap.copy_within(hop_s..frame_s, 0);
        self.overlap[frame_s - hop_s..].fill(0.0);

        self.pos += OLA_HOP as f64 * speed;
    }
}

// ── Playback state ────────────────────────────────────────────────────────────

pub struct PlaybackState {
    pub samples: Vec<f32>,
    pub channels: usize,
    pub speed: f64,
    pub volume: f32,
    /// Display position (updated from OLA pos or simple-interp pos each callback).
    pub playback_pos: f64,
    pub is_playing: bool,
    pub loop_mode: bool,
    /// Selection in frames (None = whole file).
    pub selection: Option<(f64, f64)>,
    ola: Ola,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            samples: Vec::new(),
            channels: 2,
            speed: 1.0,
            volume: 1.0,
            playback_pos: 0.0,
            is_playing: false,
            loop_mode: false,
            selection: None,
            ola: Ola::new(2),
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

    pub fn seek_to(&mut self, frame: f64) {
        self.playback_pos = frame;
        self.ola.reset(frame);
    }

    pub fn set_speed(&mut self, new_speed: f64) {
        if (new_speed - self.speed).abs() > 1e-5 {
            let pos = self.playback_pos;
            self.speed = new_speed;
            self.ola.reset(pos);
        }
    }

    pub fn seek_to_start(&mut self) {
        self.seek_to(self.play_range().0);
    }

    /// Fill `output` using plain linear interpolation (used at exactly 1× speed).
    fn fill_simple(&mut self, output: &mut [f32], out_ch: usize) {
        let (play_start, play_end) = self.play_range();
        let speed = self.speed;
        let src_ch = self.channels;
        let total = self.total_frames();
        let frame_count = output.len() / out_ch;
        let mut pos = self.playback_pos;
        let mut stopped = false;

        for f in 0..frame_count {
            if pos >= play_end {
                if self.loop_mode {
                    pos = play_start;
                } else {
                    output[f * out_ch..].fill(0.0);
                    stopped = true;
                    break;
                }
            }
            let p0 = pos as usize;
            let frac = (pos - p0 as f64) as f32;
            let p1 = (p0 + 1).min(total.saturating_sub(1));
            for oc in 0..out_ch {
                let sc = oc.min(src_ch - 1);
                let a = self.samples.get(p0 * src_ch + sc).copied().unwrap_or(0.0);
                let b = self.samples.get(p1 * src_ch + sc).copied().unwrap_or(0.0);
                output[f * out_ch + oc] = a + (b - a) * frac;
            }
            pos += speed;
        }

        if stopped {
            self.is_playing = false;
            self.playback_pos = play_start;
        } else {
            self.playback_pos = pos;
        }
    }

    /// Fill `output` using OLA time-stretching (pitch stays constant).
    fn fill_ola(&mut self, output: &mut [f32]) {
        let (play_start, play_end) = self.play_range();
        let speed = self.speed;
        let src_ch = self.channels;
        let needed = output.len();

        while self.ola.queue.len() < needed {
            if self.ola.pos >= play_end {
                if self.loop_mode {
                    self.ola.reset(play_start);
                } else {
                    let have = self.ola.queue.len();
                    for (i, s) in output.iter_mut().enumerate() {
                        *s = if i < have {
                            self.ola.queue.pop_front().unwrap_or(0.0)
                        } else {
                            0.0
                        };
                    }
                    self.is_playing = false;
                    self.playback_pos = play_start;
                    self.ola.reset(play_start);
                    return;
                }
            }
            // Split borrow: pass samples slice separately.
            let samples = &self.samples as *const Vec<f32>;
            // SAFETY: `samples` and `ola` are distinct fields; no aliasing.
            self.ola.generate_hop(unsafe { &*samples }, src_ch, speed);
        }

        for s in output.iter_mut() {
            *s = self.ola.queue.pop_front().unwrap_or(0.0);
        }
        self.playback_pos = self.ola.pos;
    }
}

// ── Audio engine ──────────────────────────────────────────────────────────────

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
            ola: Ola::new(output_channels),
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

            // Bypass OLA at exactly 1× speed for perfect quality.
            if (st.speed - 1.0).abs() < 0.005 {
                st.fill_simple(output, out_ch);
            } else {
                st.fill_ola(output);
            }

            let vol = st.volume;
            if (vol - 1.0).abs() > 1e-4 {
                for s in output.iter_mut() {
                    *s *= vol;
                }
            }
        },
        |err| eprintln!("audio stream error: {err}"),
        None,
    )?;

    Ok(stream)
}
