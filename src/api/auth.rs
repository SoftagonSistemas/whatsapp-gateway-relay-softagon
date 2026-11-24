use axum::extract::{Request, State};
use axum::http::header;
use axum::middleware::Next;
use axum::response::Response;
use subtle::ConstantTimeEq;

use crate::api::AppState;
use crate::api::error::ApiError;

pub async fn require_bearer_auth(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let Some(auth_header) = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return Err(ApiError::Unauthorized(
            "missing Authorization Bearer token".into(),
        ));
    };

    let Some(token) = auth_header.strip_prefix("Bearer ") else {
        return Err(ApiError::Unauthorized(
            "Authorization header must use Bearer scheme".into(),
        ));
    };

    let expected = state.config.api_key.as_bytes();
    let provided = token.trim().as_bytes();
    let is_valid = expected.len() == provided.len() && bool::from(expected.ct_eq(provided));

    if !is_valid {
        return Err(ApiError::Unauthorized("invalid API key".into()));
    }

    Ok(next.run(req).await)
}
