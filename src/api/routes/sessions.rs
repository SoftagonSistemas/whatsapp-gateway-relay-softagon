use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::api::AppState;
use crate::api::error::ApiError;
use crate::engines::whatsapp::QrPayload;
use crate::storage::database::sessions::SessionRecord;

#[derive(Debug, Deserialize)]
pub struct CreateSessionRequest {
    pub name: String,
    pub transcription_language: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ListSessionsResponse {
    pub sessions: Vec<SessionRecord>,
}

#[derive(Debug, Serialize)]
pub struct SessionQrResponse {
    pub session_id: String,
    pub qr: Option<QrPayload>,
}

#[derive(Debug, Serialize)]
pub struct SessionStatusResponse {
    pub session_id: String,
    pub status: String,
    pub phone_number: Option<String>,
    pub jid: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LogoutResponse {
    pub session_id: String,
    pub status: &'static str,
}

pub async fn list_sessions(
    State(state): State<AppState>,
) -> Result<Json<ListSessionsResponse>, ApiError> {
    let sessions = state.sessions.list_sessions().await?;
    Ok(Json(ListSessionsResponse { sessions }))
}

pub async fn create_session(
    State(state): State<AppState>,
    Json(body): Json<CreateSessionRequest>,
) -> Result<(StatusCode, Json<SessionRecord>), ApiError> {
    let session = state
        .sessions
        .create_session(&body.name, body.transcription_language.as_deref())
        .await?;
    Ok((StatusCode::CREATED, Json(session)))
}

pub async fn get_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<SessionRecord>, ApiError> {
    let session = state.sessions.get_session(&session_id).await?;
    Ok(Json(session))
}

pub async fn get_qr(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<SessionQrResponse>, ApiError> {
    let qr = state.sessions.get_qr(&session_id).await?;
    Ok(Json(SessionQrResponse { session_id, qr }))
}

pub async fn get_status(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<SessionStatusResponse>, ApiError> {
    let (rec, status) = state.sessions.get_status(&session_id).await?;
    Ok(Json(SessionStatusResponse {
        session_id: rec.id,
        status,
        phone_number: rec.phone_number,
        jid: rec.jid,
    }))
}

pub async fn logout_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<LogoutResponse>, ApiError> {
    state.sessions.logout_session(&session_id).await?;
    Ok(Json(LogoutResponse {
        session_id,
        status: "logged_out",
    }))
}
