pub mod audio;
pub mod whisper;
pub mod worker;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TranscriptionError {
    #[error("audio decoding failed: {0}")]
    AudioDecode(String),
    #[error("audio resampling failed: {0}")]
    Resample(String),
    #[error("transcription engine unsupported: {0}")]
    Unsupported(String),
    #[error("model download or load failed: {0}")]
    Model(String),
    #[error("inference failed: {0}")]
    Inference(String),
    #[error("transcription timed out after {0}s")]
    Timeout(u64),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionSegment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionResult {
    pub status: String,
    pub text: Option<String>,
    pub language: Option<String>,
    pub engine: String,
    pub model: String,
    pub duration_ms: Option<i64>,
    pub processing_ms: Option<i64>,
    pub segments: Vec<TranscriptionSegment>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EngineTranscriptionOutput {
    pub text: String,
    pub language: Option<String>,
    pub segments: Vec<TranscriptionSegment>,
}

#[async_trait]
pub trait TranscriptionEngine: Send + Sync {
    fn engine_name(&self) -> &str;
    fn model_name(&self) -> &str;
    fn is_available(&self) -> bool;

    async fn preload(&self) -> Result<(), TranscriptionError> {
        Ok(())
    }

    async fn transcribe_pcm_16k_mono(
        &self,
        samples: Vec<f32>,
        language: Option<String>,
    ) -> Result<EngineTranscriptionOutput, TranscriptionError>;
}
