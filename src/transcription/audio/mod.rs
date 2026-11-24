use std::io::Cursor;

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};
use symphonia::core::audio::GenericAudioBufferRef;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::codecs::registry::CodecRegistry;
use symphonia::core::common::Tier;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::formats::probe::Hint;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia_opus::OpusDecoder;

use crate::transcription::TranscriptionError;

pub const WHISPER_SAMPLE_RATE: usize = 16_000;

#[derive(Debug, Clone)]
pub struct DecodedAudio {
    pub samples_16k_mono: Vec<f32>,
    pub duration_ms: i64,
    pub source_sample_rate: u32,
    pub source_channels: usize,
}

/// Decodes OGG/Opus (or WAV/PCM) audio bytes into 16 kHz mono f32 PCM samples suitable for Whisper.
pub fn decode_and_resample_to_16k_mono(
    bytes: &[u8],
    mime_hint: Option<&str>,
) -> Result<DecodedAudio, TranscriptionError> {
    if bytes.is_empty() {
        return Err(TranscriptionError::AudioDecode(
            "empty audio payload".into(),
        ));
    }

    let mut registry = CodecRegistry::new();
    symphonia::default::register_enabled_codecs(&mut registry);
    registry.register_audio_decoder_at_tier::<OpusDecoder>(Tier::Fallback);

    let cursor = Cursor::new(bytes.to_vec());
    let mss = MediaSourceStream::new(Box::new(cursor), Default::default());

    let mut hint = Hint::new();
    if let Some(mime) = mime_hint {
        let base = mime.split(';').next().unwrap_or(mime).trim();
        hint.mime_type(base);
        match base {
            "audio/ogg" => {
                hint.with_extension("ogg");
            }
            "audio/wav" | "audio/x-wav" => {
                hint.with_extension("wav");
            }
            _ => {}
        }
    }

    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| TranscriptionError::AudioDecode(format!("format probe failed: {e}")))?;

    let track = format
        .default_track(symphonia::core::formats::TrackType::Audio)
        .ok_or_else(|| TranscriptionError::AudioDecode("no audio track found".into()))?;

    let track_id = track.id;
    let codec_params = track
        .codec_params
        .as_ref()
        .and_then(|cp| cp.audio())
        .ok_or_else(|| TranscriptionError::AudioDecode("missing audio codec parameters".into()))?
        .clone();

    let mut decoder = registry
        .make_audio_decoder(&codec_params, &AudioDecoderOptions::default())
        .map_err(|e| TranscriptionError::AudioDecode(format!("decoder init failed: {e}")))?;

    let mut mono_samples: Vec<f32> = Vec::new();
    let mut source_rate: u32 = codec_params.sample_rate.unwrap_or(48_000);
    let mut source_channels: usize = codec_params
        .channels
        .as_ref()
        .map(|c| c.count())
        .unwrap_or(1);

    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(SymphoniaError::IoError(err))
                if err.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(SymphoniaError::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(err) => {
                return Err(TranscriptionError::AudioDecode(format!(
                    "demux error: {err}"
                )));
            }
        };

        if packet.track_id != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(buf) => buf,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(err) => {
                return Err(TranscriptionError::AudioDecode(format!(
                    "decode error: {err}"
                )));
            }
        };

        let spec = decoded.spec();
        source_rate = spec.rate();
        source_channels = spec.channels().count().max(1);

        append_downmixed_mono_f32(&decoded, &mut mono_samples);
    }

    if mono_samples.is_empty() {
        return Err(TranscriptionError::AudioDecode(
            "decoded audio contains 0 samples".into(),
        ));
    }

    let samples_16k_mono = resample_mono_to_16k(&mono_samples, source_rate)?;
    let duration_ms =
        ((samples_16k_mono.len() as f64 / WHISPER_SAMPLE_RATE as f64) * 1000.0).round() as i64;

    Ok(DecodedAudio {
        samples_16k_mono,
        duration_ms,
        source_sample_rate: source_rate,
        source_channels,
    })
}

fn append_downmixed_mono_f32(decoded: &GenericAudioBufferRef<'_>, out: &mut Vec<f32>) {
    let frames = decoded.frames();
    if frames == 0 {
        return;
    }
    let channels = decoded.spec().channels().count().max(1);
    let mut interleaved: Vec<f32> = Vec::with_capacity(frames * channels);
    decoded.copy_to_vec_interleaved(&mut interleaved);

    out.reserve(frames);
    if channels == 1 {
        out.extend_from_slice(&interleaved);
    } else {
        let inv = 1.0f32 / (channels as f32);
        for frame in interleaved.chunks_exact(channels) {
            let sum: f32 = frame.iter().copied().sum();
            out.push(sum * inv);
        }
    }
}

pub fn resample_mono_to_16k(
    samples: &[f32],
    source_rate: u32,
) -> Result<Vec<f32>, TranscriptionError> {
    if source_rate == 0 {
        return Err(TranscriptionError::Resample(
            "source sample rate is 0".into(),
        ));
    }
    if source_rate as usize == WHISPER_SAMPLE_RATE {
        return Ok(samples.to_vec());
    }
    if samples.is_empty() {
        return Ok(Vec::new());
    }

    let chunk_size = 1024usize;
    let mut resampler = Fft::<f32>::new(
        source_rate as usize,
        WHISPER_SAMPLE_RATE,
        chunk_size,
        1,
        FixedSync::Both,
    )
    .map_err(|e| TranscriptionError::Resample(format!("resampler init failed: {e}")))?;

    let input_adapter = InterleavedSlice::new(samples, 1, samples.len())
        .map_err(|e| TranscriptionError::Resample(format!("input buffer adapter error: {e}")))?;

    let output = resampler
        .process_all(&input_adapter, samples.len(), None)
        .map_err(|e| TranscriptionError::Resample(format!("resample failed: {e}")))?;

    Ok(output.take_data())
}
