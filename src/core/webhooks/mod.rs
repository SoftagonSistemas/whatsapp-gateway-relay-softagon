use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use rand::Rng;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use thiserror::Error;
use tokio::sync::{Notify, watch};
use tracing::{debug, info, warn};

use crate::config::WebhookConfig;
use crate::core::events::ALL_EVENT_TYPES;
use crate::storage::database::webhooks::{
    NewWebhook, UpdateWebhook, WebhookDeliveryRow, WebhookRecord,
};
use crate::storage::database::{Database, DbError};

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Error)]
pub enum WebhookError {
    #[error("invalid webhook url: {0}")]
    InvalidUrl(String),
    #[error("invalid event type: {0}")]
    InvalidEventType(String),
    #[error("webhook not found: {0}")]
    NotFound(String),
    #[error("database error: {0}")]
    Db(#[from] DbError),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookView {
    pub id: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_header_name: Option<String>,
    pub has_auth_header: bool,
    pub session_id: Option<String>,
    pub events: Vec<String>,
    pub enabled: bool,
    pub description: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl WebhookView {
    pub fn from_record(rec: WebhookRecord, include_secret: bool) -> Self {
        let events = serde_json::from_str(&rec.events_json).unwrap_or_default();
        let has_auth_header = rec.auth_header_name.is_some() && rec.auth_header_value.is_some();
        Self {
            id: rec.id,
            url: rec.url,
            secret: if include_secret {
                Some(rec.secret)
            } else {
                None
            },
            auth_header_name: rec.auth_header_name,
            has_auth_header,
            session_id: rec.session_id,
            events,
            enabled: rec.enabled,
            description: rec.description,
            created_at: rec.created_at,
            updated_at: rec.updated_at,
        }
    }
}

pub fn validate_webhook_url(raw: &str) -> Result<(), WebhookError> {
    let parsed = url::Url::parse(raw).map_err(|e| WebhookError::InvalidUrl(e.to_string()))?;
    match parsed.scheme() {
        "http" | "https" => Ok(()),
        other => Err(WebhookError::InvalidUrl(format!(
            "unsupported scheme '{other}', expected http or https"
        ))),
    }
}

pub fn validate_event_types(events: &[String]) -> Result<(), WebhookError> {
    for ev in events {
        if ev != "*" && !ALL_EVENT_TYPES.contains(&ev.as_str()) {
            return Err(WebhookError::InvalidEventType(ev.clone()));
        }
    }
    Ok(())
}

pub fn sign_payload(secret: &str, timestamp: i64, body: &[u8]) -> String {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts keys of any size");
    mac.update(timestamp.to_string().as_bytes());
    mac.update(b".");
    mac.update(body);
    let result = mac.finalize().into_bytes();
    format!("sha256={}", hex::encode(result))
}

pub fn generate_webhook_secret() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill(&mut bytes);
    format!("whsec_{}", hex::encode(bytes))
}

pub fn compute_next_attempt(
    now: DateTime<Utc>,
    attempts_after_failure: i64,
    max_attempts: i64,
    backoff_base_seconds: u64,
) -> Option<DateTime<Utc>> {
    if attempts_after_failure >= max_attempts {
        return None;
    }
    let exp = (attempts_after_failure.saturating_sub(1)).min(10) as u32;
    let base_delay = backoff_base_seconds.saturating_mul(1u64 << exp);
    let jitter_ms: u64 = rand::rng().random_range(0..=500);
    let delay = chrono::Duration::seconds(base_delay as i64)
        + chrono::Duration::milliseconds(jitter_ms as i64);
    Some(now + delay)
}

#[derive(Clone)]
pub struct WebhookService {
    db: Database,
}

impl WebhookService {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        &self,
        url: &str,
        secret: Option<&str>,
        auth_header_name: Option<&str>,
        auth_header_value: Option<&str>,
        session_id: Option<&str>,
        events: Vec<String>,
        enabled: bool,
        description: Option<&str>,
    ) -> Result<WebhookView, WebhookError> {
        validate_webhook_url(url)?;
        let normalized_events = if events.is_empty() {
            vec!["message.received".to_string(), "message.sent".to_string()]
        } else {
            validate_event_types(&events)?;
            events
        };
        let secret_val = secret
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(generate_webhook_secret);

        let clean_hdr_name = auth_header_name.map(str::trim).filter(|s| !s.is_empty());
        let clean_hdr_val = auth_header_value.map(str::trim).filter(|s| !s.is_empty());

        let id = format!("wh_{}", uuid::Uuid::now_v7());
        let rec = self
            .db
            .create_webhook(NewWebhook {
                id: &id,
                url,
                secret: &secret_val,
                auth_header_name: clean_hdr_name,
                auth_header_value: clean_hdr_val,
                session_id,
                events: &normalized_events,
                enabled,
                description,
            })
            .await?;

        Ok(WebhookView::from_record(rec, true))
    }

    pub async fn list(&self) -> Result<Vec<WebhookView>, WebhookError> {
        let rows = self.db.list_webhooks().await?;
        Ok(rows
            .into_iter()
            .map(|r| WebhookView::from_record(r, false))
            .collect())
    }

    pub async fn get(&self, id: &str) -> Result<WebhookView, WebhookError> {
        let rec = self
            .db
            .get_webhook(id)
            .await?
            .ok_or_else(|| WebhookError::NotFound(id.to_string()))?;
        Ok(WebhookView::from_record(rec, false))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update(
        &self,
        id: &str,
        url: Option<&str>,
        secret: Option<&str>,
        auth_header_name: Option<Option<&str>>,
        auth_header_value: Option<Option<&str>>,
        events: Option<&[String]>,
        enabled: Option<bool>,
        description: Option<Option<&str>>,
    ) -> Result<WebhookView, WebhookError> {
        if let Some(u) = url {
            validate_webhook_url(u)?;
        }
        if let Some(ev) = events {
            validate_event_types(ev)?;
        }
        let rec = self
            .db
            .update_webhook(
                id,
                UpdateWebhook {
                    url,
                    secret,
                    auth_header_name,
                    auth_header_value,
                    events,
                    enabled,
                    description,
                },
            )
            .await?
            .ok_or_else(|| WebhookError::NotFound(id.to_string()))?;
        Ok(WebhookView::from_record(rec, false))
    }

    pub async fn delete(&self, id: &str) -> Result<(), WebhookError> {
        let deleted = self.db.delete_webhook(id).await?;
        if deleted {
            Ok(())
        } else {
            Err(WebhookError::NotFound(id.to_string()))
        }
    }
}

pub struct WebhookDispatcher {
    db: Database,
    client: Client,
    notify: Arc<Notify>,
    config: WebhookConfig,
}

impl WebhookDispatcher {
    pub fn new(db: Database, notify: Arc<Notify>, config: WebhookConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.delivery_timeout_seconds))
            .user_agent("softagon-relay/0.1")
            .build()
            .expect("failed to construct reqwest client");
        Self {
            db,
            client,
            notify,
            config,
        }
    }

    pub fn spawn(self, mut shutdown: watch::Receiver<bool>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            info!("webhook dispatcher started");
            loop {
                if *shutdown.borrow() {
                    info!("webhook dispatcher shutting down");
                    break;
                }

                match self.dispatch_batch().await {
                    Ok(processed) if processed > 0 => continue,
                    Ok(_) => {}
                    Err(err) => {
                        warn!(error = %err, "webhook dispatcher batch error");
                    }
                }

                tokio::select! {
                    _ = self.notify.notified() => {}
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                    _ = shutdown.changed() => {}
                }
            }
        })
    }

    async fn dispatch_batch(&self) -> Result<usize, DbError> {
        let due = self.db.fetch_due_webhook_deliveries(20).await?;
        let count = due.len();
        for delivery in due {
            self.deliver_one(delivery).await;
        }
        Ok(count)
    }

    async fn deliver_one(&self, delivery: WebhookDeliveryRow) {
        let ts = Utc::now().timestamp();
        let signature = sign_payload(&delivery.secret, ts, delivery.payload_json.as_bytes());

        let mut req = self
            .client
            .post(&delivery.url)
            .header("Content-Type", "application/json")
            .header("X-Relay-Event-Id", &delivery.event_id)
            .header("X-Relay-Event-Type", &delivery.event_type)
            .header("X-Relay-Timestamp", ts.to_string())
            .header("X-Relay-Signature", signature);

        if let (Some(hdr_name), Some(hdr_val)) = (
            delivery.auth_header_name.as_deref(),
            delivery.auth_header_value.as_deref(),
        ) && !hdr_name.is_empty()
            && !hdr_val.is_empty()
        {
            req = req.header(hdr_name, hdr_val);
        }

        let result = req.body(delivery.payload_json.clone()).send().await;

        let attempts_after = delivery.attempts + 1;

        match result {
            Ok(resp) if resp.status().is_success() => {
                let code = resp.status().as_u16();
                if let Err(err) = self
                    .db
                    .mark_webhook_delivery_succeeded(&delivery.id, code)
                    .await
                {
                    warn!(delivery_id = %delivery.id, error = %err, "failed to mark delivery succeeded");
                } else {
                    debug!(delivery_id = %delivery.id, status = code, "webhook delivered");
                }
            }
            Ok(resp) => {
                let code = resp.status().as_u16();
                // Do not retry permanent authentication/authorization rejections (401/403)
                let next = if code == 401 || code == 403 {
                    None
                } else {
                    compute_next_attempt(
                        Utc::now(),
                        attempts_after,
                        delivery.max_attempts,
                        self.config.backoff_base_seconds,
                    )
                };
                let msg = format!("non-2xx HTTP status: {code}");
                let _ = self
                    .db
                    .mark_webhook_delivery_failed(&delivery.id, next, Some(code), &msg)
                    .await;
                warn!(
                    delivery_id = %delivery.id,
                    status = code,
                    attempt = attempts_after,
                    max_attempts = delivery.max_attempts,
                    "webhook delivery failed"
                );
            }
            Err(err) => {
                let next = compute_next_attempt(
                    Utc::now(),
                    attempts_after,
                    delivery.max_attempts,
                    self.config.backoff_base_seconds,
                );
                let msg = err.to_string();
                let _ = self
                    .db
                    .mark_webhook_delivery_failed(&delivery.id, next, None, &msg)
                    .await;
                warn!(
                    delivery_id = %delivery.id,
                    error = %msg,
                    attempt = attempts_after,
                    max_attempts = delivery.max_attempts,
                    "webhook delivery request error"
                );
            }
        }
    }
}
