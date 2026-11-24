use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use super::{Database, DbError};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MessageRecord {
    pub id: String,
    pub session_id: String,
    pub external_id: Option<String>,
    pub direction: String,
    pub chat_jid: String,
    pub sender_jid: Option<String>,
    pub message_type: String,
    pub text_body: Option<String>,
    pub caption: Option<String>,
    pub media_id: Option<String>,
    pub status: String,
    pub metadata_json: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct NewMessage<'a> {
    pub id: &'a str,
    pub session_id: &'a str,
    pub external_id: Option<&'a str>,
    pub direction: &'a str,
    pub chat_jid: &'a str,
    pub sender_jid: Option<&'a str>,
    pub message_type: &'a str,
    pub text_body: Option<&'a str>,
    pub caption: Option<&'a str>,
    pub media_id: Option<&'a str>,
    pub status: &'a str,
    pub metadata_json: &'a str,
}

impl Database {
    pub async fn insert_message(&self, msg: NewMessage<'_>) -> Result<MessageRecord, DbError> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            INSERT INTO messages (
                id, session_id, external_id, direction, chat_jid, sender_jid,
                message_type, text_body, caption, media_id, status, metadata_json,
                created_at, updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
            "#,
        )
        .bind(msg.id)
        .bind(msg.session_id)
        .bind(msg.external_id)
        .bind(msg.direction)
        .bind(msg.chat_jid)
        .bind(msg.sender_jid)
        .bind(msg.message_type)
        .bind(msg.text_body)
        .bind(msg.caption)
        .bind(msg.media_id)
        .bind(msg.status)
        .bind(msg.metadata_json)
        .bind(&now)
        .bind(&now)
        .execute(self.pool())
        .await?;

        self.get_message(msg.id)
            .await?
            .ok_or_else(|| DbError::Sqlx(sqlx::Error::RowNotFound))
    }

    pub async fn get_message(&self, id: &str) -> Result<Option<MessageRecord>, DbError> {
        let row = sqlx::query_as::<_, MessageRecord>(
            r#"
            SELECT id, session_id, external_id, direction, chat_jid, sender_jid,
                   message_type, text_body, caption, media_id, status, metadata_json,
                   created_at, updated_at
            FROM messages
            WHERE id = ?1
            "#,
        )
        .bind(id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row)
    }

    pub async fn update_message_status_by_external_id(
        &self,
        session_id: &str,
        external_id: &str,
        status: &str,
    ) -> Result<Option<String>, DbError> {
        let now = Utc::now().to_rfc3339();
        let found: Option<(String,)> = sqlx::query_as(
            "SELECT id FROM messages WHERE session_id = ?1 AND external_id = ?2 LIMIT 1",
        )
        .bind(session_id)
        .bind(external_id)
        .fetch_optional(self.pool())
        .await?;

        if let Some((id,)) = found {
            sqlx::query("UPDATE messages SET status = ?2, updated_at = ?3 WHERE id = ?1")
                .bind(&id)
                .bind(status)
                .bind(&now)
                .execute(self.pool())
                .await?;
            Ok(Some(id))
        } else {
            Ok(None)
        }
    }
}
