use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use thiserror::Error;
use tokio::sync::{Mutex, RwLock, mpsc, watch};
use tracing::{error, info, warn};

use crate::config::AppConfig;
use crate::core::events::EventBus;
use crate::core::queue::{QueueError, TranscriptionQueue};
use crate::engines::whatsapp::{
    EngineError, EngineEvent, MediaKind, QrPayload, SendMediaParams, SendTextParams,
    SessionConnectionState, WhatsAppEngine,
};
use crate::storage::database::media::NewMediaObject;
use crate::storage::database::messages::{MessageRecord, NewMessage};
use crate::storage::database::sessions::SessionRecord;
use crate::storage::database::transcriptions::{TranscriptionRecord, UpsertTranscription};
use crate::storage::database::{Database, DbError};
use crate::storage::media::{MediaError, MediaStorage, build_media_object_key};

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("session not found: {0}")]
    NotFound(String),
    #[error("message not found: {0}")]
    MessageNotFound(String),
    #[error("rate limit exceeded for session {0}")]
    RateLimited(String),
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("engine error: {0}")]
    Engine(#[from] EngineError),
    #[error("database error: {0}")]
    Db(#[from] DbError),
    #[error("media storage error: {0}")]
    Media(#[from] MediaError),
    #[error("transcription queue error: {0}")]
    Queue(#[from] QueueError),
}

struct RateLimiter {
    limit_per_minute: u32,
    windows: Mutex<HashMap<String, VecDeque<Instant>>>,
}

impl RateLimiter {
    fn new(limit_per_minute: u32) -> Self {
        Self {
            limit_per_minute,
            windows: Mutex::new(HashMap::new()),
        }
    }

    async fn check(&self, session_id: &str) -> bool {
        if self.limit_per_minute == 0 {
            return true;
        }
        let now = Instant::now();
        let cutoff = now - Duration::from_secs(60);
        let mut guard = self.windows.lock().await;
        let entries = guard.entry(session_id.to_string()).or_default();
        while entries.front().is_some_and(|t| *t < cutoff) {
            entries.pop_front();
        }
        if entries.len() >= self.limit_per_minute as usize {
            false
        } else {
            entries.push_back(now);
            true
        }
    }
}

type CoalesceMap = Arc<Mutex<HashMap<(String, String), Vec<serde_json::Value>>>>;

pub struct SessionManager {
    db: Database,
    media: Arc<dyn MediaStorage>,
    engine: Arc<dyn WhatsAppEngine>,
    events: EventBus,
    tx_queue: TranscriptionQueue,
    config: AppConfig,
    event_tx: mpsc::Sender<EngineEvent>,
    qr_cache: RwLock<HashMap<String, QrPayload>>,
    rate_limiter: RateLimiter,
    presence_subscribed: Mutex<std::collections::HashSet<(String, String)>>,
    coalesce_buffers: CoalesceMap,
    seen_connected: Mutex<std::collections::HashSet<String>>,
    reconnect_count: std::sync::atomic::AtomicU64,
}

impl SessionManager {
    pub fn new(
        db: Database,
        media: Arc<dyn MediaStorage>,
        engine: Arc<dyn WhatsAppEngine>,
        events: EventBus,
        tx_queue: TranscriptionQueue,
        config: AppConfig,
    ) -> (Arc<Self>, mpsc::Receiver<EngineEvent>) {
        let (event_tx, event_rx) = mpsc::channel(512);
        let rate_limiter = RateLimiter::new(config.session_rate_limit_per_minute);
        let manager = Arc::new(Self {
            db,
            media,
            engine,
            events,
            tx_queue,
            config,
            event_tx,
            qr_cache: RwLock::new(HashMap::new()),
            rate_limiter,
            presence_subscribed: Mutex::new(std::collections::HashSet::new()),
            coalesce_buffers: Arc::new(Mutex::new(HashMap::new())),
            seen_connected: Mutex::new(std::collections::HashSet::new()),
            reconnect_count: std::sync::atomic::AtomicU64::new(0),
        });
        (manager, event_rx)
    }

    pub fn reconnect_count(&self) -> u64 {
        self.reconnect_count
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn spawn_event_loop(
        self: Arc<Self>,
        mut event_rx: mpsc::Receiver<EngineEvent>,
        mut shutdown: watch::Receiver<bool>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            info!("session event loop started");
            loop {
                tokio::select! {
                    maybe_ev = event_rx.recv() => {
                        match maybe_ev {
                            Some(ev) => self.handle_engine_event(ev).await,
                            None => break,
                        }
                    }
                    _ = shutdown.changed() => {
                        if *shutdown.borrow() {
                            info!("session event loop shutting down");
                            break;
                        }
                    }
                }
            }
        })
    }

    pub async fn restore_sessions(&self) -> Result<usize, SessionError> {
        let restorable = self.db.list_restorable_sessions().await?;
        let mut restored = 0usize;
        for session in restorable {
            match self
                .engine
                .start_session(&session.id, self.event_tx.clone())
                .await
            {
                Ok(()) => {
                    restored += 1;
                }
                Err(err) => {
                    warn!(session_id = %session.id, error = %err, "failed to restore whatsapp session");
                }
            }
        }
        Ok(restored)
    }

    pub async fn create_session(
        &self,
        name: &str,
        transcription_language: Option<&str>,
    ) -> Result<SessionRecord, SessionError> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(SessionError::BadRequest(
                "session name cannot be empty".into(),
            ));
        }
        let id = format!("sess_{}", uuid::Uuid::now_v7());
        let record = self
            .db
            .create_session(
                &id,
                trimmed,
                SessionConnectionState::Created.as_str(),
                transcription_language,
            )
            .await?;

        self.engine
            .start_session(&id, self.event_tx.clone())
            .await?;

        Ok(record)
    }

    pub async fn list_sessions(&self) -> Result<Vec<SessionRecord>, SessionError> {
        let mut sessions = self.db.list_sessions().await?;
        for rec in &mut sessions {
            if let Ok(live) = self.engine.get_status(&rec.id).await {
                rec.status = live.as_str().to_string();
            }
        }
        Ok(sessions)
    }

    pub async fn get_session(&self, session_id: &str) -> Result<SessionRecord, SessionError> {
        let mut rec = self
            .db
            .get_session(session_id)
            .await?
            .ok_or_else(|| SessionError::NotFound(session_id.to_string()))?;
        if let Ok(live) = self.engine.get_status(&rec.id).await {
            rec.status = live.as_str().to_string();
        }
        Ok(rec)
    }

    pub async fn get_qr(&self, session_id: &str) -> Result<Option<QrPayload>, SessionError> {
        let _ = self.get_session(session_id).await?;
        if let Ok(Some(qr)) = self.engine.get_qr(session_id).await {
            return Ok(Some(qr));
        }
        Ok(self.qr_cache.read().await.get(session_id).cloned())
    }

    pub async fn request_pair_code(
        &self,
        session_id: &str,
        phone_number: &str,
    ) -> Result<crate::engines::whatsapp::PairCodePayload, SessionError> {
        let digits: String = phone_number
            .chars()
            .filter(|c| c.is_ascii_digit())
            .collect();
        if digits.len() < 8 || digits.len() > 15 {
            return Err(SessionError::BadRequest(
                "phone_number must contain 8 to 15 digits including country code (e.g., 5511999990001)"
                    .into(),
            ));
        }
        let (rec, live_status) = self.get_status(session_id).await?;
        if live_status == SessionConnectionState::Connected.as_str()
            || rec.status == SessionConnectionState::Connected.as_str()
        {
            return Err(SessionError::Conflict(
                "session is already connected".into(),
            ));
        }
        // Ensure session socket is running before requesting pair code
        if self.engine.get_status(session_id).await.is_err()
            || live_status == SessionConnectionState::Disconnected.as_str()
            || live_status == SessionConnectionState::LoggedOut.as_str()
        {
            self.engine
                .start_session(session_id, self.event_tx.clone())
                .await?;
        }
        let payload = self.engine.request_pair_code(session_id, &digits).await?;
        Ok(payload)
    }

    pub async fn get_pair_code(
        &self,
        session_id: &str,
    ) -> Result<Option<crate::engines::whatsapp::PairCodePayload>, SessionError> {
        let _ = self.get_session(session_id).await?;
        match self.engine.get_pair_code(session_id).await {
            Ok(p) => Ok(p),
            Err(_) => Ok(None),
        }
    }

    pub async fn get_status(
        &self,
        session_id: &str,
    ) -> Result<(SessionRecord, String), SessionError> {
        let rec = self.get_session(session_id).await?;
        let live_status = match self.engine.get_status(session_id).await {
            Ok(s) => s.as_str().to_string(),
            Err(_) => rec.status.clone(),
        };
        Ok((rec, live_status))
    }

    pub async fn reconnect_session(&self, session_id: &str) -> Result<SessionRecord, SessionError> {
        let (rec, live_status) = self.get_status(session_id).await?;
        let was_logged_out = rec.status == SessionConnectionState::LoggedOut.as_str()
            || live_status == SessionConnectionState::LoggedOut.as_str();
        let _ = self.engine.stop_session(session_id).await;
        if was_logged_out {
            // Clear revoked WhatsApp credentials so re-pairing generates a fresh QR code
            let _ = self.engine.delete_session_data(session_id).await;
        }
        self.qr_cache.write().await.remove(session_id);
        self.db
            .update_session_status(
                session_id,
                SessionConnectionState::Connecting.as_str(),
                None,
                None,
                None,
            )
            .await?;
        self.engine
            .start_session(session_id, self.event_tx.clone())
            .await?;
        self.get_session(&rec.id).await
    }

    pub async fn delete_session(&self, session_id: &str) -> Result<(), SessionError> {
        let (rec, live_status) = self.get_status(session_id).await?;
        if live_status == SessionConnectionState::Connected.as_str()
            || rec.status == SessionConnectionState::Connected.as_str()
        {
            return Err(SessionError::Conflict(
                "disconnect/logout the session before deleting it".into(),
            ));
        }
        let _ = self.engine.stop_session(session_id).await;
        let _ = self.engine.delete_session_data(session_id).await;
        self.qr_cache.write().await.remove(session_id);
        self.db.delete_session(session_id).await?;
        Ok(())
    }

    pub async fn logout_session(&self, session_id: &str) -> Result<(), SessionError> {
        let _ = self.get_session(session_id).await?;
        let _ = self.engine.logout_session(session_id).await;
        self.qr_cache.write().await.remove(session_id);
        self.db
            .update_session_status(
                session_id,
                SessionConnectionState::LoggedOut.as_str(),
                None,
                None,
                Some("logout"),
            )
            .await?;
        let _ = self
            .events
            .publish(
                "session.status",
                session_id,
                serde_json::json!({
                    "session_id": session_id,
                    "status": "logged_out",
                    "reason": "logout"
                }),
            )
            .await;
        Ok(())
    }

    pub async fn send_text(
        &self,
        session_id: &str,
        to: &str,
        text: &str,
    ) -> Result<MessageRecord, SessionError> {
        self.send_text_with_reply(session_id, to, text, None).await
    }

    pub async fn send_text_with_reply(
        &self,
        session_id: &str,
        to: &str,
        text: &str,
        reply_to_id: Option<&str>,
    ) -> Result<MessageRecord, SessionError> {
        let session = self.get_session(session_id).await?;
        if session.status == SessionConnectionState::LoggedOut.as_str() {
            let detail = match session.status_reason.as_deref() {
                Some("device_removed") => {
                    "WhatsApp disconnected this linked device (device_removed). Check the WhatsApp app on your phone for any temporary account restriction before re-pairing."
                }
                Some("account_restricted") => {
                    "WhatsApp temporarily restricted or suspended this account (account_restricted). Wait until the restriction expires before reconnecting."
                }
                Some(r) => {
                    return Err(SessionError::Conflict(format!(
                        "session {session_id} is logged out ({r}); reconnect before sending messages"
                    )));
                }
                None => {
                    "session is logged out; check your phone for any account restriction and reconnect before sending messages"
                }
            };
            return Err(SessionError::Conflict(detail.to_string()));
        }
        if !self.rate_limiter.check(session_id).await {
            return Err(SessionError::RateLimited(session_id.to_string()));
        }
        if text.is_empty() {
            return Err(SessionError::BadRequest("text cannot be empty".into()));
        }

        let res = self
            .engine
            .send_text(
                session_id,
                SendTextParams {
                    to: to.to_string(),
                    text: text.to_string(),
                    reply_to_id: reply_to_id.map(ToString::to_string),
                },
            )
            .await?;

        let msg_id = format!("msg_{}", uuid::Uuid::now_v7());
        let (canonical_chat_jid, chat_lid) = self.resolve_outbound_chat_jid(session_id, to).await;
        let meta_str = match chat_lid {
            Some(ref lid) => serde_json::json!({ "chat_lid": lid }).to_string(),
            None => "{}".to_string(),
        };
        let record = self
            .db
            .insert_message(NewMessage {
                id: &msg_id,
                session_id,
                external_id: Some(&res.external_message_id),
                direction: "outbound",
                chat_jid: &canonical_chat_jid,
                sender_jid: None,
                message_type: "text",
                text_body: Some(text),
                caption: None,
                media_id: None,
                status: "sent",
                metadata_json: &meta_str,
                created_at: None,
            })
            .await?;

        let _ = self
            .events
            .publish(
                "message.sent",
                session_id,
                serde_json::json!({
                    "message_id": record.id,
                    "external_id": record.external_id,
                    "chat_jid": record.chat_jid,
                    "message_type": record.message_type,
                    "status": record.status,
                }),
            )
            .await;

        Ok(record)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn send_media(
        &self,
        session_id: &str,
        to: &str,
        kind: MediaKind,
        mime_type: &str,
        filename: Option<&str>,
        caption: Option<&str>,
        data: Bytes,
    ) -> Result<MessageRecord, SessionError> {
        self.send_media_with_options(
            session_id, to, kind, mime_type, filename, caption, data, false, None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn send_media_with_options(
        &self,
        session_id: &str,
        to: &str,
        kind: MediaKind,
        mime_type: &str,
        filename: Option<&str>,
        caption: Option<&str>,
        data: Bytes,
        as_voice_note: bool,
        reply_to_id: Option<&str>,
    ) -> Result<MessageRecord, SessionError> {
        let session = self.get_session(session_id).await?;
        if session.status == SessionConnectionState::LoggedOut.as_str() {
            let detail = match session.status_reason.as_deref() {
                Some("device_removed") => {
                    "WhatsApp disconnected this linked device (device_removed). Check the WhatsApp app on your phone for any temporary account restriction before re-pairing."
                }
                Some("account_restricted") => {
                    "WhatsApp temporarily restricted or suspended this account (account_restricted). Wait until the restriction expires before reconnecting."
                }
                Some(r) => {
                    return Err(SessionError::Conflict(format!(
                        "session {session_id} is logged out ({r}); reconnect before sending messages"
                    )));
                }
                None => {
                    "session is logged out; check your phone for any account restriction and reconnect before sending messages"
                }
            };
            return Err(SessionError::Conflict(detail.to_string()));
        }
        if !self.rate_limiter.check(session_id).await {
            return Err(SessionError::RateLimited(session_id.to_string()));
        }
        if data.is_empty() {
            return Err(SessionError::BadRequest("media payload is empty".into()));
        }
        if data.len() > self.config.media_max_upload_bytes {
            return Err(SessionError::BadRequest(format!(
                "media payload exceeds max size of {} bytes",
                self.config.media_max_upload_bytes
            )));
        }

        let media_id = format!("med_{}", uuid::Uuid::now_v7());
        let ext = extension_from_mime(mime_type);
        let key = build_media_object_key(session_id, &media_id, ext);
        let stored = self.media.put(&key, data.clone()).await?;

        self.db
            .insert_media_object(NewMediaObject {
                id: &media_id,
                session_id,
                storage_backend: stored.backend,
                object_key: &stored.object_key,
                mime_type,
                size_bytes: stored.size_bytes,
                sha256: &stored.sha256,
                filename,
            })
            .await?;

        let res = self
            .engine
            .send_media(
                session_id,
                SendMediaParams {
                    to: to.to_string(),
                    kind,
                    mime_type: mime_type.to_string(),
                    filename: filename.map(ToString::to_string),
                    caption: caption.map(ToString::to_string),
                    data,
                    as_voice_note,
                    reply_to_id: reply_to_id.map(ToString::to_string),
                },
            )
            .await?;

        let msg_id = format!("msg_{}", uuid::Uuid::now_v7());
        let (canonical_chat_jid, chat_lid) = self.resolve_outbound_chat_jid(session_id, to).await;
        let meta_str = match chat_lid {
            Some(ref lid) => serde_json::json!({ "chat_lid": lid }).to_string(),
            None => "{}".to_string(),
        };
        let record = self
            .db
            .insert_message(NewMessage {
                id: &msg_id,
                session_id,
                external_id: Some(&res.external_message_id),
                direction: "outbound",
                chat_jid: &canonical_chat_jid,
                sender_jid: None,
                message_type: kind.as_str(),
                text_body: None,
                caption,
                media_id: Some(&media_id),
                status: "sent",
                metadata_json: &meta_str,
                created_at: None,
            })
            .await?;

        let _ = self
            .events
            .publish(
                "message.sent",
                session_id,
                serde_json::json!({
                    "message_id": record.id,
                    "external_id": record.external_id,
                    "chat_jid": record.chat_jid,
                    "message_type": record.message_type,
                    "media_id": media_id,
                    "status": record.status,
                }),
            )
            .await;

        Ok(record)
    }

    async fn resolve_outbound_chat_jid(
        &self,
        session_id: &str,
        to: &str,
    ) -> (String, Option<String>) {
        let trimmed = to.trim();
        if trimmed.ends_with("@lid") {
            if let Ok(contact) = self.get_contact(session_id, trimmed).await
                && !contact.jid.ends_with("@lid")
            {
                return (contact.jid, Some(trimmed.to_string()));
            }
            return (trimmed.to_string(), Some(trimmed.to_string()));
        }
        (normalize_outbound_chat_jid(trimmed), None)
    }

    pub async fn get_message(&self, message_id: &str) -> Result<MessageRecord, SessionError> {
        self.db
            .get_message(message_id)
            .await?
            .ok_or_else(|| SessionError::MessageNotFound(message_id.to_string()))
    }

    pub async fn get_media_bytes(
        &self,
        media_id: &str,
    ) -> Result<(crate::storage::database::media::MediaObjectRecord, Bytes), SessionError> {
        let rec = self
            .db
            .get_media_object(media_id)
            .await?
            .ok_or_else(|| SessionError::NotFound(format!("media {media_id}")))?;
        if rec.retention_state == "purged" {
            return Err(SessionError::NotFound(format!(
                "media {media_id} has been purged by retention policy"
            )));
        }
        let bytes = self.media.get(&rec.object_key).await?;
        Ok((rec, bytes))
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
        let _ = self.get_session(session_id).await?;
        let res = self
            .engine
            .send_reaction(
                session_id,
                crate::engines::whatsapp::SendReactionParams {
                    chat_jid: chat_jid.to_string(),
                    message_external_id: message_external_id.to_string(),
                    emoji: emoji.to_string(),
                    from_me,
                    participant: participant.map(ToString::to_string),
                },
            )
            .await?;
        Ok(res.external_message_id)
    }

    pub async fn send_chat_presence(
        &self,
        session_id: &str,
        chat_jid: &str,
        state: crate::engines::whatsapp::ChatPresenceState,
    ) -> Result<(), SessionError> {
        let _ = self.get_session(session_id).await?;
        self.engine
            .send_chat_presence(session_id, chat_jid, state)
            .await?;
        Ok(())
    }

    pub async fn subscribe_presence(
        &self,
        session_id: &str,
        jid: &str,
    ) -> Result<(), SessionError> {
        let _ = self.get_session(session_id).await?;
        self.engine.subscribe_presence(session_id, jid).await?;
        self.presence_subscribed
            .lock()
            .await
            .insert((session_id.to_string(), jid.to_string()));
        Ok(())
    }

    async fn maybe_auto_subscribe_presence(&self, session_id: &str, jid: &str) {
        if jid.ends_with("@g.us") || jid.ends_with("@newsletter") || jid == "status@broadcast" {
            return;
        }
        let key = (session_id.to_string(), jid.to_string());
        {
            let mut set = self.presence_subscribed.lock().await;
            if set.contains(&key) {
                return;
            }
            set.insert(key);
        }
        let _ = self.engine.subscribe_presence(session_id, jid).await;
    }

    /// Migrate legacy `@lid` chats in SQLite to canonical `{phone}@s.whatsapp.net` JIDs.
    pub async fn migrate_legacy_lid_history(&self, session_id: &str) -> u64 {
        let Ok(rows) = self.db.list_legacy_lid_chats(session_id).await else {
            return 0;
        };
        let mut migrated = 0u64;
        for (lid, cached_phone) in rows {
            let canonical_jid = if let Some(phone) = cached_phone
                .as_deref()
                .map(|p| p.chars().filter(|c| c.is_ascii_digit()).collect::<String>())
                .filter(|p| !p.is_empty())
            {
                Some(format!("{phone}@s.whatsapp.net"))
            } else if let Ok(contact) = self.get_contact(session_id, &lid).await
                && !contact.jid.ends_with("@lid")
            {
                Some(contact.jid)
            } else {
                None
            };
            if let Some(target_jid) = canonical_jid
                && let Ok(n) = self
                    .db
                    .rewrite_chat_jid_from_lid(session_id, &lid, &target_jid)
                    .await
            {
                migrated += n;
            }
        }
        if migrated > 0 {
            info!(session_id = %session_id, migrated_rows = migrated, "migrated legacy @lid chat history to canonical phone JIDs");
        }
        migrated
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
        let _ = self.get_session(session_id).await?;
        if !self.rate_limiter.check(session_id).await {
            return Err(SessionError::RateLimited(session_id.to_string()));
        }
        let res = self
            .engine
            .send_location(
                session_id,
                crate::engines::whatsapp::SendLocationParams {
                    to: to.to_string(),
                    latitude,
                    longitude,
                    name: name.map(ToString::to_string),
                    address: address.map(ToString::to_string),
                },
            )
            .await?;
        let msg_id = format!("msg_{}", uuid::Uuid::now_v7());
        let (canonical_chat_jid, chat_lid) = self.resolve_outbound_chat_jid(session_id, to).await;
        let summary = match name {
            Some(n) if !n.trim().is_empty() => format!("{n} ({latitude:.6}, {longitude:.6})"),
            _ => format!("{latitude:.6}, {longitude:.6}"),
        };
        let meta_str = serde_json::json!({
            "chat_lid": chat_lid,
            "latitude": latitude,
            "longitude": longitude,
            "name": name,
            "address": address,
        })
        .to_string();
        let record = self
            .db
            .insert_message(NewMessage {
                id: &msg_id,
                session_id,
                external_id: Some(&res.external_message_id),
                direction: "outbound",
                chat_jid: &canonical_chat_jid,
                sender_jid: None,
                message_type: "location",
                text_body: Some(&summary),
                caption: None,
                media_id: None,
                status: "sent",
                metadata_json: &meta_str,
                created_at: None,
            })
            .await?;
        Ok(record)
    }

    pub async fn send_contact(
        &self,
        session_id: &str,
        to: &str,
        display_name: &str,
        phone_number: Option<&str>,
        vcard: Option<&str>,
    ) -> Result<MessageRecord, SessionError> {
        let _ = self.get_session(session_id).await?;
        if !self.rate_limiter.check(session_id).await {
            return Err(SessionError::RateLimited(session_id.to_string()));
        }
        let vcard_str = match vcard.map(str::trim).filter(|s| !s.is_empty()) {
            Some(v) => v.to_string(),
            None => {
                let digits: String = phone_number
                    .unwrap_or("")
                    .chars()
                    .filter(|c| c.is_ascii_digit())
                    .collect();
                if digits.is_empty() {
                    return Err(SessionError::BadRequest(
                        "either vcard or phone_number is required".into(),
                    ));
                }
                format!(
                    "BEGIN:VCARD\nVERSION:3.0\nFN:{display_name}\nTEL;type=CELL;type=VOICE;waid={digits}:+{digits}\nEND:VCARD"
                )
            }
        };
        let res = self
            .engine
            .send_contact(
                session_id,
                crate::engines::whatsapp::SendContactParams {
                    to: to.to_string(),
                    display_name: display_name.to_string(),
                    vcard: vcard_str,
                },
            )
            .await?;
        let msg_id = format!("msg_{}", uuid::Uuid::now_v7());
        let (canonical_chat_jid, chat_lid) = self.resolve_outbound_chat_jid(session_id, to).await;
        let meta_str = serde_json::json!({ "chat_lid": chat_lid }).to_string();
        let record = self
            .db
            .insert_message(NewMessage {
                id: &msg_id,
                session_id,
                external_id: Some(&res.external_message_id),
                direction: "outbound",
                chat_jid: &canonical_chat_jid,
                sender_jid: None,
                message_type: "contact",
                text_body: Some(display_name),
                caption: None,
                media_id: None,
                status: "sent",
                metadata_json: &meta_str,
                created_at: None,
            })
            .await?;
        Ok(record)
    }

    pub async fn edit_message(
        &self,
        session_id: &str,
        chat_jid: &str,
        external_message_id: &str,
        new_text: &str,
    ) -> Result<String, SessionError> {
        let _ = self.get_session(session_id).await?;
        if new_text.trim().is_empty() {
            return Err(SessionError::BadRequest("new_text cannot be empty".into()));
        }
        let edit_id = self
            .engine
            .edit_message(session_id, chat_jid, external_message_id, new_text)
            .await?;
        let _ = self
            .db
            .update_message_text_by_external_id(session_id, external_message_id, new_text)
            .await;
        Ok(edit_id)
    }

    pub async fn revoke_message(
        &self,
        session_id: &str,
        chat_jid: &str,
        external_message_id: &str,
    ) -> Result<(), SessionError> {
        let _ = self.get_session(session_id).await?;
        self.engine
            .revoke_message(session_id, chat_jid, external_message_id)
            .await?;
        let _ = self
            .db
            .mark_message_revoked_by_external_id(session_id, external_message_id)
            .await;
        Ok(())
    }

    pub async fn is_on_whatsapp(
        &self,
        session_id: &str,
        phones: &[String],
    ) -> Result<Vec<crate::engines::whatsapp::WhatsAppCheckResult>, SessionError> {
        let _ = self.get_session(session_id).await?;
        Ok(self.engine.is_on_whatsapp(session_id, phones).await?)
    }

    pub async fn get_profile_picture(
        &self,
        session_id: &str,
        jid_or_phone: &str,
        preview: bool,
    ) -> Result<Option<crate::engines::whatsapp::ProfilePictureInfo>, SessionError> {
        let _ = self.get_session(session_id).await?;
        Ok(self
            .engine
            .get_profile_picture(session_id, jid_or_phone, preview)
            .await?)
    }

    pub async fn list_groups(
        &self,
        session_id: &str,
    ) -> Result<Vec<crate::engines::whatsapp::GroupInfoView>, SessionError> {
        let _ = self.get_session(session_id).await?;
        Ok(self.engine.list_groups(session_id).await?)
    }

    pub async fn get_group_info(
        &self,
        session_id: &str,
        group_jid: &str,
    ) -> Result<crate::engines::whatsapp::GroupInfoView, SessionError> {
        let _ = self.get_session(session_id).await?;
        Ok(self.engine.get_group_info(session_id, group_jid).await?)
    }

    pub async fn mark_read(
        &self,
        session_id: &str,
        chat_jid: &str,
        sender_jid: Option<&str>,
        message_ids: Vec<String>,
    ) -> Result<usize, SessionError> {
        let _ = self.get_session(session_id).await?;
        let chat = chat_jid.trim();
        if chat.is_empty() {
            return Err(SessionError::BadRequest("chat_jid cannot be empty".into()));
        }
        let ids = if message_ids.is_empty() {
            self.db
                .list_recent_inbound_external_ids(session_id, chat, 20)
                .await?
        } else {
            message_ids
                .into_iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        };
        if ids.is_empty() {
            return Ok(0);
        }
        self.engine
            .mark_read(session_id, chat, sender_jid, &ids)
            .await?;
        Ok(ids.len())
    }

    pub async fn get_contact(
        &self,
        session_id: &str,
        jid_or_phone: &str,
    ) -> Result<crate::engines::whatsapp::ContactInfo, SessionError> {
        let _ = self.get_session(session_id).await?;
        let query = jid_or_phone.trim();
        if query.is_empty() {
            return Err(SessionError::BadRequest(
                "jid_or_phone cannot be empty".into(),
            ));
        }

        let mut info = match self.engine.resolve_contact(session_id, query).await {
            Ok(resolved) => resolved,
            Err(_) => crate::engines::whatsapp::ContactInfo {
                query: query.to_string(),
                jid: query.to_string(),
                lid: if query.ends_with("@lid") {
                    Some(query.to_string())
                } else {
                    None
                },
                phone_number: None,
                full_name: None,
                first_name: None,
                push_name: None,
            },
        };

        if let Ok(Some(saved)) = self.db.get_contact_by_any(session_id, query).await {
            if info.full_name.is_none() {
                info.full_name = saved.full_name;
            }
            if info.first_name.is_none() {
                info.first_name = saved.first_name;
            }
            if info.push_name.is_none() {
                info.push_name = saved.push_name;
            }
            if info.lid.is_none() {
                info.lid = saved.lid;
            }
            if info.phone_number.is_none() {
                info.phone_number = saved.phone_number;
            }
            if (info.jid == query || info.jid.ends_with("@lid")) && !saved.jid.ends_with("@lid") {
                info.jid = saved.jid;
            }
        }

        if let Ok(Some((hist_jid, hist_name, hist_phone))) =
            self.db.lookup_contact_from_history(session_id, query).await
        {
            if info.push_name.is_none() {
                info.push_name = hist_name;
            }
            if info.phone_number.is_none() {
                info.phone_number = hist_phone;
            }
            if info.jid == query && !hist_jid.is_empty() {
                info.jid = hist_jid;
            }
        }

        if let Some(ref pn) = info.phone_number
            && info.jid.ends_with("@lid")
        {
            info.jid = format!("{pn}@s.whatsapp.net");
        }

        Ok(info)
    }

    pub async fn list_contacts(
        &self,
        session_id: &str,
        query: Option<&str>,
        limit: i64,
    ) -> Result<Vec<crate::storage::database::contacts::ContactRecord>, SessionError> {
        let _ = self.get_session(session_id).await?;
        Ok(self.db.list_contacts(session_id, query, limit).await?)
    }

    pub async fn save_contact(
        &self,
        session_id: &str,
        jid_or_phone: &str,
        full_name: Option<&str>,
        first_name: Option<&str>,
        save_on_primary_addressbook: bool,
    ) -> Result<crate::storage::database::contacts::ContactRecord, SessionError> {
        let _ = self.get_session(session_id).await?;
        let trimmed = jid_or_phone.trim();
        if trimmed.is_empty() {
            return Err(SessionError::BadRequest(
                "jid_or_phone cannot be empty".into(),
            ));
        }
        let canonical_jid = self
            .engine
            .save_contact(
                session_id,
                trimmed,
                full_name,
                first_name,
                save_on_primary_addressbook,
            )
            .await?;
        let phone = canonical_jid
            .split('@')
            .next()
            .map(|u| u.split(':').next().unwrap_or(u))
            .filter(|u| !u.is_empty() && u.chars().all(|c| c.is_ascii_digit()));
        let rec = self
            .db
            .upsert_contact(crate::storage::database::contacts::UpsertContact {
                session_id,
                jid: &canonical_jid,
                lid: None,
                phone_number: phone,
                full_name,
                first_name,
                push_name: None,
            })
            .await?;
        Ok(rec)
    }

    /// Backfill contacts from message history and force a full WhatsApp address-book sync
    /// (`CriticalUnblockLow`) so previously paired sessions receive all saved contact names.
    pub async fn sync_contacts(&self, session_id: &str) -> Result<(u64, i64), SessionError> {
        let _ = self.get_session(session_id).await?;
        let backfilled = self
            .db
            .backfill_contacts_from_messages(session_id)
            .await
            .unwrap_or(0);
        self.engine.sync_contacts(session_id).await?;
        let total = self.db.count_contacts(session_id).await.unwrap_or(0);
        Ok((backfilled, total))
    }

    pub async fn fetch_media_from_url(
        &self,
        url: &str,
    ) -> Result<(Bytes, Option<String>, Option<String>), SessionError> {
        let parsed = reqwest::Url::parse(url.trim())
            .map_err(|e| SessionError::BadRequest(format!("invalid media_url: {e}")))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(SessionError::BadRequest(
                "media_url must use http:// or https://".into(),
            ));
        }
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .map_err(|e| SessionError::BadRequest(format!("http client error: {e}")))?;
        let resp =
            client.get(parsed.clone()).send().await.map_err(|e| {
                SessionError::BadRequest(format!("failed to download media_url: {e}"))
            })?;
        if !resp.status().is_success() {
            return Err(SessionError::BadRequest(format!(
                "media_url returned HTTP {}",
                resp.status().as_u16()
            )));
        }
        if let Some(len) = resp.content_length()
            && len as usize > self.config.media_max_upload_bytes
        {
            return Err(SessionError::BadRequest(format!(
                "media_url payload exceeds max size of {} bytes",
                self.config.media_max_upload_bytes
            )));
        }
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.split(';').next().unwrap_or(s).trim().to_string())
            .filter(|s| !s.is_empty());
        let filename = parsed
            .path_segments()
            .and_then(|mut s| s.next_back())
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| SessionError::BadRequest(format!("failed to read media_url body: {e}")))?;
        if bytes.len() > self.config.media_max_upload_bytes {
            return Err(SessionError::BadRequest(format!(
                "media_url payload exceeds max size of {} bytes",
                self.config.media_max_upload_bytes
            )));
        }
        Ok((bytes, content_type, filename))
    }

    pub async fn request_transcription(
        &self,
        message_id: &str,
        language_override: Option<&str>,
    ) -> Result<TranscriptionRecord, SessionError> {
        let message = self.get_message(message_id).await?;
        if message.message_type != "audio" {
            return Err(SessionError::BadRequest(
                "transcription is only supported for audio messages".into(),
            ));
        }
        let Some(media_id) = message.media_id.as_deref() else {
            return Err(SessionError::BadRequest(
                "message has no associated media object".into(),
            ));
        };

        let session = self.get_session(&message.session_id).await?;
        let effective_lang = language_override
            .map(ToString::to_string)
            .or(session.transcription_language)
            .or_else(|| self.config.transcription.language.clone());

        self.tx_queue
            .enqueue(
                &message.id,
                &message.session_id,
                media_id,
                effective_lang.as_deref(),
            )
            .await?;

        let rec = self
            .db
            .upsert_transcription(UpsertTranscription {
                message_id: &message.id,
                session_id: &message.session_id,
                status: "queued",
                text: None,
                language: effective_lang.as_deref(),
                engine: &self.config.transcription.engine,
                model: self.config.transcription.model.as_str(),
                duration_ms: None,
                processing_ms: None,
                segments_json: "[]",
                error: None,
            })
            .await?;

        Ok(rec)
    }

    async fn handle_engine_event(&self, event: EngineEvent) {
        match event {
            EngineEvent::QrUpdated { session_id, qr } => {
                self.qr_cache.write().await.insert(session_id, qr);
            }
            EngineEvent::StatusChanged {
                session_id,
                status,
                phone_number,
                jid,
                reason,
            } => {
                if status == SessionConnectionState::Connected
                    || status == SessionConnectionState::LoggedOut
                {
                    self.qr_cache.write().await.remove(&session_id);
                }
                if let Err(err) = self
                    .db
                    .update_session_status(
                        &session_id,
                        status.as_str(),
                        phone_number.as_deref(),
                        jid.as_deref(),
                        reason.as_deref(),
                    )
                    .await
                {
                    error!(session_id = %session_id, error = %err, "failed to persist session status");
                }
                let _ = self
                    .events
                    .publish(
                        "session.status",
                        &session_id,
                        serde_json::json!({
                            "session_id": session_id,
                            "status": status.as_str(),
                            "phone_number": phone_number,
                            "jid": jid,
                            "reason": reason,
                        }),
                    )
                    .await;
                if status == SessionConnectionState::LoggedOut
                    || status == SessionConnectionState::Disconnected
                {
                    let _ = self
                        .events
                        .publish(
                            "session.disconnected",
                            &session_id,
                            serde_json::json!({
                                "session_id": session_id,
                                "status": status.as_str(),
                                "reason": reason,
                                "phone_number": phone_number,
                                "jid": jid,
                                "requires_reauth": status == SessionConnectionState::LoggedOut,
                            }),
                        )
                        .await;
                } else if status == SessionConnectionState::Connected {
                    {
                        let mut seen = self.seen_connected.lock().await;
                        if !seen.insert(session_id.clone()) {
                            self.reconnect_count
                                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                    let _ = self.migrate_legacy_lid_history(&session_id).await;
                    let _ = self.db.backfill_contacts_from_messages(&session_id).await;
                    if self.db.count_named_contacts(&session_id).await.unwrap_or(0) == 0 {
                        let engine = Arc::clone(&self.engine);
                        let sid_clone = session_id.clone();
                        tokio::spawn(async move {
                            tokio::time::sleep(Duration::from_secs(3)).await;
                            if let Err(err) = engine.sync_contacts(&sid_clone).await {
                                warn!(session_id = %sid_clone, error = %err, "background full contact sync failed");
                            }
                        });
                    }
                }
            }
            EngineEvent::MessageReceived {
                session_id,
                mut message,
            } => {
                // Deduplicate device-synced echoes of messages already sent via Relay API/MCP
                if message.from_me
                    && self
                        .db
                        .has_message_by_external_id(&session_id, &message.external_id)
                        .await
                        .unwrap_or(false)
                {
                    return;
                }

                // Auto-subscribe to presence for active 1:1 chats so WhatsApp delivers chat.presence events.
                if !message.is_group && !message.is_history_sync {
                    self.maybe_auto_subscribe_presence(&session_id, &message.chat_jid)
                        .await;
                }

                // Upsert contact metadata (push_name / @lid / phone_number) and enrich sender_name with saved address-book full_name.
                if !message.from_me && !message.chat_jid.ends_with("@newsletter") {
                    let contact_jid = if message.is_group {
                        message
                            .sender_phone
                            .as_ref()
                            .map(|p| format!("{p}@s.whatsapp.net"))
                            .or_else(|| message.sender_jid.clone())
                    } else {
                        Some(message.chat_jid.clone())
                    };
                    if let Some(ref cjid) = contact_jid
                        && !cjid.ends_with("@g.us")
                        && cjid != "status@broadcast"
                        && let Ok(saved) = self
                            .db
                            .upsert_contact(crate::storage::database::contacts::UpsertContact {
                                session_id: &session_id,
                                jid: cjid,
                                lid: message.chat_lid.as_deref(),
                                phone_number: message.sender_phone.as_deref(),
                                full_name: None,
                                first_name: None,
                                push_name: message.sender_name.as_deref(),
                            })
                            .await
                        && let Some(full) = saved.full_name
                    {
                        message.sender_name = Some(full);
                    }
                }

                // Fallback to SQLite when WhatsApp omits quoted_message content on replies.
                if let Some(ref mut q) = message.quoted
                    && q.text.as_deref().map(str::trim).unwrap_or("").is_empty()
                    && let Some(ref stanza_id) = q.stanza_id
                    && let Ok(Some(found)) = self
                        .db
                        .find_message_summary_by_external_id(&session_id, stanza_id)
                        .await
                {
                    q.text = Some(found);
                }

                let mut media_id_opt = None;
                let mut is_audio = false;

                if let Some(media) = message.media {
                    is_audio = media.kind == MediaKind::Audio;
                    let media_id = format!("med_{}", uuid::Uuid::now_v7());
                    let ext = extension_from_mime(&media.mime_type);
                    let key = build_media_object_key(&session_id, &media_id, ext);
                    match self.media.put(&key, media.bytes).await {
                        Ok(stored) => {
                            if let Err(err) = self
                                .db
                                .insert_media_object(NewMediaObject {
                                    id: &media_id,
                                    session_id: &session_id,
                                    storage_backend: stored.backend,
                                    object_key: &stored.object_key,
                                    mime_type: &media.mime_type,
                                    size_bytes: stored.size_bytes,
                                    sha256: &stored.sha256,
                                    filename: media.filename.as_deref(),
                                })
                                .await
                            {
                                error!(session_id = %session_id, error = %err, "failed to save inbound media metadata");
                            } else {
                                media_id_opt = Some(media_id);
                            }
                        }
                        Err(err) => {
                            error!(session_id = %session_id, error = %err, "failed to store inbound media");
                        }
                    }
                }

                let direction = if message.from_me {
                    "outbound"
                } else {
                    "inbound"
                };
                let status = if message.from_me { "sent" } else { "received" };
                let source = if message.from_me {
                    "device_sync"
                } else {
                    "contact"
                };
                let msg_ts = message.timestamp.to_rfc3339();
                let metadata = serde_json::json!({
                    "chat_lid": message.chat_lid,
                    "sender_name": message.sender_name,
                    "sender_phone": message.sender_phone,
                    "from_me": message.from_me,
                    "is_group": message.is_group,
                    "quoted": message.quoted,
                    "source": source,
                    "message_timestamp": msg_ts,
                    "is_history_sync": message.is_history_sync,
                });
                let metadata_str = metadata.to_string();

                let msg_id = format!("msg_{}", uuid::Uuid::now_v7());
                let saved = self
                    .db
                    .insert_message(NewMessage {
                        id: &msg_id,
                        session_id: &session_id,
                        external_id: Some(&message.external_id),
                        direction,
                        chat_jid: &message.chat_jid,
                        sender_jid: message.sender_jid.as_deref(),
                        message_type: &message.message_type,
                        text_body: message.text_body.as_deref(),
                        caption: message.caption.as_deref(),
                        media_id: media_id_opt.as_deref(),
                        status,
                        metadata_json: &metadata_str,
                        created_at: Some(&msg_ts),
                    })
                    .await;

                match saved {
                    Ok(rec) => {
                        if !message.is_history_sync {
                            let mut inline_transcription: Option<String> = None;
                            let should_transcribe = !message.from_me
                                && is_audio
                                && self.config.transcription.enabled
                                && media_id_opt.is_some();
                            if should_transcribe
                                && let Err(err) = self.request_transcription(&rec.id, None).await
                            {
                                warn!(message_id = %rec.id, error = %err, "failed to enqueue voice transcription");
                            } else if should_transcribe
                                && self.config.transcription.inline_wait_seconds > 0
                            {
                                let deadline = tokio::time::Instant::now()
                                    + std::time::Duration::from_secs(
                                        self.config.transcription.inline_wait_seconds.min(30),
                                    );
                                while tokio::time::Instant::now() < deadline {
                                    if let Ok(Some(tx)) = self.db.get_transcription(&rec.id).await
                                        && (tx.status == "completed" || tx.status == "failed")
                                    {
                                        inline_transcription = tx.text;
                                        break;
                                    }
                                    tokio::time::sleep(std::time::Duration::from_millis(350)).await;
                                }
                            }

                            let event_type = if message.from_me {
                                "message.sent"
                            } else {
                                "message.received"
                            };
                            let payload = serde_json::json!({
                                "message_id": rec.id,
                                "external_id": rec.external_id,
                                "direction": rec.direction,
                                "chat_jid": rec.chat_jid,
                                "chat_lid": message.chat_lid,
                                "sender_jid": rec.sender_jid,
                                "sender_name": message.sender_name,
                                "sender_phone": message.sender_phone,
                                "from_me": message.from_me,
                                "is_group": message.is_group,
                                "quoted": message.quoted,
                                "source": source,
                                "timestamp": msg_ts,
                                "is_history_sync": message.is_history_sync,
                                "message_type": rec.message_type,
                                "text": rec.text_body,
                                "caption": rec.caption,
                                "media_id": rec.media_id,
                                "transcription": inline_transcription,
                            });

                            let coalesce_secs = self.config.webhooks.coalesce_seconds;
                            if event_type == "message.received" && coalesce_secs > 0 {
                                let key = (session_id.clone(), rec.chat_jid.clone());
                                let should_spawn = {
                                    let mut map = self.coalesce_buffers.lock().await;
                                    let entry = map.entry(key.clone()).or_default();
                                    let first = entry.is_empty();
                                    entry.push(payload);
                                    first
                                };
                                if should_spawn {
                                    let buffers = Arc::clone(&self.coalesce_buffers);
                                    let events = self.events.clone();
                                    let sid_clone = session_id.clone();
                                    tokio::spawn(async move {
                                        tokio::time::sleep(Duration::from_secs(coalesce_secs))
                                            .await;
                                        let items = {
                                            let mut map = buffers.lock().await;
                                            map.remove(&key).unwrap_or_default()
                                        };
                                        if items.is_empty() {
                                            return;
                                        }
                                        let out = if items.len() == 1 {
                                            items.into_iter().next().unwrap()
                                        } else {
                                            let combined_parts: Vec<String> = items
                                                .iter()
                                                .filter_map(|m| {
                                                    m.get("text")
                                                        .and_then(|v| v.as_str())
                                                        .or_else(|| {
                                                            m.get("caption")
                                                                .and_then(|v| v.as_str())
                                                        })
                                                        .or_else(|| {
                                                            m.get("transcription")
                                                                .and_then(|v| v.as_str())
                                                        })
                                                        .map(str::trim)
                                                        .filter(|s| !s.is_empty())
                                                        .map(ToString::to_string)
                                                })
                                                .collect();
                                            let combined_text = combined_parts.join("\n");
                                            let mut base = items.last().cloned().unwrap();
                                            if let Some(obj) = base.as_object_mut() {
                                                obj.insert(
                                                    "coalesced".to_string(),
                                                    serde_json::json!(true),
                                                );
                                                obj.insert(
                                                    "coalesced_count".to_string(),
                                                    serde_json::json!(items.len()),
                                                );
                                                if !combined_text.is_empty() {
                                                    obj.insert(
                                                        "combined_text".to_string(),
                                                        serde_json::json!(combined_text),
                                                    );
                                                    obj.insert(
                                                        "text".to_string(),
                                                        serde_json::json!(combined_text),
                                                    );
                                                }
                                                obj.insert(
                                                    "messages".to_string(),
                                                    serde_json::Value::Array(items),
                                                );
                                            }
                                            base
                                        };
                                        let _ = events
                                            .publish("message.received", &sid_clone, out)
                                            .await;
                                    });
                                }
                            } else {
                                let _ = self.events.publish(event_type, &session_id, payload).await;
                            }
                        }
                    }
                    Err(err) => {
                        error!(session_id = %session_id, error = %err, "failed to persist whatsapp message");
                    }
                }
            }
            EngineEvent::MessageAck {
                session_id,
                external_id,
                chat_jid,
                ack_status,
            } => {
                let message_id = self
                    .db
                    .update_message_status_by_external_id(&session_id, &external_id, &ack_status)
                    .await
                    .ok()
                    .flatten();

                let _ = self
                    .events
                    .publish(
                        "message.ack",
                        &session_id,
                        serde_json::json!({
                            "message_id": message_id,
                            "external_id": external_id,
                            "chat_jid": chat_jid,
                            "status": ack_status,
                        }),
                    )
                    .await;
            }
            EngineEvent::GroupUpdated {
                session_id,
                group_jid,
                action,
                details,
            } => {
                let _ = self
                    .events
                    .publish(
                        "group.updated",
                        &session_id,
                        serde_json::json!({
                            "group_jid": group_jid,
                            "action": action,
                            "details": details,
                        }),
                    )
                    .await;
            }
            EngineEvent::PresenceUpdated {
                session_id,
                chat_jid,
                sender_jid,
                state,
                last_seen,
            } => {
                let _ = self
                    .events
                    .publish(
                        "chat.presence",
                        &session_id,
                        serde_json::json!({
                            "chat_jid": chat_jid,
                            "sender_jid": sender_jid,
                            "state": state,
                            "last_seen": last_seen.map(|t| t.to_rfc3339()),
                        }),
                    )
                    .await;
            }
            EngineEvent::MessageEdited {
                session_id,
                chat_jid,
                external_id,
                new_text,
                timestamp,
            } => {
                let message_id = self
                    .db
                    .update_message_text_by_external_id(&session_id, &external_id, &new_text)
                    .await
                    .ok()
                    .flatten();
                let _ = self
                    .events
                    .publish(
                        "message.edited",
                        &session_id,
                        serde_json::json!({
                            "message_id": message_id,
                            "external_id": external_id,
                            "chat_jid": chat_jid,
                            "text": new_text,
                            "timestamp": timestamp.to_rfc3339(),
                        }),
                    )
                    .await;
            }
            EngineEvent::MessageRevoked {
                session_id,
                chat_jid,
                external_id,
                sender_jid,
                timestamp,
            } => {
                let message_id = self
                    .db
                    .mark_message_revoked_by_external_id(&session_id, &external_id)
                    .await
                    .ok()
                    .flatten();
                let _ = self
                    .events
                    .publish(
                        "message.revoked",
                        &session_id,
                        serde_json::json!({
                            "message_id": message_id,
                            "external_id": external_id,
                            "chat_jid": chat_jid,
                            "sender_jid": sender_jid,
                            "timestamp": timestamp.to_rfc3339(),
                        }),
                    )
                    .await;
            }
            EngineEvent::ContactSynced {
                session_id,
                jid,
                lid,
                phone_number,
                full_name,
                first_name,
                push_name,
            } => {
                if let Err(err) = self
                    .db
                    .upsert_contact(crate::storage::database::contacts::UpsertContact {
                        session_id: &session_id,
                        jid: &jid,
                        lid: lid.as_deref(),
                        phone_number: phone_number.as_deref(),
                        full_name: full_name.as_deref(),
                        first_name: first_name.as_deref(),
                        push_name: push_name.as_deref(),
                    })
                    .await
                {
                    warn!(session_id = %session_id, jid = %jid, error = %err, "failed to upsert synced contact");
                }
            }
        }
    }
}

fn normalize_outbound_chat_jid(to: &str) -> String {
    let trimmed = to.trim();
    if trimmed.contains('@') {
        return trimmed.to_string();
    }
    let digits: String = trimmed.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() >= 7 {
        format!("{digits}@s.whatsapp.net")
    } else {
        trimmed.to_string()
    }
}

fn extension_from_mime(mime: &str) -> Option<&'static str> {
    let base = mime.split(';').next().unwrap_or(mime).trim();
    match base {
        "audio/ogg" => Some("ogg"),
        "audio/mpeg" => Some("mp3"),
        "audio/wav" | "audio/x-wav" => Some("wav"),
        "audio/mp4" => Some("m4a"),
        "image/jpeg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/webp" => Some("webp"),
        "video/mp4" => Some("mp4"),
        "application/pdf" => Some("pdf"),
        _ => None,
    }
}
