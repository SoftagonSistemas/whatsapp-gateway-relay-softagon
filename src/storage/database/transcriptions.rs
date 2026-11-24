use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use super::{Database, DbError};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct TranscriptionRecord {
    pub message_id: String,
    pub session_id: String,
    pub status: String,
    pub text: Option<String>,
    pub language: Option<String>,
    pub engine: String,
    pub model: String,
    pub duration_ms: Option<i64>,
    pub processing_ms: Option<i64>,
    pub segments_json: String,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct TranscriptionJobRecord {
    pub id: String,
    pub message_id: String,
    pub session_id: String,
    pub media_id: String,
    pub language_override: Option<String>,
    pub status: String,
    pub attempts: i64,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct UpsertTranscription<'a> {
    pub message_id: &'a str,
    pub session_id: &'a str,
    pub status: &'a str,
    pub text: Option<&'a str>,
    pub language: Option<&'a str>,
    pub engine: &'a str,
    pub model: &'a str,
    pub duration_ms: Option<i64>,
    pub processing_ms: Option<i64>,
    pub segments_json: &'a str,
    pub error: Option<&'a str>,
}

impl Database {
    pub async fn upsert_transcription(
        &self,
        rec: UpsertTranscription<'_>,
    ) -> Result<TranscriptionRecord, DbError> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            INSERT INTO transcriptions (
                message_id, session_id, status, text, language, engine, model,
                duration_ms, processing_ms, segments_json, error, created_at, updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)
            ON CONFLICT(message_id) DO UPDATE SET
                status = excluded.status,
                text = excluded.text,
                language = excluded.language,
                engine = excluded.engine,
                model = excluded.model,
                duration_ms = excluded.duration_ms,
                processing_ms = excluded.processing_ms,
                segments_json = excluded.segments_json,
                error = excluded.error,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(rec.message_id)
        .bind(rec.session_id)
        .bind(rec.status)
        .bind(rec.text)
        .bind(rec.language)
        .bind(rec.engine)
        .bind(rec.model)
        .bind(rec.duration_ms)
        .bind(rec.processing_ms)
        .bind(rec.segments_json)
        .bind(rec.error)
        .bind(&now)
        .execute(self.pool())
        .await?;

        self.get_transcription(rec.message_id)
            .await?
            .ok_or_else(|| DbError::Sqlx(sqlx::Error::RowNotFound))
    }

    pub async fn get_transcription(
        &self,
        message_id: &str,
    ) -> Result<Option<TranscriptionRecord>, DbError> {
        let row = sqlx::query_as::<_, TranscriptionRecord>(
            r#"
            SELECT message_id, session_id, status, text, language, engine, model,
                   duration_ms, processing_ms, segments_json, error, created_at, updated_at
            FROM transcriptions
            WHERE message_id = ?1
            "#,
        )
        .bind(message_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row)
    }

    pub async fn count_pending_transcription_jobs(&self) -> Result<i64, DbError> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM transcription_jobs WHERE status IN ('queued', 'processing')",
        )
        .fetch_one(self.pool())
        .await?;
        Ok(count)
    }

    pub async fn enqueue_transcription_job(
        &self,
        id: &str,
        message_id: &str,
        session_id: &str,
        media_id: &str,
        language_override: Option<&str>,
    ) -> Result<TranscriptionJobRecord, DbError> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            INSERT INTO transcription_jobs (
                id, message_id, session_id, media_id, language_override,
                status, attempts, created_at, updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, 'queued', 0, ?6, ?6)
            "#,
        )
        .bind(id)
        .bind(message_id)
        .bind(session_id)
        .bind(media_id)
        .bind(language_override)
        .bind(&now)
        .execute(self.pool())
        .await?;

        let row = sqlx::query_as::<_, TranscriptionJobRecord>(
            r#"
            SELECT id, message_id, session_id, media_id, language_override,
                   status, attempts, last_error, created_at, updated_at
            FROM transcription_jobs WHERE id = ?1
            "#,
        )
        .bind(id)
        .fetch_one(self.pool())
        .await?;
        Ok(row)
    }

    pub async fn recover_stale_transcription_jobs(&self) -> Result<u64, DbError> {
        let now = Utc::now().to_rfc3339();
        let res = sqlx::query(
            "UPDATE transcription_jobs SET status = 'queued', updated_at = ?1 WHERE status = 'processing'",
        )
        .bind(&now)
        .execute(self.pool())
        .await?;
        Ok(res.rows_affected())
    }

    pub async fn claim_next_transcription_job(
        &self,
    ) -> Result<Option<TranscriptionJobRecord>, DbError> {
        let mut tx = self.pool().begin().await?;
        let candidate = sqlx::query_as::<_, TranscriptionJobRecord>(
            r#"
            SELECT id, message_id, session_id, media_id, language_override,
                   status, attempts, last_error, created_at, updated_at
            FROM transcription_jobs
            WHERE status = 'queued'
            ORDER BY created_at ASC
            LIMIT 1
            "#,
        )
        .fetch_optional(&mut *tx)
        .await?;

        let Some(mut job) = candidate else {
            tx.commit().await?;
            return Ok(None);
        };

        let now = Utc::now().to_rfc3339();
        let res = sqlx::query(
            r#"
            UPDATE transcription_jobs
            SET status = 'processing', attempts = attempts + 1, updated_at = ?2
            WHERE id = ?1 AND status = 'queued'
            "#,
        )
        .bind(&job.id)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        if res.rows_affected() == 1 {
            job.status = "processing".to_string();
            job.attempts += 1;
            job.updated_at = now;
            Ok(Some(job))
        } else {
            Ok(None)
        }
    }

    pub async fn finish_transcription_job(
        &self,
        job_id: &str,
        status: &str,
        last_error: Option<&str>,
    ) -> Result<(), DbError> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE transcription_jobs SET status = ?2, last_error = ?3, updated_at = ?4 WHERE id = ?1",
        )
        .bind(job_id)
        .bind(status)
        .bind(last_error)
        .bind(&now)
        .execute(self.pool())
        .await?;
        Ok(())
    }
}
