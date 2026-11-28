use std::sync::Arc;

use crate::config::AppConfig;
use crate::core::pairing::{PairingLinkRecord, PairingLinkStore};
use crate::core::sessions::{SessionError, SessionManager};
use crate::core::webhooks::WebhookService;
use crate::engines::whatsapp::PairCodePayload;
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
    pub pairing_links: PairingLinkStore,
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
            pairing_links: PairingLinkStore::new(),
        }
    }

    pub async fn create_session(
        &self,
        name: &str,
        transcription_language: Option<&str>,
    ) -> Result<SessionRecord, SessionError> {
        self.sessions
            .create_session(name, transcription_language)
            .await
    }

    pub async fn request_pair_code(
        &self,
        session_id: &str,
        phone_number: &str,
    ) -> Result<PairCodePayload, SessionError> {
        self.sessions
            .request_pair_code(session_id, phone_number)
            .await
    }

    pub async fn create_pairing_link(
        &self,
        session_id: &str,
        ttl_seconds: Option<i64>,
    ) -> Result<PairingLinkRecord, SessionError> {
        let _ = self.sessions.get_session(session_id).await?;
        Ok(self.pairing_links.create(session_id, ttl_seconds).await)
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
        let trimmed = chat_jid.trim();
        let (primary, alias) = match self.sessions.get_contact(session_id, trimmed).await {
            Ok(contact) => {
                let alt = if trimmed.ends_with("@lid") {
                    Some(contact.jid.clone())
                } else {
                    contact.lid
                };
                (
                    if trimmed.contains('@') {
                        trimmed.to_string()
                    } else {
                        contact.jid
                    },
                    alt,
                )
            }
            Err(_) => (trimmed.to_string(), None),
        };
        Ok(self
            .db
            .get_chat_messages_with_alias(session_id, &primary, alias.as_deref(), limit)
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
        idempotency_key: Option<&str>,
    ) -> Result<MessageRecord, SessionError> {
        self.send_text_message_with_reply(session_id, to, text, None, idempotency_key)
            .await
    }

    pub async fn send_text_message_with_reply(
        &self,
        session_id: &str,
        to: &str,
        text: &str,
        reply_to_message_id: Option<&str>,
        idempotency_key: Option<&str>,
    ) -> Result<MessageRecord, SessionError> {
        let key = idempotency_key.map(str::trim).filter(|k| !k.is_empty());
        let req_hash = if key.is_some() {
            use sha2::{Digest, Sha256};
            let mut h = Sha256::new();
            h.update(
                format!(
                    "text:{to}:{text}:{}",
                    reply_to_message_id.unwrap_or_default()
                )
                .as_bytes(),
            );
            Some(hex::encode(h.finalize()))
        } else {
            None
        };

        if let (Some(k), Some(hash)) = (key, req_hash.as_deref())
            && let Some(existing) = self.db.get_idempotency_key(k, session_id).await?
        {
            if existing.request_hash != hash {
                return Err(SessionError::Conflict(
                    "idempotency_key already used with a different request payload".into(),
                ));
            }
            if let Ok(cached) = serde_json::from_str::<MessageRecord>(&existing.response_body) {
                return Ok(cached);
            }
        }

        let record = self
            .sessions
            .send_text_with_reply(session_id, to, text, reply_to_message_id)
            .await?;

        if let (Some(k), Some(hash)) = (key, req_hash.as_deref())
            && let Ok(json_body) = serde_json::to_string(&record)
        {
            let _ = self
                .db
                .save_idempotency_key(k, session_id, hash, 201, &json_body)
                .await;
        }

        Ok(record)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn send_media_message(
        &self,
        session_id: &str,
        to: &str,
        kind: crate::engines::whatsapp::MediaKind,
        mime_type: Option<&str>,
        filename: Option<&str>,
        caption: Option<&str>,
        data_base64: Option<&str>,
        media_url: Option<&str>,
        idempotency_key: Option<&str>,
    ) -> Result<MessageRecord, SessionError> {
        self.send_media_message_with_options(
            session_id,
            to,
            kind,
            mime_type,
            filename,
            caption,
            data_base64,
            media_url,
            false,
            None,
            idempotency_key,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn send_media_message_with_options(
        &self,
        session_id: &str,
        to: &str,
        kind: crate::engines::whatsapp::MediaKind,
        mime_type: Option<&str>,
        filename: Option<&str>,
        caption: Option<&str>,
        data_base64: Option<&str>,
        media_url: Option<&str>,
        as_voice_note: bool,
        reply_to_message_id: Option<&str>,
        idempotency_key: Option<&str>,
    ) -> Result<MessageRecord, SessionError> {
        let key = idempotency_key.map(str::trim).filter(|k| !k.is_empty());
        let source_sig = media_url
            .map(|u| format!("url:{u}"))
            .or_else(|| data_base64.map(|b| format!("b64:{}", b.len())))
            .unwrap_or_default();
        let req_hash = if key.is_some() {
            use sha2::{Digest, Sha256};
            let mut h = Sha256::new();
            h.update(
                format!(
                    "media:{to}:{}:{source_sig}:{as_voice_note}:{}",
                    kind.as_str(),
                    reply_to_message_id.unwrap_or_default()
                )
                .as_bytes(),
            );
            Some(hex::encode(h.finalize()))
        } else {
            None
        };

        if let (Some(k), Some(hash)) = (key, req_hash.as_deref())
            && let Some(existing) = self.db.get_idempotency_key(k, session_id).await?
        {
            if existing.request_hash != hash {
                return Err(SessionError::Conflict(
                    "idempotency_key already used with a different request payload".into(),
                ));
            }
            if let Ok(cached) = serde_json::from_str::<MessageRecord>(&existing.response_body) {
                return Ok(cached);
            }
        }

        let (bytes, inferred_mime, inferred_name) = match (media_url, data_base64) {
            (Some(url), _) if !url.trim().is_empty() => {
                self.sessions.fetch_media_from_url(url).await?
            }
            (_, Some(b64)) if !b64.trim().is_empty() => {
                use base64::Engine as _;
                let raw = base64::engine::general_purpose::STANDARD
                    .decode(b64.trim())
                    .map_err(|e| {
                        SessionError::BadRequest(format!("invalid base64 payload: {e}"))
                    })?;
                (bytes::Bytes::from(raw), None, None)
            }
            _ => {
                return Err(SessionError::BadRequest(
                    "either media_url or data_base64 is required".into(),
                ));
            }
        };

        let effective_filename = filename
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .or(inferred_name);
        let effective_mime = mime_type
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .or(inferred_mime)
            .or_else(|| {
                effective_filename
                    .as_deref()
                    .and_then(|f| mime_guess::from_path(f).first())
                    .map(|m| m.to_string())
            })
            .unwrap_or_else(|| match kind {
                crate::engines::whatsapp::MediaKind::Image => "image/jpeg".to_string(),
                crate::engines::whatsapp::MediaKind::Video => "video/mp4".to_string(),
                crate::engines::whatsapp::MediaKind::Audio => "audio/ogg".to_string(),
                crate::engines::whatsapp::MediaKind::Document => {
                    "application/octet-stream".to_string()
                }
                crate::engines::whatsapp::MediaKind::Sticker => "image/webp".to_string(),
            });

        let record = self
            .sessions
            .send_media_with_options(
                session_id,
                to,
                kind,
                &effective_mime,
                effective_filename.as_deref(),
                caption,
                bytes,
                as_voice_note,
                reply_to_message_id,
            )
            .await?;

        if let (Some(k), Some(hash)) = (key, req_hash.as_deref())
            && let Ok(json_body) = serde_json::to_string(&record)
        {
            let _ = self
                .db
                .save_idempotency_key(k, session_id, hash, 201, &json_body)
                .await;
        }

        Ok(record)
    }

    pub async fn send_reaction(
        &self,
        session_id: &str,
        chat_jid: &str,
        message_external_id: &str,
        emoji: &str,
        from_me: bool,
        participant: Option<&str>,
    ) -> Result<String, SessionError> {
        self.sessions
            .send_reaction(
                session_id,
                chat_jid,
                message_external_id,
                emoji,
                from_me,
                participant,
            )
            .await
    }

    pub async fn send_chat_presence(
        &self,
        session_id: &str,
        chat_jid: &str,
        state: crate::engines::whatsapp::ChatPresenceState,
    ) -> Result<(), SessionError> {
        self.sessions
            .send_chat_presence(session_id, chat_jid, state)
            .await
    }

    pub async fn subscribe_presence(
        &self,
        session_id: &str,
        jid: &str,
    ) -> Result<(), SessionError> {
        self.sessions.subscribe_presence(session_id, jid).await
    }

    pub async fn send_location(
        &self,
        session_id: &str,
        to: &str,
        latitude: f64,
        longitude: f64,
        name: Option<&str>,
        address: Option<&str>,
    ) -> Result<MessageRecord, SessionError> {
        self.sessions
            .send_location(session_id, to, latitude, longitude, name, address)
            .await
    }

    pub async fn send_contact(
        &self,
        session_id: &str,
        to: &str,
        display_name: &str,
        phone_number: Option<&str>,
        vcard: Option<&str>,
    ) -> Result<MessageRecord, SessionError> {
        self.sessions
            .send_contact(session_id, to, display_name, phone_number, vcard)
            .await
    }

    pub async fn edit_message(
        &self,
        session_id: &str,
        chat_jid: &str,
        external_message_id: &str,
        new_text: &str,
    ) -> Result<String, SessionError> {
        self.sessions
            .edit_message(session_id, chat_jid, external_message_id, new_text)
            .await
    }

    pub async fn revoke_message(
        &self,
        session_id: &str,
        chat_jid: &str,
        external_message_id: &str,
    ) -> Result<(), SessionError> {
        self.sessions
            .revoke_message(session_id, chat_jid, external_message_id)
            .await
    }

    pub async fn is_on_whatsapp(
        &self,
        session_id: &str,
        phones: &[String],
    ) -> Result<Vec<crate::engines::whatsapp::WhatsAppCheckResult>, SessionError> {
        self.sessions.is_on_whatsapp(session_id, phones).await
    }

    pub async fn get_profile_picture(
        &self,
        session_id: &str,
        jid_or_phone: &str,
        preview: bool,
    ) -> Result<Option<crate::engines::whatsapp::ProfilePictureInfo>, SessionError> {
        self.sessions
            .get_profile_picture(session_id, jid_or_phone, preview)
            .await
    }

    pub async fn list_groups(
        &self,
        session_id: &str,
    ) -> Result<Vec<crate::engines::whatsapp::GroupInfoView>, SessionError> {
        self.sessions.list_groups(session_id).await
    }

    pub async fn get_group_info(
        &self,
        session_id: &str,
        group_jid: &str,
    ) -> Result<crate::engines::whatsapp::GroupInfoView, SessionError> {
        self.sessions.get_group_info(session_id, group_jid).await
    }

    pub async fn get_media_bytes(
        &self,
        media_id: &str,
    ) -> Result<
        (
            crate::storage::database::media::MediaObjectRecord,
            bytes::Bytes,
        ),
        SessionError,
    > {
        self.sessions.get_media_bytes(media_id).await
    }

    pub async fn mark_read(
        &self,
        session_id: &str,
        chat_jid: &str,
        sender_jid: Option<&str>,
        message_ids: Vec<String>,
    ) -> Result<usize, SessionError> {
        self.sessions
            .mark_read(session_id, chat_jid, sender_jid, message_ids)
            .await
    }

    pub async fn get_contact(
        &self,
        session_id: &str,
        jid_or_phone: &str,
    ) -> Result<crate::engines::whatsapp::ContactInfo, SessionError> {
        self.sessions.get_contact(session_id, jid_or_phone).await
    }

    pub async fn list_contacts(
        &self,
        session_id: &str,
        query: Option<&str>,
        limit: i64,
    ) -> Result<Vec<crate::storage::database::contacts::ContactRecord>, SessionError> {
        self.sessions.list_contacts(session_id, query, limit).await
    }

    pub async fn save_contact(
        &self,
        session_id: &str,
        jid_or_phone: &str,
        full_name: Option<&str>,
        first_name: Option<&str>,
        save_on_primary_addressbook: bool,
    ) -> Result<crate::storage::database::contacts::ContactRecord, SessionError> {
        self.sessions
            .save_contact(
                session_id,
                jid_or_phone,
                full_name,
                first_name,
                save_on_primary_addressbook,
            )
            .await
    }

    pub async fn sync_contacts(&self, session_id: &str) -> Result<(u64, i64), SessionError> {
        self.sessions.sync_contacts(session_id).await
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
