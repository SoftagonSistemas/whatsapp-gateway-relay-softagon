use axum::Json;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
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

/// Prometheus text-format metrics endpoint (`GET /v1/metrics`).
pub async fn metrics(State(state): State<AppState>) -> Response {
    let sessions = state.sessions.list_sessions().await.unwrap_or_default();
    let total_sessions = sessions.len();
    let connected_sessions = sessions.iter().filter(|s| s.status == "connected").count();
    let total_messages = state.db.count_messages_total().await.unwrap_or(0);
    let webhooks_pending = state
        .db
        .count_webhook_deliveries_by_status("pending")
        .await
        .unwrap_or(0);
    let webhooks_delivered = state
        .db
        .count_webhook_deliveries_by_status("delivered")
        .await
        .unwrap_or(0);
    let webhooks_dead_letter = state
        .db
        .count_webhook_deliveries_by_status("dead_letter")
        .await
        .unwrap_or(0);
    let (media_objects, media_bytes) = state.db.media_storage_stats().await.unwrap_or((0, 0));

    let body = format!(
        "# HELP softagon_relay_sessions_total Total configured WhatsApp sessions.\n\
         # TYPE softagon_relay_sessions_total gauge\n\
         softagon_relay_sessions_total {total_sessions}\n\
         # HELP softagon_relay_sessions_connected Currently connected WhatsApp sessions.\n\
         # TYPE softagon_relay_sessions_connected gauge\n\
         softagon_relay_sessions_connected {connected_sessions}\n\
         # HELP softagon_relay_messages_total Total messages stored in SQLite.\n\
         # TYPE softagon_relay_messages_total counter\n\
         softagon_relay_messages_total {total_messages}\n\
         # HELP softagon_relay_webhook_deliveries_total Webhook deliveries grouped by status.\n\
         # TYPE softagon_relay_webhook_deliveries_total gauge\n\
         softagon_relay_webhook_deliveries_total{{status=\"pending\"}} {webhooks_pending}\n\
         softagon_relay_webhook_deliveries_total{{status=\"delivered\"}} {webhooks_delivered}\n\
         softagon_relay_webhook_deliveries_total{{status=\"dead_letter\"}} {webhooks_dead_letter}\n\
         # HELP softagon_relay_media_objects_active Stored media objects not yet purged.\n\
         # TYPE softagon_relay_media_objects_active gauge\n\
         softagon_relay_media_objects_active {media_objects}\n\
         # HELP softagon_relay_media_storage_bytes Total active media storage bytes.\n\
         # TYPE softagon_relay_media_storage_bytes gauge\n\
         softagon_relay_media_storage_bytes {media_bytes}\n"
    );

    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
        .into_response()
}
