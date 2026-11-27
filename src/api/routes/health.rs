use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Serialize;

use crate::api::AppState;

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub database: &'static str,
    pub media_storage: &'static str,
    pub transcription_enabled: bool,
    pub transcription_available: bool,
    pub active_sessions: usize,
    pub total_sessions: usize,
    pub sessions_healthy: bool,
}

pub async fn health(State(state): State<AppState>) -> (StatusCode, Json<HealthResponse>) {
    let db_ok = state.db.health_check().await.is_ok();
    let sessions = state.sessions.list_sessions().await.unwrap_or_default();
    let total_sessions = sessions.len();
    let active_sessions = sessions
        .into_iter()
        .filter(|x| x.status == "connected")
        .count();
    let sessions_healthy = total_sessions == 0 || active_sessions == total_sessions;

    let status = if db_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        status,
        Json(HealthResponse {
            status: if db_ok { "ok" } else { "degraded" },
            database: if db_ok { "ok" } else { "error" },
            media_storage: state.media.backend_name(),
            transcription_enabled: state.config.transcription.enabled,
            transcription_available: state.transcription_engine.is_available(),
            active_sessions,
            total_sessions,
            sessions_healthy,
        }),
    )
}
