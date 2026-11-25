pub mod auth;
pub mod console;
pub mod docs;
pub mod error;
pub mod routes;

use std::sync::Arc;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::header;
use axum::middleware;
use axum::routing::{get, post};
use tower_http::sensitive_headers::SetSensitiveRequestHeadersLayer;
use tower_http::trace::TraceLayer;

use crate::config::AppConfig;
use crate::core::sessions::SessionManager;
use crate::core::webhooks::WebhookService;
use crate::storage::database::Database;
use crate::storage::media::MediaStorage;
use crate::transcription::TranscriptionEngine;

#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub db: Database,
    pub media: Arc<dyn MediaStorage>,
    pub sessions: Arc<SessionManager>,
    pub webhooks: WebhookService,
    pub transcription_engine: Arc<dyn TranscriptionEngine>,
}

pub fn build_router(state: AppState) -> Router {
    let protected_v1 = Router::new()
        .route(
            "/v1/sessions",
            get(routes::sessions::list_sessions).post(routes::sessions::create_session),
        )
        .route(
            "/v1/sessions/{session_id}",
            get(routes::sessions::get_session).delete(routes::sessions::delete_session),
        )
        .route(
            "/v1/sessions/{session_id}/qr",
            get(routes::sessions::get_qr),
        )
        .route(
            "/v1/sessions/{session_id}/qr.svg",
            get(routes::sessions::get_qr_svg),
        )
        .route(
            "/v1/sessions/{session_id}/status",
            get(routes::sessions::get_status),
        )
        .route(
            "/v1/sessions/{session_id}/reconnect",
            post(routes::sessions::reconnect_session),
        )
        .route(
            "/v1/sessions/{session_id}/logout",
            post(routes::sessions::logout_session),
        )
        .route(
            "/v1/sessions/{session_id}/messages/text",
            post(routes::messages::send_text),
        )
        .route(
            "/v1/sessions/{session_id}/messages/media",
            post(routes::messages::send_media),
        )
        .route(
            "/v1/messages/{message_id}",
            get(routes::messages::get_message),
        )
        .route(
            "/v1/messages/{message_id}/transcription",
            get(routes::transcriptions::get_transcription)
                .post(routes::transcriptions::request_transcription),
        )
        .route(
            "/v1/webhooks",
            get(routes::webhooks::list_webhooks).post(routes::webhooks::create_webhook),
        )
        .route(
            "/v1/webhooks/{webhook_id}",
            get(routes::webhooks::get_webhook)
                .patch(routes::webhooks::update_webhook)
                .delete(routes::webhooks::delete_webhook),
        );

    let protected = if state.config.docs_enabled && !state.config.docs_public {
        protected_v1
            .route("/docs", get(docs::scalar_docs))
            .route("/openapi.yaml", get(docs::openapi_yaml))
    } else {
        protected_v1
    }
    .route_layer(middleware::from_fn_with_state(
        state.clone(),
        auth::require_bearer_auth,
    ));

    let mut public = Router::new()
        .route("/", get(console::redirect_to_console))
        .route("/manager", get(console::redirect_to_console))
        .route("/manager/{*path}", get(console::redirect_to_console))
        .route("/console", get(console::redirect_to_console))
        .route("/console/", get(console::console_index))
        .route("/console/{*path}", get(console::console_fallback))
        .route("/v1/health", get(routes::health::health));
    if state.config.docs_enabled && state.config.docs_public {
        public = public
            .route("/docs", get(docs::scalar_docs))
            .route("/openapi.yaml", get(docs::openapi_yaml));
    }

    public
        .merge(protected)
        .with_state(state.clone())
        .layer(DefaultBodyLimit::max(state.config.media_max_upload_bytes))
        .layer(SetSensitiveRequestHeadersLayer::new([
            header::AUTHORIZATION,
        ]))
        .layer(TraceLayer::new_for_http())
}
