use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use super::{Database, DbError};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct McpAuditRecord {
    pub id: String,
    pub transport: String,
    pub tool_name: String,
    pub session_id: Option<String>,
    pub arguments_json: String,
    pub status: String,
    pub error_message: Option<String>,
    pub duration_ms: i64,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct NewMcpAudit<'a> {
    pub id: &'a str,
    pub transport: &'a str,
    pub tool_name: &'a str,
    pub session_id: Option<&'a str>,
    pub arguments_json: &'a str,
    pub status: &'a str,
    pub error_message: Option<&'a str>,
    pub duration_ms: i64,
}

impl Database {
    pub async fn insert_mcp_audit(&self, entry: NewMcpAudit<'_>) -> Result<(), DbError> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            INSERT INTO mcp_audit_log (
                id, transport, tool_name, session_id, arguments_json,
                status, error_message, duration_ms, created_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            "#,
        )
        .bind(entry.id)
        .bind(entry.transport)
        .bind(entry.tool_name)
        .bind(entry.session_id)
        .bind(entry.arguments_json)
        .bind(entry.status)
        .bind(entry.error_message)
        .bind(entry.duration_ms)
        .bind(&now)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn list_mcp_audit(&self, limit: i64) -> Result<Vec<McpAuditRecord>, DbError> {
        let clamped = limit.clamp(1, 500);
        let rows = sqlx::query_as::<_, McpAuditRecord>(
            r#"
            SELECT id, transport, tool_name, session_id, arguments_json,
                   status, error_message, duration_ms, created_at
            FROM mcp_audit_log
            ORDER BY created_at DESC
            LIMIT ?1
            "#,
        )
        .bind(clamped)
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }
}
