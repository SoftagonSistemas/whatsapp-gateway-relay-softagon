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
    let webhook = state
        .webhooks
        .create(
            &body.url,
            body.secret.as_deref(),
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
    let webhook = state
        .webhooks
        .update(
            &webhook_id,
            body.url.as_deref(),
            body.secret.as_deref(),
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
