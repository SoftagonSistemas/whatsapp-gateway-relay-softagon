use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use bytes::Bytes;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::api::AppState;
use crate::api::error::ApiError;
use crate::engines::whatsapp::MediaKind;
use crate::storage::database::messages::MessageRecord;

#[derive(Debug, Deserialize)]
pub struct SendTextRequest {
    pub to: String,
    pub text: String,
}

#[derive(Debug, Deserialize)]
pub struct SendMediaRequest {
    pub to: String,
    pub media_type: MediaKind,
    pub mime_type: Option<String>,
    pub filename: Option<String>,
    pub caption: Option<String>,
    pub data_base64: Option<String>,
    pub media_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MarkReadRequest {
    pub chat_jid: String,
    pub sender_jid: Option<String>,
    #[serde(default)]
    pub message_ids: Vec<String>,
}

pub async fn send_text(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<SendTextRequest>,
) -> Result<Response, ApiError> {
    let idempotency_key = extract_idempotency_key(&headers);
    let req_hash = hash_request(&format!("text:{}:{}", body.to, body.text));

    if let Some(ref key) = idempotency_key
        && let Some(cached) = check_idempotency(&state, key, &session_id, &req_hash).await?
    {
        return Ok(cached);
    }

    let message = state
        .sessions
        .send_text(&session_id, &body.to, &body.text)
        .await?;

    let json_body = serde_json::to_string(&message)
        .map_err(|e| ApiError::Internal(format!("serialize error: {e}")))?;

    if let Some(ref key) = idempotency_key {
        let _ = state
            .db
            .save_idempotency_key(
                key,
                &session_id,
                &req_hash,
                StatusCode::CREATED.as_u16(),
                &json_body,
            )
            .await;
    }

    Ok((StatusCode::CREATED, Json(message)).into_response())
}

pub async fn send_media(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<SendMediaRequest>,
) -> Result<Response, ApiError> {
    let idempotency_key = extract_idempotency_key(&headers);
    let source_sig = body
        .media_url
        .as_deref()
        .map(|u| format!("url:{u}"))
        .or_else(|| {
            body.data_base64
                .as_deref()
                .map(|b| format!("b64:{}", b.len()))
        })
        .unwrap_or_default();
    let req_hash = hash_request(&format!(
        "media:{}:{}:{}:{}",
        body.to,
        body.media_type.as_str(),
        body.mime_type.as_deref().unwrap_or(""),
        source_sig
    ));

    if let Some(ref key) = idempotency_key
        && let Some(cached) = check_idempotency(&state, key, &session_id, &req_hash).await?
    {
        return Ok(cached);
    }

    let (raw_bytes, inferred_mime, inferred_name) =
        match (body.media_url.as_deref(), body.data_base64.as_deref()) {
            (Some(url), _) if !url.trim().is_empty() => {
                state.sessions.fetch_media_from_url(url).await?
            }
            (_, Some(b64)) if !b64.trim().is_empty() => {
                let decoded = base64::engine::general_purpose::STANDARD
                    .decode(b64.trim())
                    .map_err(|e| ApiError::BadRequest(format!("invalid base64 payload: {e}")))?;
                (Bytes::from(decoded), None, None)
            }
            _ => {
                return Err(ApiError::BadRequest(
                    "either media_url or data_base64 is required".into(),
                ));
            }
        };

    let effective_filename = body
        .filename
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .or(inferred_name);
    let effective_mime = body
        .mime_type
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .or(inferred_mime)
        .or_else(|| {
            effective_filename
                .as_deref()
                .and_then(|f| mime_guess::from_path(f).first())
                .map(|m| m.to_string())
        })
        .unwrap_or_else(|| match body.media_type {
            MediaKind::Image => "image/jpeg".to_string(),
            MediaKind::Video => "video/mp4".to_string(),
            MediaKind::Audio => "audio/ogg".to_string(),
            MediaKind::Document => "application/octet-stream".to_string(),
        });

    let message = state
        .sessions
        .send_media(
            &session_id,
            &body.to,
            body.media_type,
            &effective_mime,
            effective_filename.as_deref(),
            body.caption.as_deref(),
            raw_bytes,
        )
        .await?;

    let json_body = serde_json::to_string(&message)
        .map_err(|e| ApiError::Internal(format!("serialize error: {e}")))?;

    if let Some(ref key) = idempotency_key {
        let _ = state
            .db
            .save_idempotency_key(
                key,
                &session_id,
                &req_hash,
                StatusCode::CREATED.as_u16(),
                &json_body,
            )
            .await;
    }

    Ok((StatusCode::CREATED, Json(message)).into_response())
}

pub async fn mark_read(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(body): Json<MarkReadRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let marked_count = state
        .sessions
        .mark_read(
            &session_id,
            &body.chat_jid,
            body.sender_jid.as_deref(),
            body.message_ids,
        )
        .await?;
    Ok(Json(serde_json::json!({
        "session_id": session_id,
        "chat_jid": body.chat_jid,
        "marked_count": marked_count,
    })))
}

pub async fn get_contact(
    State(state): State<AppState>,
    Path((session_id, jid)): Path<(String, String)>,
) -> Result<Json<crate::engines::whatsapp::ContactInfo>, ApiError> {
    let contact = state.sessions.get_contact(&session_id, &jid).await?;
    Ok(Json(contact))
}

pub async fn get_message(
    State(state): State<AppState>,
    Path(message_id): Path<String>,
) -> Result<Json<MessageRecord>, ApiError> {
    let message = state.sessions.get_message(&message_id).await?;
    Ok(Json(message))
}

fn extract_idempotency_key(headers: &HeaderMap) -> Option<String> {
    headers
        .get("Idempotency-Key")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
}

fn hash_request(input: &str) -> String {
    let mut h = Sha256::new();
    h.update(input.as_bytes());
    hex::encode(h.finalize())
}

async fn check_idempotency(
    state: &AppState,
    key: &str,
    session_id: &str,
    req_hash: &str,
) -> Result<Option<Response>, ApiError> {
    let Some(existing) = state.db.get_idempotency_key(key, session_id).await? else {
        return Ok(None);
    };
    if existing.request_hash != req_hash {
        return Err(ApiError::Conflict(
            "Idempotency-Key already used with a different request payload".into(),
        ));
    }
    let status =
        StatusCode::from_u16(existing.response_status as u16).unwrap_or(StatusCode::CREATED);
    let resp = (
        status,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        existing.response_body,
    )
        .into_response();
    Ok(Some(resp))
}
