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
    pub created_at: Option<&'a str>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ChatSummaryRecord {
    pub chat_jid: String,
    pub contact_name: Option<String>,
    pub last_message_id: String,
    pub last_direction: String,
    pub last_message_type: String,
    pub last_text: Option<String>,
    pub last_sender_jid: Option<String>,
    pub last_message_at: String,
    pub message_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MessageWithTranscriptionRecord {
    pub id: String,
    pub session_id: String,
    pub direction: String,
    pub chat_jid: String,
    pub sender_jid: Option<String>,
    pub sender_name: Option<String>,
    pub message_type: String,
    pub text_body: Option<String>,
    pub caption: Option<String>,
    pub transcription_text: Option<String>,
    pub status: String,
    pub metadata_json: String,
    pub created_at: String,
}

impl Database {
    pub async fn insert_message(&self, msg: NewMessage<'_>) -> Result<MessageRecord, DbError> {
        let now = Utc::now().to_rfc3339();
        let created_at = msg.created_at.unwrap_or(&now);
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
        .bind(created_at)
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

    pub async fn has_message_by_external_id(
        &self,
        session_id: &str,
        external_id: &str,
    ) -> Result<bool, DbError> {
        let found: Option<(i64,)> = sqlx::query_as(
            "SELECT 1 FROM messages WHERE session_id = ?1 AND external_id = ?2 LIMIT 1",
        )
        .bind(session_id)
        .bind(external_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(found.is_some())
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

    pub async fn list_chats(
        &self,
        session_id: &str,
        query: Option<&str>,
        limit: i64,
    ) -> Result<Vec<ChatSummaryRecord>, DbError> {
        let clamped = limit.clamp(1, 200);
        let pattern = query
            .map(str::trim)
            .filter(|q| !q.is_empty())
            .map(|q| format!("%{}%", q.to_lowercase()));

        let rows = if let Some(pat) = pattern {
            sqlx::query_as::<_, ChatSummaryRecord>(
                r#"
                SELECT
                    m.chat_jid,
                    COALESCE(
                        (
                            SELECT COALESCE(c.full_name, c.push_name)
                            FROM contacts c
                            WHERE c.session_id = m.session_id
                              AND (c.jid = m.chat_jid OR c.lid = m.chat_jid)
                            LIMIT 1
                        ),
                        (
                            SELECT json_extract(cn.metadata_json, '$.sender_name')
                            FROM messages cn
                            WHERE cn.session_id = m.session_id
                              AND cn.chat_jid = m.chat_jid
                              AND cn.direction = 'inbound'
                              AND json_extract(cn.metadata_json, '$.sender_name') IS NOT NULL
                            ORDER BY cn.created_at DESC
                            LIMIT 1
                        )
                    ) AS contact_name,
                    m.id AS last_message_id,
                    m.direction AS last_direction,
                    m.message_type AS last_message_type,
                    COALESCE(m.text_body, m.caption) AS last_text,
                    m.sender_jid AS last_sender_jid,
                    m.created_at AS last_message_at,
                    agg.message_count AS message_count
                FROM messages m
                INNER JOIN (
                    SELECT chat_jid, MAX(created_at) AS max_created, COUNT(*) AS message_count
                    FROM messages
                    WHERE session_id = ?1
                      AND chat_jid != 'status@broadcast'
                      AND chat_jid NOT LIKE '%@newsletter'
                    GROUP BY chat_jid
                ) agg ON agg.chat_jid = m.chat_jid AND agg.max_created = m.created_at
                WHERE m.session_id = ?1
                  AND m.chat_jid != 'status@broadcast'
                  AND m.chat_jid NOT LIKE '%@newsletter'
                  AND (
                      EXISTS (
                          SELECT 1 FROM messages sm
                          LEFT JOIN transcriptions st ON st.message_id = sm.id
                          WHERE sm.session_id = ?1
                            AND sm.chat_jid = m.chat_jid
                            AND (
                                LOWER(sm.chat_jid) LIKE ?2
                                OR LOWER(COALESCE(sm.sender_jid, '')) LIKE ?2
                                OR LOWER(COALESCE(sm.text_body, '')) LIKE ?2
                                OR LOWER(COALESCE(sm.caption, '')) LIKE ?2
                                OR LOWER(COALESCE(sm.metadata_json, '')) LIKE ?2
                                OR LOWER(COALESCE(st.text, '')) LIKE ?2
                            )
                      )
                      OR EXISTS (
                          SELECT 1 FROM contacts sc
                          WHERE sc.session_id = ?1
                            AND (sc.jid = m.chat_jid OR sc.lid = m.chat_jid)
                            AND (
                                LOWER(COALESCE(sc.full_name, '')) LIKE ?2
                                OR LOWER(COALESCE(sc.first_name, '')) LIKE ?2
                                OR LOWER(COALESCE(sc.push_name, '')) LIKE ?2
                                OR LOWER(COALESCE(sc.phone_number, '')) LIKE ?2
                            )
                      )
                  )
                GROUP BY m.chat_jid
                ORDER BY m.created_at DESC
                LIMIT ?3
                "#,
            )
            .bind(session_id)
            .bind(&pat)
            .bind(clamped)
            .fetch_all(self.pool())
            .await?
        } else {
            sqlx::query_as::<_, ChatSummaryRecord>(
                r#"
                SELECT
                    m.chat_jid,
                    COALESCE(
                        (
                            SELECT COALESCE(c.full_name, c.push_name)
                            FROM contacts c
                            WHERE c.session_id = m.session_id
                              AND (c.jid = m.chat_jid OR c.lid = m.chat_jid)
                            LIMIT 1
                        ),
                        (
                            SELECT json_extract(cn.metadata_json, '$.sender_name')
                            FROM messages cn
                            WHERE cn.session_id = m.session_id
                              AND cn.chat_jid = m.chat_jid
                              AND cn.direction = 'inbound'
                              AND json_extract(cn.metadata_json, '$.sender_name') IS NOT NULL
                            ORDER BY cn.created_at DESC
                            LIMIT 1
                        )
                    ) AS contact_name,
                    m.id AS last_message_id,
                    m.direction AS last_direction,
                    m.message_type AS last_message_type,
                    COALESCE(m.text_body, m.caption) AS last_text,
                    m.sender_jid AS last_sender_jid,
                    m.created_at AS last_message_at,
                    agg.message_count AS message_count
                FROM messages m
                INNER JOIN (
                    SELECT chat_jid, MAX(created_at) AS max_created, COUNT(*) AS message_count
                    FROM messages
                    WHERE session_id = ?1
                      AND chat_jid != 'status@broadcast'
                      AND chat_jid NOT LIKE '%@newsletter'
                    GROUP BY chat_jid
                ) agg ON agg.chat_jid = m.chat_jid AND agg.max_created = m.created_at
                WHERE m.session_id = ?1
                  AND m.chat_jid != 'status@broadcast'
                  AND m.chat_jid NOT LIKE '%@newsletter'
                GROUP BY m.chat_jid
                ORDER BY m.created_at DESC
                LIMIT ?2
                "#,
            )
            .bind(session_id)
            .bind(clamped)
            .fetch_all(self.pool())
            .await?
        };
        Ok(rows)
    }

    /// Look up a human-readable summary of a stored message by its external WhatsApp stanza ID,
    /// used as a SQLite fallback when WhatsApp omits `quoted_message` content on replies.
    pub async fn find_message_summary_by_external_id(
        &self,
        session_id: &str,
        external_id: &str,
    ) -> Result<Option<String>, DbError> {
        let row: Option<(String,)> = sqlx::query_as(
            r#"
            SELECT COALESCE(
                NULLIF(TRIM(m.text_body), ''),
                NULLIF(TRIM(m.caption), ''),
                NULLIF(TRIM(t.text), ''),
                '[' || m.message_type || ']'
            )
            FROM messages m
            LEFT JOIN transcriptions t ON t.message_id = m.id
            WHERE m.session_id = ?1 AND m.external_id = ?2
            LIMIT 1
            "#,
        )
        .bind(session_id)
        .bind(external_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|(s,)| s))
    }

    /// Return all distinct legacy `@lid` chat_jids in a session alongside any known `sender_phone`
    /// already captured in `metadata_json`.
    pub async fn list_legacy_lid_chats(
        &self,
        session_id: &str,
    ) -> Result<Vec<(String, Option<String>)>, DbError> {
        let rows: Vec<(String, Option<String>)> = sqlx::query_as(
            r#"
            SELECT
                chat_jid,
                MAX(NULLIF(TRIM(json_extract(metadata_json, '$.sender_phone')), '')) AS phone
            FROM messages
            WHERE session_id = ?1 AND chat_jid LIKE '%@lid'
            GROUP BY chat_jid
            "#,
        )
        .bind(session_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }

    /// Rewrite legacy `@lid` rows to their canonical `{phone}@s.whatsapp.net` JID while preserving
    /// the original `@lid` in `metadata_json.chat_lid`.
    pub async fn rewrite_chat_jid_from_lid(
        &self,
        session_id: &str,
        old_lid: &str,
        canonical_jid: &str,
    ) -> Result<u64, DbError> {
        let res = sqlx::query(
            r#"
            UPDATE messages
            SET chat_jid = ?3,
                metadata_json = json_set(COALESCE(NULLIF(TRIM(metadata_json), ''), '{}'), '$.chat_lid', ?2)
            WHERE session_id = ?1 AND chat_jid = ?2
            "#,
        )
        .bind(session_id)
        .bind(old_lid)
        .bind(canonical_jid)
        .execute(self.pool())
        .await?;
        Ok(res.rows_affected())
    }

    pub async fn get_chat_messages(
        &self,
        session_id: &str,
        chat_jid: &str,
        limit: i64,
    ) -> Result<Vec<MessageWithTranscriptionRecord>, DbError> {
        self.get_chat_messages_with_alias(session_id, chat_jid, None, limit)
            .await
    }

    pub async fn get_chat_messages_with_alias(
        &self,
        session_id: &str,
        chat_jid: &str,
        alt_chat_jid: Option<&str>,
        limit: i64,
    ) -> Result<Vec<MessageWithTranscriptionRecord>, DbError> {
        let clamped = limit.clamp(1, 200);
        let alt = alt_chat_jid.unwrap_or(chat_jid);
        let rows = sqlx::query_as::<_, MessageWithTranscriptionRecord>(
            r#"
            SELECT
                m.id, m.session_id, m.direction, m.chat_jid, m.sender_jid,
                json_extract(m.metadata_json, '$.sender_name') AS sender_name,
                m.message_type, m.text_body, m.caption,
                t.text AS transcription_text,
                m.status, m.metadata_json, m.created_at
            FROM messages m
            LEFT JOIN transcriptions t ON t.message_id = m.id
            WHERE m.session_id = ?1
              AND (
                  m.chat_jid = ?2
                  OR m.chat_jid = ?3
                  OR json_extract(m.metadata_json, '$.chat_lid') = ?2
                  OR json_extract(m.metadata_json, '$.chat_lid') = ?3
              )
            ORDER BY m.created_at DESC
            LIMIT ?4
            "#,
        )
        .bind(session_id)
        .bind(chat_jid)
        .bind(alt)
        .bind(clamped)
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }

    pub async fn search_messages(
        &self,
        session_id: &str,
        query: &str,
        limit: i64,
    ) -> Result<Vec<MessageWithTranscriptionRecord>, DbError> {
        let clamped = limit.clamp(1, 200);
        let pat = format!("%{}%", query.trim().to_lowercase());
        let rows = sqlx::query_as::<_, MessageWithTranscriptionRecord>(
            r#"
            SELECT
                m.id, m.session_id, m.direction, m.chat_jid, m.sender_jid,
                json_extract(m.metadata_json, '$.sender_name') AS sender_name,
                m.message_type, m.text_body, m.caption,
                t.text AS transcription_text,
                m.status, m.metadata_json, m.created_at
            FROM messages m
            LEFT JOIN transcriptions t ON t.message_id = m.id
            WHERE m.session_id = ?1
              AND (
                  LOWER(m.chat_jid) LIKE ?2
                  OR LOWER(COALESCE(m.sender_jid, '')) LIKE ?2
                  OR LOWER(COALESCE(m.text_body, '')) LIKE ?2
                  OR LOWER(COALESCE(m.caption, '')) LIKE ?2
                  OR LOWER(COALESCE(m.metadata_json, '')) LIKE ?2
                  OR LOWER(COALESCE(t.text, '')) LIKE ?2
              )
            ORDER BY m.created_at DESC
            LIMIT ?3
            "#,
        )
        .bind(session_id)
        .bind(&pat)
        .bind(clamped)
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }

    pub async fn list_recent_inbound_external_ids(
        &self,
        session_id: &str,
        chat_jid: &str,
        limit: i64,
    ) -> Result<Vec<String>, DbError> {
        let clamped = limit.clamp(1, 100);
        let rows: Vec<(String,)> = sqlx::query_as(
            r#"
            SELECT external_id
            FROM messages
            WHERE session_id = ?1
              AND chat_jid = ?2
              AND direction = 'inbound'
              AND external_id IS NOT NULL
              AND external_id != ''
            ORDER BY created_at DESC
            LIMIT ?3
            "#,
        )
        .bind(session_id)
        .bind(chat_jid)
        .bind(clamped)
        .fetch_all(self.pool())
        .await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    pub async fn lookup_contact_from_history(
        &self,
        session_id: &str,
        jid_or_phone: &str,
    ) -> Result<Option<(String, Option<String>, Option<String>)>, DbError> {
        let pat = format!("%{}%", jid_or_phone.trim().to_lowercase());
        let row: Option<(String, Option<String>, Option<String>)> = sqlx::query_as(
            r#"
            SELECT
                chat_jid,
                json_extract(metadata_json, '$.sender_name') AS sender_name,
                json_extract(metadata_json, '$.sender_phone') AS sender_phone
            FROM messages
            WHERE session_id = ?1
              AND direction = 'inbound'
              AND (
                  LOWER(chat_jid) = LOWER(?2)
                  OR LOWER(COALESCE(sender_jid, '')) = LOWER(?2)
                  OR LOWER(COALESCE(json_extract(metadata_json, '$.sender_phone'), '')) = LOWER(?2)
                  OR LOWER(chat_jid) LIKE ?3
                  OR LOWER(COALESCE(sender_jid, '')) LIKE ?3
              )
            ORDER BY
                (json_extract(metadata_json, '$.sender_name') IS NOT NULL) DESC,
                (json_extract(metadata_json, '$.sender_phone') IS NOT NULL) DESC,
                created_at DESC
            LIMIT 1
            "#,
        )
        .bind(session_id)
        .bind(jid_or_phone.trim())
        .bind(&pat)
        .fetch_optional(self.pool())
        .await?;
        Ok(row)
    }

    pub async fn update_message_text_by_external_id(
        &self,
        session_id: &str,
        external_id: &str,
        new_text: &str,
    ) -> Result<Option<String>, DbError> {
        let now = Utc::now().to_rfc3339();
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT id FROM messages WHERE session_id = ?1 AND external_id = ?2 LIMIT 1",
        )
        .bind(session_id)
        .bind(external_id)
        .fetch_optional(self.pool())
        .await?;
        let Some((id,)) = row else {
            return Ok(None);
        };
        sqlx::query("UPDATE messages SET text_body = ?1, updated_at = ?2 WHERE id = ?3")
            .bind(new_text)
            .bind(&now)
            .bind(&id)
            .execute(self.pool())
            .await?;
        Ok(Some(id))
    }

    pub async fn mark_message_revoked_by_external_id(
        &self,
        session_id: &str,
        external_id: &str,
    ) -> Result<Option<String>, DbError> {
        let now = Utc::now().to_rfc3339();
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT id FROM messages WHERE session_id = ?1 AND external_id = ?2 LIMIT 1",
        )
        .bind(session_id)
        .bind(external_id)
        .fetch_optional(self.pool())
        .await?;
        let Some((id,)) = row else {
            return Ok(None);
        };
        sqlx::query("UPDATE messages SET status = 'revoked', updated_at = ?1 WHERE id = ?2")
            .bind(&now)
            .bind(&id)
            .execute(self.pool())
            .await?;
        Ok(Some(id))
    }

    pub async fn count_messages_total(&self) -> Result<i64, DbError> {
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM messages")
            .fetch_one(self.pool())
            .await?;
        Ok(count)
    }
}
