use anyhow::{anyhow, Result};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

pub struct AudioData {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: usize,
}

impl AudioData {
    pub fn total_frames(&self) -> usize {
        if self.channels == 0 {
            return 0;
        }
        self.samples.len() / self.channels
    }

    pub fn duration_secs(&self) -> f64 {
        self.total_frames() as f64 / self.sample_rate as f64
    }
}

pub fn decode_file(path: &str) -> Result<AudioData> {
    let file = std::fs::File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let hint = Hint::new();
    let probed = symphonia::default::get_probe().format(
        &hint,
        mss,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    )?;

    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| {
            t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL
        })
        .ok_or_else(|| anyhow!("No supported audio track"))?;

    let track_id = track.id;
    let sample_rate = track.codec_params.sample_rate.unwrap_or(44100);
    let channels = track
        .codec_params
        .channels
        .map(|c| c.count())
        .unwrap_or(2);

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())?;

    let mut samples: Vec<f32> = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break
            }
            Err(symphonia::core::errors::Error::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(e) => return Err(e.into()),
        };

        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(decoded) => {
                let mut buf =
                    SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
                buf.copy_interleaved_ref(decoded);
                samples.extend_from_slice(buf.samples());
            }
            Err(symphonia::core::errors::Error::IoError(_)) => continue,
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(e) => return Err(e.into()),
        }
    }

    Ok(AudioData {
        samples,
        sample_rate,
        channels,
    })
}

/// Resample `data` from `from_rate` to `to_rate` using linear interpolation.
pub fn resample(data: &AudioData, to_rate: u32) -> AudioData {
    if data.sample_rate == to_rate {
        return AudioData {
            samples: data.samples.clone(),
            sample_rate: to_rate,
            channels: data.channels,
        };
    }

    let ratio = data.sample_rate as f64 / to_rate as f64;
    let in_frames = data.total_frames();
    let out_frames = (in_frames as f64 / ratio) as usize;
    let ch = data.channels;

    let mut out = Vec::with_capacity(out_frames * ch);

    for i in 0..out_frames {
        let src_pos = i as f64 * ratio;
        let s0 = src_pos as usize;
        let s1 = (s0 + 1).min(in_frames - 1);
        let frac = (src_pos - s0 as f64) as f32;

        for c in 0..ch {
            let a = data.samples[s0 * ch + c];
            let b = data.samples[s1 * ch + c];
            out.push(a + (b - a) * frac);
        }
    }

    AudioData {
        samples: out,
        sample_rate: to_rate,
        channels: ch,
    }
}
