use chrono::Utc;
use sqlx::FromRow;

use super::{Database, DbError};

#[derive(Debug, Clone, FromRow)]
pub struct IdempotencyRecord {
    pub key: String,
    pub session_id: String,
    pub request_hash: String,
    pub response_status: i64,
    pub response_body: String,
    pub created_at: String,
}

impl Database {
    pub async fn get_idempotency_key(
        &self,
        key: &str,
        session_id: &str,
    ) -> Result<Option<IdempotencyRecord>, DbError> {
        let row = sqlx::query_as::<_, IdempotencyRecord>(
            r#"
            SELECT key, session_id, request_hash, response_status, response_body, created_at
            FROM idempotency_keys
            WHERE key = ?1 AND session_id = ?2
            "#,
        )
        .bind(key)
        .bind(session_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row)
    }

    pub async fn save_idempotency_key(
        &self,
        key: &str,
        session_id: &str,
        request_hash: &str,
        response_status: u16,
        response_body: &str,
    ) -> Result<(), DbError> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO idempotency_keys (
                key, session_id, request_hash, response_status, response_body, created_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
        )
        .bind(key)
        .bind(session_id)
        .bind(request_hash)
        .bind(i64::from(response_status))
        .bind(response_body)
        .bind(&now)
        .execute(self.pool())
        .await?;
        Ok(())
    }
}
