use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::api::AppState;
use crate::api::error::ApiError;
use crate::core::webhooks::WebhookView;

#[derive(Debug, Deserialize)]
pub struct CreateWebhookRequest {
    pub url: String,
    pub secret: Option<String>,
    /// Shorthand for Authorization: Bearer <token> (takes precedence if set).
    pub bearer_token: Option<String>,
    /// Optional custom auth header name (e.g., "Authorization" or "X-Api-Key").
    pub auth_header_name: Option<String>,
    /// Optional custom auth header value (stored securely; never returned on GET).
    pub auth_header_value: Option<String>,
    pub session_id: Option<String>,
    #[serde(default)]
    pub events: Vec<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    pub description: Option<String>,
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct UpdateWebhookRequest {
    pub url: Option<String>,
    pub secret: Option<String>,
    pub bearer_token: Option<Option<String>>,
    pub auth_header_name: Option<Option<String>>,
    pub auth_header_value: Option<Option<String>>,
    pub events: Option<Vec<String>>,
    pub enabled: Option<bool>,
    pub description: Option<Option<String>>,
}

#[derive(Debug, Serialize)]
pub struct ListWebhooksResponse {
    pub webhooks: Vec<WebhookView>,
}

pub async fn list_webhooks(
    State(state): State<AppState>,
) -> Result<Json<ListWebhooksResponse>, ApiError> {
    let webhooks = state.webhooks.list().await?;
    Ok(Json(ListWebhooksResponse { webhooks }))
}

pub async fn create_webhook(
    State(state): State<AppState>,
    Json(body): Json<CreateWebhookRequest>,
) -> Result<(StatusCode, Json<WebhookView>), ApiError> {
    let (hdr_name, hdr_val) = if let Some(tok) = body
        .bearer_token
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let val = if tok.starts_with("Bearer ") {
            tok.to_string()
        } else {
            format!("Bearer {tok}")
        };
        (Some("Authorization".to_string()), Some(val))
    } else {
        (body.auth_header_name, body.auth_header_value)
    };

    let webhook = state
        .webhooks
        .create(
            &body.url,
            body.secret.as_deref(),
            hdr_name.as_deref(),
            hdr_val.as_deref(),
            body.session_id.as_deref(),
            body.events,
            body.enabled,
            body.description.as_deref(),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(webhook)))
}

pub async fn get_webhook(
    State(state): State<AppState>,
    Path(webhook_id): Path<String>,
) -> Result<Json<WebhookView>, ApiError> {
    let webhook = state.webhooks.get(&webhook_id).await?;
    Ok(Json(webhook))
}

pub async fn update_webhook(
    State(state): State<AppState>,
    Path(webhook_id): Path<String>,
    Json(body): Json<UpdateWebhookRequest>,
) -> Result<Json<WebhookView>, ApiError> {
    let desc_ref = body.description.as_ref().map(|opt| opt.as_deref());
    let (owned_name, owned_val): (Option<Option<String>>, Option<Option<String>>) =
        if let Some(bt) = body.bearer_token {
            match bt.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                Some(tok) => {
                    let val = if tok.starts_with("Bearer ") {
                        tok.to_string()
                    } else {
                        format!("Bearer {tok}")
                    };
                    (Some(Some("Authorization".to_string())), Some(Some(val)))
                }
                None => (Some(None), Some(None)),
            }
        } else {
            (body.auth_header_name, body.auth_header_value)
        };
    let name_ref = owned_name.as_ref().map(|opt| opt.as_deref());
    let val_ref = owned_val.as_ref().map(|opt| opt.as_deref());

    let webhook = state
        .webhooks
        .update(
            &webhook_id,
            body.url.as_deref(),
            body.secret.as_deref(),
            name_ref,
            val_ref,
            body.events.as_deref(),
            body.enabled,
            desc_ref,
        )
        .await?;
    Ok(Json(webhook))
}

pub async fn delete_webhook(
    State(state): State<AppState>,
    Path(webhook_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state.webhooks.delete(&webhook_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
