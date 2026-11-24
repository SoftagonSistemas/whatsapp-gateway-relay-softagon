use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::watch;
use tracing::{info, warn};

use crate::config::TranscriptionConfig;
use crate::core::events::EventBus;
use crate::core::queue::TranscriptionQueue;
use crate::storage::database::Database;
use crate::storage::database::transcriptions::{TranscriptionJobRecord, UpsertTranscription};
use crate::storage::media::MediaStorage;
use crate::transcription::audio::decode_and_resample_to_16k_mono;
use crate::transcription::{TranscriptionEngine, TranscriptionError, TranscriptionResult};

pub struct TranscriptionWorkerPool {
    db: Database,
    media: Arc<dyn MediaStorage>,
    queue: TranscriptionQueue,
    engine: Arc<dyn TranscriptionEngine>,
    events: EventBus,
    config: TranscriptionConfig,
    tmp_dir: PathBuf,
}

impl TranscriptionWorkerPool {
    pub fn new(
        db: Database,
        media: Arc<dyn MediaStorage>,
        queue: TranscriptionQueue,
        engine: Arc<dyn TranscriptionEngine>,
        events: EventBus,
        config: TranscriptionConfig,
        tmp_dir: PathBuf,
    ) -> Self {
        Self {
            db,
            media,
            queue,
            engine,
            events,
            config,
            tmp_dir,
        }
    }

    pub fn spawn(self, shutdown: watch::Receiver<bool>) -> Vec<tokio::task::JoinHandle<()>> {
        let shared = Arc::new(self);
        let mut handles = Vec::with_capacity(shared.config.workers);

        if shared.config.preload && shared.engine.is_available() {
            let engine = shared.engine.clone();
            tokio::spawn(async move {
                if let Err(err) = engine.preload().await {
                    warn!(error = %err, "whisper model preload failed");
                }
            });
        }

        for worker_idx in 0..shared.config.workers {
            let pool = shared.clone();
            let mut rx = shutdown.clone();
            let handle = tokio::spawn(async move {
                info!(worker = worker_idx, "transcription worker started");
                loop {
                    if *rx.borrow() {
                        info!(worker = worker_idx, "transcription worker shutting down");
                        break;
                    }

                    match pool.queue.claim_next().await {
                        Ok(Some(job)) => {
                            pool.process_job(job).await;
                            continue;
                        }
                        Ok(None) => {}
                        Err(err) => {
                            warn!(worker = worker_idx, error = %err, "failed to claim transcription job");
                        }
                    }

                    tokio::select! {
                        _ = pool.queue.notify().notified() => {}
                        _ = tokio::time::sleep(Duration::from_secs(2)) => {}
                        _ = rx.changed() => {}
                    }
                }
            });
            handles.push(handle);
        }

        handles
    }

    async fn process_job(&self, job: TranscriptionJobRecord) {
        let started = Instant::now();
        let _ = self
            .db
            .upsert_transcription(UpsertTranscription {
                message_id: &job.message_id,
                session_id: &job.session_id,
                status: "processing",
                text: None,
                language: job.language_override.as_deref(),
                engine: self.engine.engine_name(),
                model: self.engine.model_name(),
                duration_ms: None,
                processing_ms: None,
                segments_json: "[]",
                error: None,
            })
            .await;

        let temp_file = self.tmp_dir.join(format!("tx_{}.audio", job.id));
        let outcome = self.run_pipeline(&job, &temp_file).await;

        if self.config.delete_temp_audio {
            let _ = tokio::fs::remove_file(&temp_file).await;
        }

        let processing_ms = started.elapsed().as_millis() as i64;

        match outcome {
            Ok((duration_ms, output)) => {
                let segments_json =
                    serde_json::to_string(&output.segments).unwrap_or_else(|_| "[]".to_string());
                let _ = self
                    .db
                    .upsert_transcription(UpsertTranscription {
                        message_id: &job.message_id,
                        session_id: &job.session_id,
                        status: "completed",
                        text: Some(&output.text),
                        language: output.language.as_deref(),
                        engine: self.engine.engine_name(),
                        model: self.engine.model_name(),
                        duration_ms: Some(duration_ms),
                        processing_ms: Some(processing_ms),
                        segments_json: &segments_json,
                        error: None,
                    })
                    .await;
                let _ = self.queue.mark_completed(&job.id).await;

                let payload = TranscriptionResult {
                    status: "completed".to_string(),
                    text: Some(output.text),
                    language: output.language,
                    engine: self.engine.engine_name().to_string(),
                    model: self.engine.model_name().to_string(),
                    duration_ms: Some(duration_ms),
                    processing_ms: Some(processing_ms),
                    segments: output.segments,
                    error: None,
                };
                let _ = self
                    .events
                    .publish(
                        "message.transcription.completed",
                        &job.session_id,
                        serde_json::json!({
                            "message_id": job.message_id,
                            "transcription": payload,
                        }),
                    )
                    .await;
            }
            Err(err) => {
                let status = if matches!(err, TranscriptionError::Unsupported(_)) {
                    "unsupported"
                } else {
                    "failed"
                };
                let err_msg = err.to_string();
                let _ = self
                    .db
                    .upsert_transcription(UpsertTranscription {
                        message_id: &job.message_id,
                        session_id: &job.session_id,
                        status,
                        text: None,
                        language: job.language_override.as_deref(),
                        engine: self.engine.engine_name(),
                        model: self.engine.model_name(),
                        duration_ms: None,
                        processing_ms: Some(processing_ms),
                        segments_json: "[]",
                        error: Some(&err_msg),
                    })
                    .await;
                let _ = self.queue.mark_failed(&job.id, &err_msg).await;

                let _ = self
                    .events
                    .publish(
                        "message.transcription.failed",
                        &job.session_id,
                        serde_json::json!({
                            "message_id": job.message_id,
                            "status": status,
                            "error": err_msg,
                        }),
                    )
                    .await;
            }
        }
    }

    async fn run_pipeline(
        &self,
        job: &TranscriptionJobRecord,
        temp_file: &PathBuf,
    ) -> Result<(i64, crate::transcription::EngineTranscriptionOutput), TranscriptionError> {
        let media = self
            .db
            .get_media_object(&job.media_id)
            .await
            .map_err(|e| TranscriptionError::AudioDecode(format!("db lookup failed: {e}")))?
            .ok_or_else(|| {
                TranscriptionError::AudioDecode(format!("media object not found: {}", job.media_id))
            })?;

        let raw_bytes = self
            .media
            .get(&media.object_key)
            .await
            .map_err(|e| TranscriptionError::AudioDecode(format!("media fetch failed: {e}")))?;

        // Write temporary file in DATA_DIR/tmp to exercise configured temp retention policy
        let _ = tokio::fs::create_dir_all(&self.tmp_dir).await;
        tokio::fs::write(temp_file, &raw_bytes).await?;

        let mime_hint = media.mime_type.clone();
        let bytes_for_decode = raw_bytes.to_vec();
        let decoded = tokio::task::spawn_blocking(move || {
            decode_and_resample_to_16k_mono(&bytes_for_decode, Some(&mime_hint))
        })
        .await
        .map_err(|e| TranscriptionError::AudioDecode(format!("decode task failed: {e}")))??;

        let timeout = Duration::from_secs(self.config.timeout_seconds);
        let output = tokio::time::timeout(
            timeout,
            self.engine
                .transcribe_pcm_16k_mono(decoded.samples_16k_mono, job.language_override.clone()),
        )
        .await
        .map_err(|_| TranscriptionError::Timeout(self.config.timeout_seconds))??;

        Ok((decoded.duration_ms, output))
    }
}
