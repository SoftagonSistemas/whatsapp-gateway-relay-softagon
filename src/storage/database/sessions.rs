use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use super::{Database, DbError};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SessionRecord {
    pub id: String,
    pub name: String,
    pub status: String,
    pub phone_number: Option<String>,
    pub jid: Option<String>,
    pub transcription_language: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl Database {
    pub async fn create_session(
        &self,
        id: &str,
        name: &str,
        status: &str,
        transcription_language: Option<&str>,
    ) -> Result<SessionRecord, DbError> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            INSERT INTO sessions (id, name, status, transcription_language, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
        )
        .bind(id)
        .bind(name)
        .bind(status)
        .bind(transcription_language)
        .bind(&now)
        .bind(&now)
        .execute(self.pool())
        .await?;

        self.get_session(id)
            .await?
            .ok_or_else(|| DbError::Sqlx(sqlx::Error::RowNotFound))
    }

    pub async fn get_session(&self, id: &str) -> Result<Option<SessionRecord>, DbError> {
        let row = sqlx::query_as::<_, SessionRecord>(
            "SELECT id, name, status, phone_number, jid, transcription_language, created_at, updated_at FROM sessions WHERE id = ?1",
        )
        .bind(id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row)
    }

    pub async fn list_sessions(&self) -> Result<Vec<SessionRecord>, DbError> {
        let rows = sqlx::query_as::<_, SessionRecord>(
            "SELECT id, name, status, phone_number, jid, transcription_language, created_at, updated_at FROM sessions ORDER BY created_at DESC",
        )
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }

    pub async fn update_session_status(
        &self,
        id: &str,
        status: &str,
        phone_number: Option<&str>,
        jid: Option<&str>,
    ) -> Result<(), DbError> {
        let now: DateTime<Utc> = Utc::now();
        sqlx::query(
            r#"
            UPDATE sessions
            SET status = ?2,
                phone_number = COALESCE(?3, phone_number),
                jid = COALESCE(?4, jid),
                updated_at = ?5
            WHERE id = ?1
            "#,
        )
        .bind(id)
        .bind(status)
        .bind(phone_number)
        .bind(jid)
        .bind(now.to_rfc3339())
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn list_restorable_sessions(&self) -> Result<Vec<SessionRecord>, DbError> {
        let rows = sqlx::query_as::<_, SessionRecord>(
            "SELECT id, name, status, phone_number, jid, transcription_language, created_at, updated_at FROM sessions WHERE status != 'logged_out'",
        )
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }

    pub async fn delete_session(&self, id: &str) -> Result<bool, DbError> {
        let res = sqlx::query("DELETE FROM sessions WHERE id = ?1")
            .bind(id)
            .execute(self.pool())
            .await?;
        Ok(res.rows_affected() > 0)
    }
}
