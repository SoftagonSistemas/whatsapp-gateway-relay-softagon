use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use super::{Database, DbError};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MediaObjectRecord {
    pub id: String,
    pub session_id: String,
    pub storage_backend: String,
    pub object_key: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub sha256: String,
    pub filename: Option<String>,
    pub retention_state: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct NewMediaObject<'a> {
    pub id: &'a str,
    pub session_id: &'a str,
    pub storage_backend: &'a str,
    pub object_key: &'a str,
    pub mime_type: &'a str,
    pub size_bytes: i64,
    pub sha256: &'a str,
    pub filename: Option<&'a str>,
}

impl Database {
    pub async fn insert_media_object(
        &self,
        media: NewMediaObject<'_>,
    ) -> Result<MediaObjectRecord, DbError> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            INSERT INTO media_objects (
                id, session_id, storage_backend, object_key, mime_type,
                size_bytes, sha256, filename, retention_state, created_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'stored', ?9)
            "#,
        )
        .bind(media.id)
        .bind(media.session_id)
        .bind(media.storage_backend)
        .bind(media.object_key)
        .bind(media.mime_type)
        .bind(media.size_bytes)
        .bind(media.sha256)
        .bind(media.filename)
        .bind(&now)
        .execute(self.pool())
        .await?;

        self.get_media_object(media.id)
            .await?
            .ok_or_else(|| DbError::Sqlx(sqlx::Error::RowNotFound))
    }

    pub async fn get_media_object(&self, id: &str) -> Result<Option<MediaObjectRecord>, DbError> {
        let row = sqlx::query_as::<_, MediaObjectRecord>(
            r#"
            SELECT id, session_id, storage_backend, object_key, mime_type,
                   size_bytes, sha256, filename, retention_state, created_at
            FROM media_objects
            WHERE id = ?1
            "#,
        )
        .bind(id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row)
    }

    pub async fn list_expired_media_objects(
        &self,
        cutoff_rfc3339: &str,
        limit: i64,
    ) -> Result<Vec<MediaObjectRecord>, DbError> {
        let rows = sqlx::query_as::<_, MediaObjectRecord>(
            r#"
            SELECT id, session_id, storage_backend, object_key, mime_type,
                   size_bytes, sha256, filename, retention_state, created_at
            FROM media_objects
            WHERE retention_state = 'stored' AND created_at < ?1
            ORDER BY created_at ASC
            LIMIT ?2
            "#,
        )
        .bind(cutoff_rfc3339)
        .bind(limit.clamp(1, 500))
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }

    pub async fn mark_media_purged(&self, id: &str) -> Result<(), DbError> {
        sqlx::query("UPDATE media_objects SET retention_state = 'purged' WHERE id = ?1")
            .bind(id)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    pub async fn media_storage_stats(&self) -> Result<(i64, i64), DbError> {
        let row: (i64, i64) = sqlx::query_as(
            "SELECT COUNT(*), COALESCE(SUM(size_bytes), 0) FROM media_objects WHERE retention_state = 'stored'",
        )
        .fetch_one(self.pool())
        .await?;
        Ok(row)
    }
}
