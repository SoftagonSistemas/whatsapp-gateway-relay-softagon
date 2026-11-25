use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use qrcode::QrCode;
use qrcode::render::svg;
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
    pub status_reason: Option<String>,
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

pub async fn get_qr_svg(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Response, ApiError> {
    let Some(qr) = state.sessions.get_qr(&session_id).await? else {
        return Err(ApiError::NotFound(
            "no active QR code for this session (already connected or expired)".into(),
        ));
    };
    let code = QrCode::new(qr.code.as_bytes())
        .map_err(|e| ApiError::Internal(format!("qr render error: {e}")))?;
    let svg_str = code
        .render::<svg::Color>()
        .min_dimensions(280, 280)
        .quiet_zone(true)
        .dark_color(svg::Color("#0f172a"))
        .light_color(svg::Color("#ffffff"))
        .build();

    Ok((
        StatusCode::OK,
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("image/svg+xml; charset=utf-8"),
            ),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static("no-store, max-age=0"),
            ),
        ],
        svg_str,
    )
        .into_response())
}

pub async fn reconnect_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<SessionRecord>, ApiError> {
    let session = state.sessions.reconnect_session(&session_id).await?;
    Ok(Json(session))
}

pub async fn get_status(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<SessionStatusResponse>, ApiError> {
    let (rec, status) = state.sessions.get_status(&session_id).await?;
    Ok(Json(SessionStatusResponse {
        session_id: rec.id,
        status,
        status_reason: rec.status_reason,
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

pub async fn delete_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state.sessions.delete_session(&session_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
