pub mod auth;
pub mod console;
pub mod docs;
pub mod error;
pub mod pairing;
pub mod routes;

use std::sync::Arc;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::header;
use axum::middleware;
use axum::routing::{get, patch, post};
use tower_http::sensitive_headers::SetSensitiveRequestHeadersLayer;
use tower_http::trace::TraceLayer;

use crate::config::AppConfig;
use crate::core::pairing::PairingLinkStore;
use crate::core::service::RelayService;
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
    pub pairing_links: PairingLinkStore,
}

impl From<RelayService> for AppState {
    fn from(svc: RelayService) -> Self {
        Self {
            config: svc.config,
            db: svc.db,
            media: svc.media,
            sessions: svc.sessions,
            webhooks: svc.webhooks,
            transcription_engine: svc.transcription_engine,
            pairing_links: svc.pairing_links,
        }
    }
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
            "/v1/sessions/{session_id}/qr.png",
            get(routes::sessions::get_qr_png),
        )
        .route(
            "/v1/sessions/{session_id}/pair-code",
            post(routes::sessions::request_pair_code),
        )
        .route(
            "/v1/sessions/{session_id}/pair-link",
            post(routes::sessions::create_pair_link),
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
            "/v1/sessions/{session_id}/messages/reaction",
            post(routes::messages::send_reaction),
        )
        .route(
            "/v1/sessions/{session_id}/messages/location",
            post(routes::messages::send_location),
        )
        .route(
            "/v1/sessions/{session_id}/messages/contact",
            post(routes::messages::send_contact),
        )
        .route(
            "/v1/sessions/{session_id}/messages/{message_id}",
            patch(routes::messages::edit_message).delete(routes::messages::revoke_message),
        )
        .route(
            "/v1/sessions/{session_id}/chats/read",
            post(routes::messages::mark_read),
        )
        .route(
            "/v1/sessions/{session_id}/chats/presence",
            post(routes::messages::send_chat_presence),
        )
        .route(
            "/v1/sessions/{session_id}/contacts/check",
            post(routes::messages::check_whatsapp),
        )
        .route(
            "/v1/sessions/{session_id}/contacts/{jid}",
            get(routes::messages::get_contact),
        )
        .route(
            "/v1/sessions/{session_id}/contacts/{jid}/profile-picture",
            get(routes::messages::get_profile_picture),
        )
        .route(
            "/v1/sessions/{session_id}/groups",
            get(routes::messages::list_groups),
        )
        .route(
            "/v1/sessions/{session_id}/groups/{group_jid}",
            get(routes::messages::get_group_info),
        )
        .route("/v1/media/{media_id}", get(routes::messages::get_media))
        .route("/v1/metrics", get(routes::health::metrics))
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
            .route("/openapi.json", get(docs::openapi_json))
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
        .route("/pair/{token}", get(pairing::pair_page))
        .route("/pair/{token}/status", get(pairing::pair_status))
        .route("/pair/{token}/qr.svg", get(pairing::pair_qr_svg))
        .route("/pair/{token}/qr.png", get(pairing::pair_qr_png))
        .route("/pair/{token}/reconnect", post(pairing::pair_reconnect))
        .route("/pair/{token}/pair-code", post(pairing::pair_request_code))
        .route("/v1/health", get(routes::health::health));
    if state.config.docs_enabled && state.config.docs_public {
        public = public
            .route("/docs", get(docs::scalar_docs))
            .route("/openapi.yaml", get(docs::openapi_yaml))
            .route("/openapi.json", get(docs::openapi_json));
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
