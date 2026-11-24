#[cfg(feature = "whisper")]
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(feature = "whisper")]
use std::time::{Duration, Instant};

use async_trait::async_trait;
#[cfg(feature = "whisper")]
use tokio::sync::Mutex;
#[cfg(feature = "whisper")]
use tracing::{info, warn};

use crate::config::TranscriptionConfig;
use crate::transcription::{
    EngineTranscriptionOutput, TranscriptionEngine, TranscriptionError, TranscriptionSegment,
};

pub fn build_transcription_engine(config: &TranscriptionConfig) -> Arc<dyn TranscriptionEngine> {
    #[cfg(feature = "whisper")]
    {
        Arc::new(WhisperEngine::new(config.clone()))
    }
    #[cfg(not(feature = "whisper"))]
    {
        Arc::new(DisabledWhisperEngine {
            model: config.model.as_str().to_string(),
        })
    }
}

/// Fallback engine used when the binary is compiled without `--features whisper`.
pub struct DisabledWhisperEngine {
    model: String,
}

#[async_trait]
impl TranscriptionEngine for DisabledWhisperEngine {
    fn engine_name(&self) -> &str {
        "whisper"
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn is_available(&self) -> bool {
        false
    }

    async fn transcribe_pcm_16k_mono(
        &self,
        _samples: Vec<f32>,
        _language: Option<String>,
    ) -> Result<EngineTranscriptionOutput, TranscriptionError> {
        Err(TranscriptionError::Unsupported(
            "binary built without the 'whisper' Cargo feature; rebuild with --features whisper"
                .to_string(),
        ))
    }
}

/// Deterministic mock engine for automated tests.
pub struct MockTranscriptionEngine {
    pub response_text: String,
    pub detected_language: String,
    pub model: String,
}

impl MockTranscriptionEngine {
    pub fn new(text: impl Into<String>, language: impl Into<String>) -> Self {
        Self {
            response_text: text.into(),
            detected_language: language.into(),
            model: "base".to_string(),
        }
    }
}

#[async_trait]
impl TranscriptionEngine for MockTranscriptionEngine {
    fn engine_name(&self) -> &str {
        "whisper"
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn is_available(&self) -> bool {
        true
    }

    async fn transcribe_pcm_16k_mono(
        &self,
        samples: Vec<f32>,
        language: Option<String>,
    ) -> Result<EngineTranscriptionOutput, TranscriptionError> {
        let duration_ms = ((samples.len() as f64 / 16_000.0) * 1000.0).round() as i64;
        Ok(EngineTranscriptionOutput {
            text: self.response_text.clone(),
            language: language.or_else(|| Some(self.detected_language.clone())),
            segments: vec![TranscriptionSegment {
                start_ms: 0,
                end_ms: duration_ms,
                text: self.response_text.clone(),
            }],
        })
    }
}

#[cfg(feature = "whisper")]
struct LoadedModel {
    ctx: Arc<whisper_rs::WhisperContext>,
    last_used: Instant,
}

#[cfg(feature = "whisper")]
pub struct WhisperEngine {
    config: TranscriptionConfig,
    loaded: Arc<Mutex<Option<LoadedModel>>>,
}

#[cfg(feature = "whisper")]
impl WhisperEngine {
    pub fn new(config: TranscriptionConfig) -> Self {
        let loaded = Arc::new(Mutex::new(None::<LoadedModel>));
        if config.idle_unload_seconds > 0 {
            let loaded_bg = loaded.clone();
            let idle_ttl = Duration::from_secs(config.idle_unload_seconds);
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(30));
                loop {
                    interval.tick().await;
                    let mut guard = loaded_bg.lock().await;
                    if let Some(model) = guard.as_ref()
                        && model.last_used.elapsed() >= idle_ttl
                    {
                        info!("unloading idle Whisper model to reclaim memory");
                        *guard = None;
                    }
                }
            });
        }
        Self { config, loaded }
    }

    fn model_path(&self) -> PathBuf {
        self.config
            .model_dir
            .join(self.config.model.ggml_filename())
    }

    async fn ensure_model_downloaded(&self) -> Result<PathBuf, TranscriptionError> {
        let path = self.model_path();
        if path.exists() {
            return Ok(path);
        }

        tokio::fs::create_dir_all(&self.config.model_dir).await?;
        let url = self.config.model.download_url();
        info!(model = %self.config.model.as_str(), url = %url, "downloading Whisper model");

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(600))
            .build()
            .map_err(|e| TranscriptionError::Model(format!("http client build failed: {e}")))?;

        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(|e| TranscriptionError::Model(format!("model download failed: {e}")))?;

        if !resp.status().is_success() {
            return Err(TranscriptionError::Model(format!(
                "model download returned HTTP {}",
                resp.status()
            )));
        }

        let tmp_path = path.with_extension("bin.part");
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| TranscriptionError::Model(format!("model read failed: {e}")))?;
        tokio::fs::write(&tmp_path, &bytes).await?;
        tokio::fs::rename(&tmp_path, &path).await?;

        info!(path = %path.display(), bytes = bytes.len(), "downloaded Whisper model");
        Ok(path)
    }

    async fn get_or_load_context(
        &self,
    ) -> Result<Arc<whisper_rs::WhisperContext>, TranscriptionError> {
        {
            let mut guard = self.loaded.lock().await;
            if let Some(m) = guard.as_mut() {
                m.last_used = Instant::now();
                return Ok(m.ctx.clone());
            }
        }

        let path = self.ensure_model_downloaded().await?;
        let path_str = path.to_string_lossy().to_string();

        let ctx = tokio::task::spawn_blocking(move || {
            whisper_rs::WhisperContext::new_with_params(
                &path_str,
                whisper_rs::WhisperContextParameters::default(),
            )
        })
        .await
        .map_err(|e| TranscriptionError::Model(format!("model load task failed: {e}")))?
        .map_err(|e| TranscriptionError::Model(format!("whisper context init failed: {e}")))?;

        let arc_ctx = Arc::new(ctx);
        let mut guard = self.loaded.lock().await;
        *guard = Some(LoadedModel {
            ctx: arc_ctx.clone(),
            last_used: Instant::now(),
        });
        Ok(arc_ctx)
    }
}

#[cfg(feature = "whisper")]
#[async_trait]
impl TranscriptionEngine for WhisperEngine {
    fn engine_name(&self) -> &str {
        "whisper"
    }

    fn model_name(&self) -> &str {
        self.config.model.as_str()
    }

    fn is_available(&self) -> bool {
        true
    }

    async fn preload(&self) -> Result<(), TranscriptionError> {
        let _ = self.get_or_load_context().await?;
        Ok(())
    }

    async fn transcribe_pcm_16k_mono(
        &self,
        samples: Vec<f32>,
        language: Option<String>,
    ) -> Result<EngineTranscriptionOutput, TranscriptionError> {
        let ctx = self.get_or_load_context().await?;
        let threads = self.config.threads as i32;

        let result = tokio::task::spawn_blocking(move || {
            let mut state = ctx
                .create_state()
                .map_err(|e| TranscriptionError::Inference(format!("create_state failed: {e}")))?;

            let mut params =
                whisper_rs::FullParams::new(whisper_rs::SamplingStrategy::Greedy { best_of: 1 });
            params.set_n_threads(threads);
            params.set_print_special(false);
            params.set_print_progress(false);
            params.set_print_realtime(false);
            params.set_print_timestamps(false);
            if let Some(ref lang) = language {
                params.set_language(Some(lang.as_str()));
            } else {
                params.set_language(None);
            }

            state.full(params, &samples).map_err(|e| {
                TranscriptionError::Inference(format!("whisper full() failed: {e}"))
            })?;

            let num_segments = state.full_n_segments();
            let mut segments = Vec::with_capacity(num_segments.max(0) as usize);
            let mut full_text = String::new();

            for i in 0..num_segments {
                if let Some(seg) = state.get_segment(i) {
                    let start_ms = seg.start_timestamp() * 10;
                    let end_ms = seg.end_timestamp() * 10;
                    let text = seg
                        .to_str_lossy()
                        .map(|s| s.trim().to_string())
                        .unwrap_or_default();
                    if !text.is_empty() {
                        if !full_text.is_empty() {
                            full_text.push(' ');
                        }
                        full_text.push_str(&text);
                        segments.push(TranscriptionSegment {
                            start_ms,
                            end_ms,
                            text,
                        });
                    }
                }
            }

            let detected_lang = language.or_else(|| {
                let lang_id = state.full_lang_id_from_state();
                whisper_rs::get_lang_str(lang_id).map(ToString::to_string)
            });

            Ok::<EngineTranscriptionOutput, TranscriptionError>(EngineTranscriptionOutput {
                text: full_text,
                language: detected_lang,
                segments,
            })
        })
        .await
        .map_err(|e| TranscriptionError::Inference(format!("inference task panicked: {e}")))??;

        if let Ok(mut guard) = self.loaded.try_lock()
            && let Some(m) = guard.as_mut()
        {
            m.last_used = Instant::now();
        } else {
            warn!("could not update last_used timestamp on whisper model");
        }

        Ok(result)
    }
}
