use std::sync::Arc;

use crate::config::AppConfig;
use crate::core::sessions::{SessionError, SessionManager};
use crate::core::webhooks::WebhookService;
use crate::storage::database::Database;
use crate::storage::database::audit::{McpAuditRecord, NewMcpAudit};
use crate::storage::database::messages::{
    ChatSummaryRecord, MessageRecord, MessageWithTranscriptionRecord,
};
use crate::storage::database::sessions::SessionRecord;
use crate::storage::database::transcriptions::TranscriptionRecord;
use crate::storage::media::MediaStorage;
use crate::transcription::TranscriptionEngine;

/// Shared application service layer consumed by both the REST API and the MCP server.
///
/// Keeping domain operations here guarantees that MCP never depends on HTTP REST
/// and that validation, rate limiting, idempotency, and WhatsApp session state
/// remain single-source-of-truth.
#[derive(Clone)]
pub struct RelayService {
    pub config: AppConfig,
    pub db: Database,
    pub media: Arc<dyn MediaStorage>,
    pub sessions: Arc<SessionManager>,
    pub webhooks: WebhookService,
    pub transcription_engine: Arc<dyn TranscriptionEngine>,
}

impl RelayService {
    pub fn new(
        config: AppConfig,
        db: Database,
        media: Arc<dyn MediaStorage>,
        sessions: Arc<SessionManager>,
        webhooks: WebhookService,
        transcription_engine: Arc<dyn TranscriptionEngine>,
    ) -> Self {
        Self {
            config,
            db,
            media,
            sessions,
            webhooks,
            transcription_engine,
        }
    }

    pub async fn list_sessions(&self) -> Result<Vec<SessionRecord>, SessionError> {
        self.sessions.list_sessions().await
    }

    pub async fn get_session(&self, session_id: &str) -> Result<SessionRecord, SessionError> {
        self.sessions.get_session(session_id).await
    }

    pub async fn get_session_status(
        &self,
        session_id: &str,
    ) -> Result<(SessionRecord, String), SessionError> {
        self.sessions.get_status(session_id).await
    }

    pub async fn list_chats(
        &self,
        session_id: &str,
        query: Option<&str>,
        limit: i64,
    ) -> Result<Vec<ChatSummaryRecord>, SessionError> {
        // Ensure the session exists before querying its chats.
        let _ = self.sessions.get_session(session_id).await?;
        Ok(self.db.list_chats(session_id, query, limit).await?)
    }

    pub async fn get_chat_messages(
        &self,
        session_id: &str,
        chat_jid: &str,
        limit: i64,
    ) -> Result<Vec<MessageWithTranscriptionRecord>, SessionError> {
        let _ = self.sessions.get_session(session_id).await?;
        Ok(self
            .db
            .get_chat_messages(session_id, chat_jid, limit)
            .await?)
    }

    pub async fn search_messages(
        &self,
        session_id: &str,
        query: &str,
        limit: i64,
    ) -> Result<Vec<MessageWithTranscriptionRecord>, SessionError> {
        let _ = self.sessions.get_session(session_id).await?;
        if query.trim().is_empty() {
            return Err(SessionError::BadRequest("query must not be empty".into()));
        }
        Ok(self.db.search_messages(session_id, query, limit).await?)
    }

    pub async fn send_text_message(
        &self,
        session_id: &str,
        to: &str,
        text: &str,
    ) -> Result<MessageRecord, SessionError> {
        self.sessions.send_text(session_id, to, text).await
    }

    pub async fn get_message_transcription(
        &self,
        message_id: &str,
    ) -> Result<Option<TranscriptionRecord>, SessionError> {
        Ok(self.db.get_transcription(message_id).await?)
    }

    pub async fn logout_session(&self, session_id: &str) -> Result<(), SessionError> {
        self.sessions.logout_session(session_id).await
    }

    pub async fn delete_session(&self, session_id: &str) -> Result<(), SessionError> {
        self.sessions.delete_session(session_id).await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_mcp_audit(
        &self,
        transport: &str,
        tool_name: &str,
        session_id: Option<&str>,
        arguments_json: &str,
        status: &str,
        error_message: Option<&str>,
        duration_ms: i64,
    ) {
        let id = format!("aud_{}", uuid::Uuid::now_v7());
        if let Err(err) = self
            .db
            .insert_mcp_audit(NewMcpAudit {
                id: &id,
                transport,
                tool_name,
                session_id,
                arguments_json,
                status,
                error_message,
                duration_ms,
            })
            .await
        {
            tracing::warn!(error = %err, tool = %tool_name, "failed to persist mcp_audit_log entry");
        }
    }

    pub async fn list_mcp_audit(&self, limit: i64) -> Result<Vec<McpAuditRecord>, SessionError> {
        Ok(self.db.list_mcp_audit(limit).await?)
    }
}
