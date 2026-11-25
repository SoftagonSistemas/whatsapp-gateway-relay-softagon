use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

use crate::core::queue::QueueError;
use crate::core::sessions::SessionError;
use crate::core::webhooks::WebhookError;
use crate::engines::whatsapp::EngineError;
use crate::storage::database::DbError;

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub error: &'static str,
    pub message: String,
}

#[derive(Debug)]
pub enum ApiError {
    BadRequest(String),
    Unauthorized(String),
    NotFound(String),
    Conflict(String),
    TooManyRequests(String),
    ServiceUnavailable(String),
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            Self::BadRequest(m) => (StatusCode::BAD_REQUEST, "bad_request", m),
            Self::Unauthorized(m) => (StatusCode::UNAUTHORIZED, "unauthorized", m),
            Self::NotFound(m) => (StatusCode::NOT_FOUND, "not_found", m),
            Self::Conflict(m) => (StatusCode::CONFLICT, "conflict", m),
            Self::TooManyRequests(m) => (StatusCode::TOO_MANY_REQUESTS, "too_many_requests", m),
            Self::ServiceUnavailable(m) => {
                (StatusCode::SERVICE_UNAVAILABLE, "service_unavailable", m)
            }
            Self::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error", m),
        };
        (
            status,
            Json(ErrorBody {
                error: code,
                message,
            }),
        )
            .into_response()
    }
}

impl From<DbError> for ApiError {
    fn from(value: DbError) -> Self {
        Self::Internal(value.to_string())
    }
}

impl From<WebhookError> for ApiError {
    fn from(value: WebhookError) -> Self {
        match value {
            WebhookError::InvalidUrl(m) | WebhookError::InvalidEventType(m) => Self::BadRequest(m),
            WebhookError::NotFound(m) => Self::NotFound(m),
            WebhookError::Db(e) => Self::Internal(e.to_string()),
        }
    }
}

impl From<SessionError> for ApiError {
    fn from(value: SessionError) -> Self {
        match value {
            SessionError::NotFound(m) | SessionError::MessageNotFound(m) => Self::NotFound(m),
            SessionError::RateLimited(m) => Self::TooManyRequests(m),
            SessionError::BadRequest(m) => Self::BadRequest(m),
            SessionError::Conflict(m) => Self::Conflict(m),
            SessionError::Queue(QueueError::QueueFull(max)) => {
                Self::TooManyRequests(format!("transcription queue is full (max {max})"))
            }
            SessionError::Queue(QueueError::Db(e)) => Self::Internal(e.to_string()),
            SessionError::Engine(EngineError::SessionNotFound(m)) => Self::NotFound(m),
            SessionError::Engine(EngineError::NotConnected(m)) => {
                Self::Conflict(format!("session is not connected: {m}"))
            }
            SessionError::Engine(EngineError::InvalidRecipient(m)) => Self::BadRequest(m),
            SessionError::Engine(e) => Self::ServiceUnavailable(e.to_string()),
            SessionError::Db(e) => Self::Internal(e.to_string()),
            SessionError::Media(e) => Self::Internal(e.to_string()),
        }
    }
}
