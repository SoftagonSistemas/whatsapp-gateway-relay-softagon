use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;

use crate::api::AppState;
use crate::api::error::ApiError;
use crate::storage::database::transcriptions::TranscriptionRecord;
use crate::transcription::{TranscriptionResult, TranscriptionSegment};

#[derive(Debug, Default, Deserialize)]
pub struct RequestTranscriptionBody {
    pub language: Option<String>,
}

fn to_api_result(rec: TranscriptionRecord) -> TranscriptionResult {
    let segments: Vec<TranscriptionSegment> =
        serde_json::from_str(&rec.segments_json).unwrap_or_default();
    TranscriptionResult {
        status: rec.status,
        text: rec.text,
        language: rec.language,
        engine: rec.engine,
        model: rec.model,
        duration_ms: rec.duration_ms,
        processing_ms: rec.processing_ms,
        segments,
        error: rec.error,
    }
}

pub async fn get_transcription(
    State(state): State<AppState>,
    Path(message_id): Path<String>,
) -> Result<Json<TranscriptionResult>, ApiError> {
    let Some(rec) = state.db.get_transcription(&message_id).await? else {
        return Err(ApiError::NotFound(format!(
            "transcription not found for message {message_id}"
        )));
    };
    Ok(Json(to_api_result(rec)))
}

pub async fn request_transcription(
    State(state): State<AppState>,
    Path(message_id): Path<String>,
    body: Option<Json<RequestTranscriptionBody>>,
) -> Result<(StatusCode, Json<TranscriptionResult>), ApiError> {
    let lang = body.and_then(|b| b.0.language);
    let rec = state
        .sessions
        .request_transcription(&message_id, lang.as_deref())
        .await?;
    Ok((StatusCode::ACCEPTED, Json(to_api_result(rec))))
}
