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
        });
        (manager, event_rx)
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
        Ok(self.db.list_sessions().await?)
    }

    pub async fn get_session(&self, session_id: &str) -> Result<SessionRecord, SessionError> {
        self.db
            .get_session(session_id)
            .await?
            .ok_or_else(|| SessionError::NotFound(session_id.to_string()))
    }

    pub async fn get_qr(&self, session_id: &str) -> Result<Option<QrPayload>, SessionError> {
        let _ = self.get_session(session_id).await?;
        if let Ok(Some(qr)) = self.engine.get_qr(session_id).await {
            return Ok(Some(qr));
        }
        Ok(self.qr_cache.read().await.get(session_id).cloned())
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
        let rec = self.get_session(session_id).await?;
        let _ = self.engine.stop_session(session_id).await;
        self.qr_cache.write().await.remove(session_id);
        self.db
            .update_session_status(
                session_id,
                SessionConnectionState::Connecting.as_str(),
                None,
                None,
            )
            .await?;
        self.engine
            .start_session(session_id, self.event_tx.clone())
            .await?;
        self.get_session(&rec.id).await
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
        let _ = self.get_session(session_id).await?;
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
                },
            )
            .await?;

        let msg_id = format!("msg_{}", uuid::Uuid::now_v7());
        let record = self
            .db
            .insert_message(NewMessage {
                id: &msg_id,
                session_id,
                external_id: Some(&res.external_message_id),
                direction: "outbound",
                chat_jid: to,
                sender_jid: None,
                message_type: "text",
                text_body: Some(text),
                caption: None,
                media_id: None,
                status: "sent",
                metadata_json: "{}",
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
        let _ = self.get_session(session_id).await?;
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
                },
            )
            .await?;

        let msg_id = format!("msg_{}", uuid::Uuid::now_v7());
        let record = self
            .db
            .insert_message(NewMessage {
                id: &msg_id,
                session_id,
                external_id: Some(&res.external_message_id),
                direction: "outbound",
                chat_jid: to,
                sender_jid: None,
                message_type: kind.as_str(),
                text_body: None,
                caption,
                media_id: Some(&media_id),
                status: "sent",
                metadata_json: "{}",
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

    pub async fn get_message(&self, message_id: &str) -> Result<MessageRecord, SessionError> {
        self.db
            .get_message(message_id)
            .await?
            .ok_or_else(|| SessionError::MessageNotFound(message_id.to_string()))
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
                            "reason": reason,
                        }),
                    )
                    .await;
            }
            EngineEvent::MessageReceived {
                session_id,
                message,
            } => {
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

                let msg_id = format!("msg_{}", uuid::Uuid::now_v7());
                let saved = self
                    .db
                    .insert_message(NewMessage {
                        id: &msg_id,
                        session_id: &session_id,
                        external_id: Some(&message.external_id),
                        direction: "inbound",
                        chat_jid: &message.chat_jid,
                        sender_jid: message.sender_jid.as_deref(),
                        message_type: &message.message_type,
                        text_body: message.text_body.as_deref(),
                        caption: message.caption.as_deref(),
                        media_id: media_id_opt.as_deref(),
                        status: "received",
                        metadata_json: "{}",
                    })
                    .await;

                match saved {
                    Ok(rec) => {
                        let _ = self
                            .events
                            .publish(
                                "message.received",
                                &session_id,
                                serde_json::json!({
                                    "message_id": rec.id,
                                    "external_id": rec.external_id,
                                    "chat_jid": rec.chat_jid,
                                    "sender_jid": rec.sender_jid,
                                    "message_type": rec.message_type,
                                    "text": rec.text_body,
                                    "caption": rec.caption,
                                    "media_id": rec.media_id,
                                }),
                            )
                            .await;

                        if is_audio
                            && self.config.transcription.enabled
                            && media_id_opt.is_some()
                            && let Err(err) = self.request_transcription(&rec.id, None).await
                        {
                            warn!(message_id = %rec.id, error = %err, "failed to enqueue voice transcription");
                        }
                    }
                    Err(err) => {
                        error!(session_id = %session_id, error = %err, "failed to persist inbound message");
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
        }
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
