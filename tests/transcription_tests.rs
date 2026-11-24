use std::sync::Arc;

use bytes::Bytes;
use tempfile::TempDir;
use tokio::sync::{Notify, watch};

use softagon_relay::config::{
    JournalMode, MediaBackendConfig, MediaConfig, SqliteConfig, TranscriptionConfig, WhisperModel,
};
use softagon_relay::core::events::EventBus;
use softagon_relay::core::queue::TranscriptionQueue;
use softagon_relay::storage::database::Database;
use softagon_relay::storage::database::media::NewMediaObject;
use softagon_relay::storage::database::messages::NewMessage;
use softagon_relay::storage::media::{MediaStorage, ObjectStoreMediaStorage};
use softagon_relay::transcription::audio::{
    WHISPER_SAMPLE_RATE, decode_and_resample_to_16k_mono, resample_mono_to_16k,
};
use softagon_relay::transcription::whisper::MockTranscriptionEngine;
use softagon_relay::transcription::worker::TranscriptionWorkerPool;

/// Builds a minimal valid stereo 48 kHz 16-bit PCM WAV file in memory (0.5 seconds of sine wave).
fn generate_test_wav_48k_stereo() -> Vec<u8> {
    let sample_rate: u32 = 48_000;
    let channels: u16 = 2;
    let bits_per_sample: u16 = 16;
    let num_frames: u32 = sample_rate / 2; // 0.5 s
    let data_bytes = num_frames * u32::from(channels) * u32::from(bits_per_sample / 8);

    let mut buf = Vec::with_capacity(44 + data_bytes as usize);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    buf.extend_from_slice(b"WAVE");
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes()); // PCM
    buf.extend_from_slice(&channels.to_le_bytes());
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    let byte_rate = sample_rate * u32::from(channels) * u32::from(bits_per_sample / 8);
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    let block_align = channels * (bits_per_sample / 8);
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&bits_per_sample.to_le_bytes());
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_bytes.to_le_bytes());

    for i in 0..num_frames {
        let t = i as f32 / sample_rate as f32;
        let sample = (2.0 * std::f32::consts::PI * 440.0 * t).sin();
        let s16 = (sample * 16000.0) as i16;
        // Left + Right
        buf.extend_from_slice(&s16.to_le_bytes());
        buf.extend_from_slice(&s16.to_le_bytes());
    }
    buf
}

#[test]
fn decode_wav_downmix_and_resample_48k_to_16k() {
    let wav = generate_test_wav_48k_stereo();
    let decoded = decode_and_resample_to_16k_mono(&wav, Some("audio/wav")).unwrap();
    assert_eq!(decoded.source_sample_rate, 48_000);
    assert_eq!(decoded.source_channels, 2);
    // 0.5 seconds at 16 kHz = ~8000 samples
    assert!((decoded.samples_16k_mono.len() as i64 - 8000).abs() < 200);
    assert!((decoded.duration_ms - 500).abs() < 30);
}

#[test]
fn resample_16k_passthrough() {
    let input = vec![0.25f32; WHISPER_SAMPLE_RATE / 4];
    let out = resample_mono_to_16k(&input, 16_000).unwrap();
    assert_eq!(out.len(), input.len());
}

#[tokio::test]
async fn transcription_worker_processes_audio_and_cleans_temp_files() {
    let dir = TempDir::new().unwrap();
    let tmp_dir = dir.path().join("tmp");
    let db = Database::connect(&SqliteConfig {
        relay_database_path: dir.path().join("relay.db"),
        whatsapp_database_path: dir.path().join("wa.db"),
        busy_timeout_ms: 5000,
        journal_mode: JournalMode::Wal,
    })
    .await
    .unwrap();

    let media: Arc<dyn MediaStorage> = Arc::new(
        ObjectStoreMediaStorage::from_config(&MediaConfig {
            backend: MediaBackendConfig::Local {
                path: dir.path().join("media"),
            },
        })
        .unwrap(),
    );

    db.create_session("sess_tx", "voice", "connected", Some("en"))
        .await
        .unwrap();

    let wav = generate_test_wav_48k_stereo();
    let stored = media
        .put("sess_tx/med_voice.wav", Bytes::from(wav))
        .await
        .unwrap();

    db.insert_media_object(NewMediaObject {
        id: "med_voice",
        session_id: "sess_tx",
        storage_backend: stored.backend,
        object_key: &stored.object_key,
        mime_type: "audio/wav",
        size_bytes: stored.size_bytes,
        sha256: &stored.sha256,
        filename: Some("voice.wav"),
    })
    .await
    .unwrap();

    db.insert_message(NewMessage {
        id: "msg_voice",
        session_id: "sess_tx",
        external_id: Some("wamid.voice1"),
        direction: "inbound",
        chat_jid: "5511999999999@s.whatsapp.net",
        sender_jid: Some("5511999999999@s.whatsapp.net"),
        message_type: "audio",
        text_body: None,
        caption: None,
        media_id: Some("med_voice"),
        status: "received",
        metadata_json: "{}",
    })
    .await
    .unwrap();

    let wh_notify = Arc::new(Notify::new());
    let tx_notify = Arc::new(Notify::new());
    let events = EventBus::new(db.clone(), wh_notify, 3);
    let queue = TranscriptionQueue::new(db.clone(), 10, tx_notify);
    let engine = Arc::new(MockTranscriptionEngine::new("testing one two", "en"));

    let tx_cfg = TranscriptionConfig {
        enabled: true,
        engine: "whisper".to_string(),
        model: WhisperModel::Base,
        language: None,
        model_dir: dir.path().join("models"),
        preload: false,
        idle_unload_seconds: 180,
        threads: 1,
        workers: 1,
        max_queue: 10,
        timeout_seconds: 30,
        delete_temp_audio: true,
    };

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let handles = TranscriptionWorkerPool::new(
        db.clone(),
        media,
        queue.clone(),
        engine,
        events,
        tx_cfg,
        tmp_dir.clone(),
    )
    .spawn(shutdown_rx);

    queue
        .enqueue("msg_voice", "sess_tx", "med_voice", Some("en"))
        .await
        .unwrap();

    // Poll for completion
    let mut completed = None;
    for _ in 0..30 {
        if let Some(rec) = db.get_transcription("msg_voice").await.unwrap()
            && rec.status == "completed"
        {
            completed = Some(rec);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    let _ = shutdown_tx.send(true);
    for h in handles {
        let _ = h.await;
    }

    let rec = completed.expect("transcription should complete");
    assert_eq!(rec.text.as_deref(), Some("testing one two"));
    assert_eq!(rec.language.as_deref(), Some("en"));
    assert!(rec.duration_ms.unwrap() > 400);

    // Temp audio directory should be empty after cleanup
    let remaining: Vec<_> = std::fs::read_dir(&tmp_dir)
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert!(remaining.is_empty(), "temp audio file was not cleaned up");
}
