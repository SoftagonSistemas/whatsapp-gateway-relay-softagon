use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use super::{Database, DbError};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct WebhookRecord {
    pub id: String,
    pub url: String,
    pub secret: String,
    pub auth_header_name: Option<String>,
    pub auth_header_value: Option<String>,
    pub session_id: Option<String>,
    pub events_json: String,
    pub enabled: bool,
    pub description: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct EventRecord {
    pub id: String,
    pub event_type: String,
    pub session_id: String,
    pub payload_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct WebhookDeliveryRow {
    pub id: String,
    pub event_id: String,
    pub webhook_id: String,
    pub url: String,
    pub secret: String,
    pub auth_header_name: Option<String>,
    pub auth_header_value: Option<String>,
    pub event_type: String,
    pub payload_json: String,
    pub attempts: i64,
    pub max_attempts: i64,
}

#[derive(Debug, Clone)]
pub struct NewWebhook<'a> {
    pub id: &'a str,
    pub url: &'a str,
    pub secret: &'a str,
    pub auth_header_name: Option<&'a str>,
    pub auth_header_value: Option<&'a str>,
    pub session_id: Option<&'a str>,
    pub events: &'a [String],
    pub enabled: bool,
    pub description: Option<&'a str>,
}

#[derive(Debug, Clone, Default)]
pub struct UpdateWebhook<'a> {
    pub url: Option<&'a str>,
    pub secret: Option<&'a str>,
    pub auth_header_name: Option<Option<&'a str>>,
    pub auth_header_value: Option<Option<&'a str>>,
    pub events: Option<&'a [String]>,
    pub enabled: Option<bool>,
    pub description: Option<Option<&'a str>>,
}

impl Database {
    pub async fn create_webhook(&self, input: NewWebhook<'_>) -> Result<WebhookRecord, DbError> {
        let now = Utc::now().to_rfc3339();
        let events_json = serde_json::to_string(input.events)?;
        sqlx::query(
            r#"
            INSERT INTO webhooks (
                id, url, secret, auth_header_name, auth_header_value,
                session_id, events_json, enabled, description, created_at, updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)
            "#,
        )
        .bind(input.id)
        .bind(input.url)
        .bind(input.secret)
        .bind(input.auth_header_name)
        .bind(input.auth_header_value)
        .bind(input.session_id)
        .bind(&events_json)
        .bind(input.enabled)
        .bind(input.description)
        .bind(&now)
        .execute(self.pool())
        .await?;

        self.get_webhook(input.id)
            .await?
            .ok_or_else(|| DbError::Sqlx(sqlx::Error::RowNotFound))
    }

    pub async fn get_webhook(&self, id: &str) -> Result<Option<WebhookRecord>, DbError> {
        let row = sqlx::query_as::<_, WebhookRecord>(
            r#"
            SELECT id, url, secret, auth_header_name, auth_header_value,
                   session_id, events_json, enabled, description, created_at, updated_at
            FROM webhooks
            WHERE id = ?1
            "#,
        )
        .bind(id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row)
    }

    pub async fn list_webhooks(&self) -> Result<Vec<WebhookRecord>, DbError> {
        let rows = sqlx::query_as::<_, WebhookRecord>(
            r#"
            SELECT id, url, secret, auth_header_name, auth_header_value,
                   session_id, events_json, enabled, description, created_at, updated_at
            FROM webhooks
            ORDER BY created_at DESC
            "#,
        )
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }

    pub async fn update_webhook(
        &self,
        id: &str,
        patch: UpdateWebhook<'_>,
    ) -> Result<Option<WebhookRecord>, DbError> {
        let Some(existing) = self.get_webhook(id).await? else {
            return Ok(None);
        };
        let url = patch.url.unwrap_or(&existing.url);
        let secret = patch.secret.unwrap_or(&existing.secret);
        let auth_header_name = match patch.auth_header_name {
            Some(v) => v.map(ToString::to_string),
            None => existing.auth_header_name,
        };
        let auth_header_value = match patch.auth_header_value {
            Some(v) => v.map(ToString::to_string),
            None => existing.auth_header_value,
        };
        let events_json = match patch.events {
            Some(ev) => serde_json::to_string(ev)?,
            None => existing.events_json,
        };
        let enabled = patch.enabled.unwrap_or(existing.enabled);
        let description = match patch.description {
            Some(d) => d.map(ToString::to_string),
            None => existing.description,
        };
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            r#"
            UPDATE webhooks
            SET url = ?2, secret = ?3, auth_header_name = ?4, auth_header_value = ?5,
                events_json = ?6, enabled = ?7, description = ?8, updated_at = ?9
            WHERE id = ?1
            "#,
        )
        .bind(id)
        .bind(url)
        .bind(secret)
        .bind(auth_header_name)
        .bind(auth_header_value)
        .bind(&events_json)
        .bind(enabled)
        .bind(description)
        .bind(&now)
        .execute(self.pool())
        .await?;

        self.get_webhook(id).await
    }

    pub async fn delete_webhook(&self, id: &str) -> Result<bool, DbError> {
        let res = sqlx::query("DELETE FROM webhooks WHERE id = ?1")
            .bind(id)
            .execute(self.pool())
            .await?;
        Ok(res.rows_affected() > 0)
    }

    pub async fn persist_event_and_enqueue_deliveries(
        &self,
        event_id: &str,
        event_type: &str,
        session_id: &str,
        payload_json: &str,
        max_attempts: u32,
    ) -> Result<usize, DbError> {
        let now = Utc::now().to_rfc3339();
        let mut tx = self.pool().begin().await?;

        sqlx::query(
            r#"
            INSERT INTO events (id, event_type, session_id, payload_json, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
        )
        .bind(event_id)
        .bind(event_type)
        .bind(session_id)
        .bind(payload_json)
        .bind(&now)
        .execute(&mut *tx)
        .await?;

        let candidates = sqlx::query_as::<_, WebhookRecord>(
            r#"
            SELECT id, url, secret, auth_header_name, auth_header_value,
                   session_id, events_json, enabled, description, created_at, updated_at
            FROM webhooks
            WHERE enabled = 1 AND (session_id IS NULL OR session_id = ?1)
            "#,
        )
        .bind(session_id)
        .fetch_all(&mut *tx)
        .await?;

        let mut enqueued = 0usize;
        for wh in candidates {
            let subscribed: Vec<String> = serde_json::from_str(&wh.events_json).unwrap_or_default();
            let matches =
                subscribed.is_empty() || subscribed.iter().any(|e| e == "*" || e == event_type);
            if !matches {
                continue;
            }
            let delivery_id = format!("dlv_{}", uuid::Uuid::now_v7());
            sqlx::query(
                r#"
                INSERT INTO webhook_deliveries (
                    id, event_id, webhook_id, status, attempts, max_attempts,
                    next_attempt_at, created_at, updated_at
                )
                VALUES (?1, ?2, ?3, 'pending', 0, ?4, ?5, ?5, ?5)
                "#,
            )
            .bind(&delivery_id)
            .bind(event_id)
            .bind(&wh.id)
            .bind(i64::from(max_attempts))
            .bind(&now)
            .execute(&mut *tx)
            .await?;
            enqueued += 1;
        }

        tx.commit().await?;
        Ok(enqueued)
    }

    pub async fn fetch_due_webhook_deliveries(
        &self,
        limit: i64,
    ) -> Result<Vec<WebhookDeliveryRow>, DbError> {
        let now = Utc::now().to_rfc3339();
        let rows = sqlx::query_as::<_, WebhookDeliveryRow>(
            r#"
            SELECT d.id, d.event_id, d.webhook_id, w.url, w.secret,
                   w.auth_header_name, w.auth_header_value,
                   e.event_type, e.payload_json, d.attempts, d.max_attempts
            FROM webhook_deliveries d
            INNER JOIN webhooks w ON w.id = d.webhook_id
            INNER JOIN events e ON e.id = d.event_id
            WHERE d.status = 'pending' AND w.enabled = 1 AND d.next_attempt_at <= ?1
            ORDER BY d.next_attempt_at ASC
            LIMIT ?2
            "#,
        )
        .bind(&now)
        .bind(limit)
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }

    pub async fn mark_webhook_delivery_succeeded(
        &self,
        delivery_id: &str,
        status_code: u16,
    ) -> Result<(), DbError> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            UPDATE webhook_deliveries
            SET status = 'delivered',
                attempts = attempts + 1,
                last_status_code = ?2,
                last_error = NULL,
                delivered_at = ?3,
                updated_at = ?3
            WHERE id = ?1
            "#,
        )
        .bind(delivery_id)
        .bind(i64::from(status_code))
        .bind(&now)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn mark_webhook_delivery_failed(
        &self,
        delivery_id: &str,
        next_attempt_at: Option<DateTime<Utc>>,
        status_code: Option<u16>,
        error: &str,
    ) -> Result<(), DbError> {
        let now = Utc::now().to_rfc3339();
        let (status, next_str) = match next_attempt_at {
            Some(dt) => ("pending", dt.to_rfc3339()),
            None => ("dead_letter", now.clone()),
        };
        sqlx::query(
            r#"
            UPDATE webhook_deliveries
            SET status = ?2,
                attempts = attempts + 1,
                next_attempt_at = ?3,
                last_status_code = ?4,
                last_error = ?5,
                updated_at = ?6
            WHERE id = ?1
            "#,
        )
        .bind(delivery_id)
        .bind(status)
        .bind(&next_str)
        .bind(status_code.map(i64::from))
        .bind(error)
        .bind(&now)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn count_webhook_deliveries_by_status(&self, status: &str) -> Result<i64, DbError> {
        let (count,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM webhook_deliveries WHERE status = ?1")
                .bind(status)
                .fetch_one(self.pool())
                .await?;
        Ok(count)
    }
}
