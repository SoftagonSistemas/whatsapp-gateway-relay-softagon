use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use super::{Database, DbError};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ContactRecord {
    pub session_id: String,
    pub jid: String,
    pub lid: Option<String>,
    pub phone_number: Option<String>,
    pub full_name: Option<String>,
    pub first_name: Option<String>,
    pub push_name: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct UpsertContact<'a> {
    pub session_id: &'a str,
    pub jid: &'a str,
    pub lid: Option<&'a str>,
    pub phone_number: Option<&'a str>,
    pub full_name: Option<&'a str>,
    pub first_name: Option<&'a str>,
    pub push_name: Option<&'a str>,
}

impl Database {
    pub async fn upsert_contact(&self, input: UpsertContact<'_>) -> Result<ContactRecord, DbError> {
        let now = Utc::now().to_rfc3339();
        let clean_lid = input.lid.map(str::trim).filter(|s| !s.is_empty());
        let clean_phone = input.phone_number.map(str::trim).filter(|s| !s.is_empty());
        let clean_full = input.full_name.map(str::trim).filter(|s| !s.is_empty());
        let clean_first = input.first_name.map(str::trim).filter(|s| !s.is_empty());
        let clean_push = input.push_name.map(str::trim).filter(|s| !s.is_empty());

        sqlx::query(
            r#"
            INSERT INTO contacts (
                session_id, jid, lid, phone_number, full_name, first_name, push_name, updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT(session_id, jid) DO UPDATE SET
                lid = COALESCE(excluded.lid, contacts.lid),
                phone_number = COALESCE(excluded.phone_number, contacts.phone_number),
                full_name = COALESCE(excluded.full_name, contacts.full_name),
                first_name = COALESCE(excluded.first_name, contacts.first_name),
                push_name = COALESCE(excluded.push_name, contacts.push_name),
                updated_at = excluded.updated_at
            "#,
        )
        .bind(input.session_id)
        .bind(input.jid)
        .bind(clean_lid)
        .bind(clean_phone)
        .bind(clean_full)
        .bind(clean_first)
        .bind(clean_push)
        .bind(&now)
        .execute(self.pool())
        .await?;

        let rec = sqlx::query_as::<_, ContactRecord>(
            r#"
            SELECT session_id, jid, lid, phone_number, full_name, first_name, push_name, updated_at
            FROM contacts
            WHERE session_id = ?1 AND jid = ?2
            "#,
        )
        .bind(input.session_id)
        .bind(input.jid)
        .fetch_one(self.pool())
        .await?;
        Ok(rec)
    }

    pub async fn get_contact_by_any(
        &self,
        session_id: &str,
        query: &str,
    ) -> Result<Option<ContactRecord>, DbError> {
        let trimmed = query.trim();
        let digits: String = trimmed.chars().filter(|c| c.is_ascii_digit()).collect();
        let phone_jid = if !digits.is_empty() {
            format!("{digits}@s.whatsapp.net")
        } else {
            String::new()
        };
        let row = sqlx::query_as::<_, ContactRecord>(
            r#"
            SELECT session_id, jid, lid, phone_number, full_name, first_name, push_name, updated_at
            FROM contacts
            WHERE session_id = ?1
              AND (
                  jid = ?2
                  OR lid = ?2
                  OR (?3 != '' AND (phone_number = ?3 OR jid = ?4))
              )
            ORDER BY updated_at DESC
            LIMIT 1
            "#,
        )
        .bind(session_id)
        .bind(trimmed)
        .bind(&digits)
        .bind(&phone_jid)
        .fetch_optional(self.pool())
        .await?;
        Ok(row)
    }

    pub async fn list_contacts(
        &self,
        session_id: &str,
        query: Option<&str>,
        limit: i64,
    ) -> Result<Vec<ContactRecord>, DbError> {
        let clamped = limit.clamp(1, 500);
        let q = query.map(str::trim).filter(|s| !s.is_empty());
        let rows = if let Some(term) = q {
            let pat = format!("%{}%", term.to_lowercase());
            sqlx::query_as::<_, ContactRecord>(
                r#"
                SELECT session_id, jid, lid, phone_number, full_name, first_name, push_name, updated_at
                FROM contacts
                WHERE session_id = ?1
                  AND (
                      LOWER(COALESCE(full_name, '')) LIKE ?2
                      OR LOWER(COALESCE(first_name, '')) LIKE ?2
                      OR LOWER(COALESCE(push_name, '')) LIKE ?2
                      OR LOWER(COALESCE(phone_number, '')) LIKE ?2
                      OR LOWER(jid) LIKE ?2
                      OR LOWER(COALESCE(lid, '')) LIKE ?2
                  )
                ORDER BY LOWER(COALESCE(full_name, push_name, jid)) ASC
                LIMIT ?3
                "#,
            )
            .bind(session_id)
            .bind(&pat)
            .bind(clamped)
            .fetch_all(self.pool())
            .await?
        } else {
            sqlx::query_as::<_, ContactRecord>(
                r#"
                SELECT session_id, jid, lid, phone_number, full_name, first_name, push_name, updated_at
                FROM contacts
                WHERE session_id = ?1
                ORDER BY LOWER(COALESCE(full_name, push_name, jid)) ASC
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
}
