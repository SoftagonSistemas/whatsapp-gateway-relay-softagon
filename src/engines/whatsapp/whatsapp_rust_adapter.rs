use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::{Duration, Utc};
use tokio::sync::{RwLock, mpsc};
use tracing::{debug, info, warn};
use whatsapp_rust::download::Downloadable;
use whatsapp_rust::prelude::*;
use whatsapp_rust::upload::UploadOptions;
use whatsapp_rust::wacore::download::MediaType;
use whatsapp_rust::wacore::store::device::DevicePropsOverride;
use whatsapp_rust::wacore::types::events::Event;

use super::{
    EngineError, EngineEvent, InboundMedia, InboundMessage, MediaKind, QrPayload, SendMediaParams,
    SendResult, SendTextParams, SessionConnectionState, WhatsAppEngine,
};

struct ActiveSession {
    handle: BotHandle,
    status: Arc<RwLock<SessionConnectionState>>,
    qr: Arc<RwLock<Option<QrPayload>>>,
}

pub struct WhatsAppRustEngine {
    base_db_path: PathBuf,
    sessions: Arc<RwLock<HashMap<String, ActiveSession>>>,
}

impl WhatsAppRustEngine {
    pub fn new(base_db_path: PathBuf) -> Self {
        Self {
            base_db_path,
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn session_db_path(&self, session_id: &str) -> Result<String, EngineError> {
        let sanitized: String = session_id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        if sanitized.is_empty() {
            return Err(EngineError::Protocol("invalid session_id".into()));
        }

        let dir = self
            .base_db_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(|p| p.join("whatsapp-sessions"))
            .unwrap_or_else(|| PathBuf::from("whatsapp-sessions"));
        std::fs::create_dir_all(&dir)
            .map_err(|e| EngineError::Protocol(format!("failed to create session dir: {e}")))?;

        let db_file = dir.join(format!("{sanitized}.db"));
        Ok(db_file.to_string_lossy().to_string())
    }
}

fn parse_recipient_jid(to: &str) -> Result<Jid, EngineError> {
    let trimmed = to.trim();
    if trimmed.is_empty() {
        return Err(EngineError::InvalidRecipient(
            "recipient cannot be empty".into(),
        ));
    }
    let normalized = if trimmed.contains('@') {
        trimmed.to_string()
    } else {
        let digits: String = trimmed.chars().filter(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            return Err(EngineError::InvalidRecipient(format!(
                "invalid phone number or JID: {trimmed}"
            )));
        }
        format!("{digits}@s.whatsapp.net")
    };
    Jid::from_str(&normalized)
        .map_err(|e| EngineError::InvalidRecipient(format!("invalid JID '{normalized}': {e}")))
}

/// Resolves the exact WhatsApp JID for a phone number, handling Brazilian 9th-digit variations
/// (e.g., `5587992005656` vs `558792005656`) via `client.contacts().is_on_whatsapp(...)`.
async fn resolve_recipient_jid(
    client: &Arc<whatsapp_rust::Client>,
    to: &str,
) -> Result<Jid, EngineError> {
    let primary = parse_recipient_jid(to)?;
    if to.contains('@') || !primary.is_pn() {
        return Ok(primary);
    }

    let digits = primary.user.to_string();
    let mut candidates = vec![primary.clone()];

    // Brazilian mobile numbers: 55 + 2-digit DDD + optional 9 + 8 digits
    if digits.starts_with("55") && digits.len() == 13 && digits.as_bytes()[4] == b'9' {
        let without_nine = format!("{}{}", &digits[..4], &digits[5..]);
        if let Ok(alt) = Jid::from_str(&format!("{without_nine}@s.whatsapp.net")) {
            candidates.push(alt);
        }
    } else if digits.starts_with("55") && digits.len() == 12 {
        let with_nine = format!("{}9{}", &digits[..4], &digits[4..]);
        if let Ok(alt) = Jid::from_str(&format!("{with_nine}@s.whatsapp.net")) {
            candidates.push(alt);
        }
    } else if digits.starts_with("521") && digits.len() == 13 {
        // Mexico mobile variation (521 vs 52)
        let alt_num = format!("52{}", &digits[3..]);
        if let Ok(alt) = Jid::from_str(&format!("{alt_num}@s.whatsapp.net")) {
            candidates.push(alt);
        }
    } else if digits.starts_with("52") && digits.len() == 12 {
        let alt_num = format!("521{}", &digits[2..]);
        if let Ok(alt) = Jid::from_str(&format!("{alt_num}@s.whatsapp.net")) {
            candidates.push(alt);
        }
    } else if digits.starts_with("549") && digits.len() == 13 {
        // Argentina mobile variation (549 vs 54)
        let alt_num = format!("54{}", &digits[3..]);
        if let Ok(alt) = Jid::from_str(&format!("{alt_num}@s.whatsapp.net")) {
            candidates.push(alt);
        }
    } else if digits.starts_with("54") && digits.len() == 12 {
        let alt_num = format!("549{}", &digits[2..]);
        if let Ok(alt) = Jid::from_str(&format!("{alt_num}@s.whatsapp.net")) {
            candidates.push(alt);
        }
    }

    match client.contacts().is_on_whatsapp(&candidates).await {
        Ok(results) => {
            if let Some(found) = results.into_iter().find(|r| r.is_registered) {
                let resolved = found.pn_jid.unwrap_or(found.jid);
                debug!(requested = %primary, resolved = %resolved, "resolved recipient JID");
                return Ok(resolved);
            }
        }
        Err(err) => {
            warn!(error = %err, "is_on_whatsapp lookup failed; falling back to primary JID");
        }
    }

    Ok(primary)
}

#[async_trait]
impl WhatsAppEngine for WhatsAppRustEngine {
    async fn start_session(
        &self,
        session_id: &str,
        event_tx: mpsc::Sender<EngineEvent>,
    ) -> Result<(), EngineError> {
        {
            let mut guard = self.sessions.write().await;
            if let Some(existing) = guard.get(session_id) {
                let st = *existing.status.read().await;
                if st == SessionConnectionState::Connected
                    || st == SessionConnectionState::PairingRequired
                    || st == SessionConnectionState::Connecting
                {
                    return Ok(());
                }
            }
            if let Some(old) = guard.remove(session_id) {
                old.handle.shutdown().await;
            }
        }

        let db_path = self.session_db_path(session_id)?;
        let store = SqliteStore::new(&db_path).await.map_err(|e| {
            EngineError::Protocol(format!("failed to open whatsapp sqlite store: {e}"))
        })?;

        let status_cell = Arc::new(RwLock::new(SessionConnectionState::Connecting));
        let qr_cell = Arc::new(RwLock::new(None::<QrPayload>));

        let sid_ev = session_id.to_string();
        let tx_ev = event_tx.clone();
        let status_ev = status_cell.clone();
        let qr_ev = qr_cell.clone();

        let bot = Bot::builder()
            .with_backend(store)
            .with_device_props(
                DevicePropsOverride::new()
                    .with_os("Softagon Relay")
                    .with_platform_type(wa::device_props::PlatformType::CHROME),
            )
            .on_event(move |event, client| {
                let sid = sid_ev.clone();
                let tx = tx_ev.clone();
                let status = status_ev.clone();
                let qr = qr_ev.clone();
                async move {
                    handle_wa_event(&sid, event, client, &tx, &status, &qr).await;
                }
            })
            .build()
            .await
            .map_err(|e| EngineError::Protocol(format!("failed to build whatsapp bot: {e}")))?;

        let handle = bot.spawn();
        info!(session_id = %session_id, "started whatsapp-rust session");

        let mut guard = self.sessions.write().await;
        guard.insert(
            session_id.to_string(),
            ActiveSession {
                handle,
                status: status_cell,
                qr: qr_cell,
            },
        );

        Ok(())
    }

    async fn stop_session(&self, session_id: &str) -> Result<(), EngineError> {
        let removed = {
            let mut guard = self.sessions.write().await;
            guard.remove(session_id)
        };
        if let Some(active) = removed {
            *active.status.write().await = SessionConnectionState::Disconnected;
            active.handle.shutdown().await;
        }
        Ok(())
    }

    async fn logout_session(&self, session_id: &str) -> Result<(), EngineError> {
        let removed = {
            let mut guard = self.sessions.write().await;
            guard.remove(session_id)
        };
        let Some(active) = removed else {
            return Err(EngineError::SessionNotFound(session_id.to_string()));
        };
        let client = active.handle.client();
        client.logout().await;
        *active.status.write().await = SessionConnectionState::LoggedOut;
        *active.qr.write().await = None;
        active.handle.shutdown().await;
        Ok(())
    }

    async fn delete_session_data(&self, session_id: &str) -> Result<(), EngineError> {
        let db_path = self.session_db_path(session_id)?;
        for suffix in ["", "-wal", "-shm"] {
            let p = format!("{db_path}{suffix}");
            let _ = std::fs::remove_file(p);
        }
        Ok(())
    }

    async fn get_status(&self, session_id: &str) -> Result<SessionConnectionState, EngineError> {
        let guard = self.sessions.read().await;
        let Some(active) = guard.get(session_id) else {
            return Err(EngineError::SessionNotFound(session_id.to_string()));
        };
        let status = *active.status.read().await;
        Ok(status)
    }

    async fn get_qr(&self, session_id: &str) -> Result<Option<QrPayload>, EngineError> {
        let guard = self.sessions.read().await;
        let Some(active) = guard.get(session_id) else {
            return Err(EngineError::SessionNotFound(session_id.to_string()));
        };
        let qr = active.qr.read().await.clone();
        Ok(qr)
    }

    async fn send_text(
        &self,
        session_id: &str,
        params: SendTextParams,
    ) -> Result<SendResult, EngineError> {
        let client = {
            let guard = self.sessions.read().await;
            let Some(active) = guard.get(session_id) else {
                return Err(EngineError::SessionNotFound(session_id.to_string()));
            };
            if *active.status.read().await != SessionConnectionState::Connected {
                return Err(EngineError::NotConnected(session_id.to_string()));
            }
            active.handle.client()
        };

        let to = resolve_recipient_jid(&client, &params.to).await?;
        let msg = wa::Message {
            conversation: Some(params.text),
            ..Default::default()
        };
        let res = client
            .send_message(to, msg)
            .await
            .map_err(|e| EngineError::Protocol(format!("send_message failed: {e}")))?;

        Ok(SendResult {
            external_message_id: res.message_id,
        })
    }

    async fn send_media(
        &self,
        session_id: &str,
        params: SendMediaParams,
    ) -> Result<SendResult, EngineError> {
        let client = {
            let guard = self.sessions.read().await;
            let Some(active) = guard.get(session_id) else {
                return Err(EngineError::SessionNotFound(session_id.to_string()));
            };
            if *active.status.read().await != SessionConnectionState::Connected {
                return Err(EngineError::NotConnected(session_id.to_string()));
            }
            active.handle.client()
        };

        let to = resolve_recipient_jid(&client, &params.to).await?;
        let media_type = match params.kind {
            MediaKind::Image => MediaType::Image,
            MediaKind::Video => MediaType::Video,
            MediaKind::Audio => MediaType::Audio,
            MediaKind::Document => MediaType::Document,
        };

        let upload = client
            .upload(params.data.to_vec(), media_type, UploadOptions::default())
            .await
            .map_err(|e| EngineError::Media(format!("media upload failed: {e}")))?;

        let msg = match params.kind {
            MediaKind::Image => wa::Message {
                image_message: wa::message::ImageMessage {
                    mimetype: Some(params.mime_type),
                    caption: params.caption,
                    url: Some(upload.url),
                    direct_path: Some(upload.direct_path),
                    media_key: Some(upload.media_key.to_vec()),
                    file_enc_sha256: Some(upload.file_enc_sha256.to_vec()),
                    file_sha256: Some(upload.file_sha256.to_vec()),
                    file_length: Some(upload.file_length),
                    ..Default::default()
                }
                .into(),
                ..Default::default()
            },
            MediaKind::Video => wa::Message {
                video_message: wa::message::VideoMessage {
                    mimetype: Some(params.mime_type),
                    caption: params.caption,
                    url: Some(upload.url),
                    direct_path: Some(upload.direct_path),
                    media_key: Some(upload.media_key.to_vec()),
                    file_enc_sha256: Some(upload.file_enc_sha256.to_vec()),
                    file_sha256: Some(upload.file_sha256.to_vec()),
                    file_length: Some(upload.file_length),
                    ..Default::default()
                }
                .into(),
                ..Default::default()
            },
            MediaKind::Audio => wa::Message {
                audio_message: wa::message::AudioMessage {
                    mimetype: Some(params.mime_type),
                    url: Some(upload.url),
                    direct_path: Some(upload.direct_path),
                    media_key: Some(upload.media_key.to_vec()),
                    file_enc_sha256: Some(upload.file_enc_sha256.to_vec()),
                    file_sha256: Some(upload.file_sha256.to_vec()),
                    file_length: Some(upload.file_length),
                    ptt: Some(false),
                    ..Default::default()
                }
                .into(),
                ..Default::default()
            },
            MediaKind::Document => wa::Message {
                document_message: wa::message::DocumentMessage {
                    mimetype: Some(params.mime_type),
                    file_name: params.filename,
                    caption: params.caption,
                    url: Some(upload.url),
                    direct_path: Some(upload.direct_path),
                    media_key: Some(upload.media_key.to_vec()),
                    file_enc_sha256: Some(upload.file_enc_sha256.to_vec()),
                    file_sha256: Some(upload.file_sha256.to_vec()),
                    file_length: Some(upload.file_length),
                    ..Default::default()
                }
                .into(),
                ..Default::default()
            },
        };

        let res = client
            .send_message(to, msg)
            .await
            .map_err(|e| EngineError::Protocol(format!("send media message failed: {e}")))?;

        Ok(SendResult {
            external_message_id: res.message_id,
        })
    }
}

async fn handle_wa_event(
    session_id: &str,
    event: Arc<Event>,
    client: Arc<whatsapp_rust::Client>,
    tx: &mpsc::Sender<EngineEvent>,
    status_cell: &Arc<RwLock<SessionConnectionState>>,
    qr_cell: &Arc<RwLock<Option<QrPayload>>>,
) {
    match event.as_ref() {
        Event::PairingQrCode(qr) => {
            let timeout_secs = qr.timeout.as_secs().max(20) as i64;
            let payload = QrPayload {
                code: qr.code.clone(),
                expires_at: Utc::now() + Duration::seconds(timeout_secs),
            };
            *qr_cell.write().await = Some(payload.clone());
            *status_cell.write().await = SessionConnectionState::PairingRequired;
            let _ = tx
                .send(EngineEvent::QrUpdated {
                    session_id: session_id.to_string(),
                    qr: payload,
                })
                .await;
            let _ = tx
                .send(EngineEvent::StatusChanged {
                    session_id: session_id.to_string(),
                    status: SessionConnectionState::PairingRequired,
                    phone_number: None,
                    jid: None,
                    reason: None,
                })
                .await;
        }
        Event::Connected(_) => {
            *status_cell.write().await = SessionConnectionState::Connected;
            *qr_cell.write().await = None;
            let _ = tx
                .send(EngineEvent::StatusChanged {
                    session_id: session_id.to_string(),
                    status: SessionConnectionState::Connected,
                    phone_number: None,
                    jid: None,
                    reason: None,
                })
                .await;
        }
        Event::PairSuccess(pair) => {
            *status_cell.write().await = SessionConnectionState::Connected;
            *qr_cell.write().await = None;
            let jid_str = pair.id.to_string();
            let phone = pair.id.user.to_string();
            let _ = tx
                .send(EngineEvent::StatusChanged {
                    session_id: session_id.to_string(),
                    status: SessionConnectionState::Connected,
                    phone_number: Some(phone),
                    jid: Some(jid_str),
                    reason: None,
                })
                .await;
        }
        Event::PairingQrCodesExhausted(_) => {
            *status_cell.write().await = SessionConnectionState::Disconnected;
            *qr_cell.write().await = None;
            let _ = tx
                .send(EngineEvent::StatusChanged {
                    session_id: session_id.to_string(),
                    status: SessionConnectionState::Disconnected,
                    phone_number: None,
                    jid: None,
                    reason: Some("qr_expired".into()),
                })
                .await;
        }
        Event::Disconnected(_) => {
            *status_cell.write().await = SessionConnectionState::Disconnected;
            let _ = tx
                .send(EngineEvent::StatusChanged {
                    session_id: session_id.to_string(),
                    status: SessionConnectionState::Disconnected,
                    phone_number: None,
                    jid: None,
                    reason: Some("disconnected".into()),
                })
                .await;
        }
        Event::LoggedOut(lo) => {
            *status_cell.write().await = SessionConnectionState::LoggedOut;
            *qr_cell.write().await = None;
            let _ = tx
                .send(EngineEvent::StatusChanged {
                    session_id: session_id.to_string(),
                    status: SessionConnectionState::LoggedOut,
                    phone_number: None,
                    jid: None,
                    reason: Some(format!("{:?}", lo.reason)),
                })
                .await;
        }
        Event::Messages(batch) => {
            for item in batch.iter() {
                if let Some(inbound) =
                    convert_inbound_message(item.message.as_ref(), &item.info, &client).await
                {
                    let _ = tx
                        .send(EngineEvent::MessageReceived {
                            session_id: session_id.to_string(),
                            message: inbound,
                        })
                        .await;
                }
            }
        }
        Event::Receipt(receipt) => {
            let ack_status = format!("{:?}", receipt.r#type).to_ascii_lowercase();
            let chat_jid = receipt.source.chat.to_string();
            for msg_id in &receipt.message_ids {
                let _ = tx
                    .send(EngineEvent::MessageAck {
                        session_id: session_id.to_string(),
                        external_id: msg_id.clone(),
                        chat_jid: chat_jid.clone(),
                        ack_status: ack_status.clone(),
                    })
                    .await;
            }
        }
        Event::GroupUpdate(update) => {
            let _ = tx
                .send(EngineEvent::GroupUpdated {
                    session_id: session_id.to_string(),
                    group_jid: update.group_jid.to_string(),
                    action: "updated".to_string(),
                    details: serde_json::json!({
                        "group_jid": update.group_jid.to_string(),
                    }),
                })
                .await;
        }
        other => {
            debug!(session_id = %session_id, event_kind = ?other.kind(), "ignoring unhandled whatsapp event");
        }
    }
}

async fn convert_inbound_message(
    msg: &wa::Message,
    info: &whatsapp_rust::wacore::types::message::MessageInfo,
    client: &Arc<whatsapp_rust::Client>,
) -> Option<InboundMessage> {
    let external_id = info.id.clone();
    let chat_jid = info.source.chat.to_string();
    let sender_jid = Some(info.source.sender.to_string());
    let timestamp = Utc::now();

    if let Some(audio) = msg.audio_message.as_option() {
        let bytes = download_media_bytes(client, audio).await?;
        let mime = audio
            .mimetype
            .clone()
            .unwrap_or_else(|| "audio/ogg; codecs=opus".to_string());
        let is_voice_note = audio.ptt.unwrap_or(false) || mime.contains("ogg");
        return Some(InboundMessage {
            external_id,
            chat_jid,
            sender_jid,
            message_type: "audio".to_string(),
            text_body: None,
            caption: None,
            media: Some(InboundMedia {
                kind: MediaKind::Audio,
                mime_type: mime,
                filename: None,
                bytes,
                is_voice_note,
            }),
            timestamp,
        });
    }

    if let Some(image) = msg.image_message.as_option() {
        let bytes = download_media_bytes(client, image).await?;
        return Some(InboundMessage {
            external_id,
            chat_jid,
            sender_jid,
            message_type: "image".to_string(),
            text_body: None,
            caption: image.caption.clone(),
            media: Some(InboundMedia {
                kind: MediaKind::Image,
                mime_type: image
                    .mimetype
                    .clone()
                    .unwrap_or_else(|| "image/jpeg".to_string()),
                filename: None,
                bytes,
                is_voice_note: false,
            }),
            timestamp,
        });
    }

    if let Some(video) = msg.video_message.as_option() {
        let bytes = download_media_bytes(client, video).await?;
        return Some(InboundMessage {
            external_id,
            chat_jid,
            sender_jid,
            message_type: "video".to_string(),
            text_body: None,
            caption: video.caption.clone(),
            media: Some(InboundMedia {
                kind: MediaKind::Video,
                mime_type: video
                    .mimetype
                    .clone()
                    .unwrap_or_else(|| "video/mp4".to_string()),
                filename: None,
                bytes,
                is_voice_note: false,
            }),
            timestamp,
        });
    }

    if let Some(doc) = msg.document_message.as_option() {
        let bytes = download_media_bytes(client, doc).await?;
        return Some(InboundMessage {
            external_id,
            chat_jid,
            sender_jid,
            message_type: "document".to_string(),
            text_body: None,
            caption: doc.caption.clone(),
            media: Some(InboundMedia {
                kind: MediaKind::Document,
                mime_type: doc
                    .mimetype
                    .clone()
                    .unwrap_or_else(|| "application/octet-stream".to_string()),
                filename: doc.file_name.clone(),
                bytes,
                is_voice_note: false,
            }),
            timestamp,
        });
    }

    let text_body = msg.conversation.clone().or_else(|| {
        msg.extended_text_message
            .as_option()
            .and_then(|e| e.text.clone())
    });

    text_body.map(|text| InboundMessage {
        external_id,
        chat_jid,
        sender_jid,
        message_type: "text".to_string(),
        text_body: Some(text),
        caption: None,
        media: None,
        timestamp,
    })
}

async fn download_media_bytes(
    client: &Arc<whatsapp_rust::Client>,
    downloadable: &dyn Downloadable,
) -> Option<Bytes> {
    match client.download(downloadable).await {
        Ok(data) => Some(Bytes::from(data)),
        Err(err) => {
            warn!(error = %err, "failed to download inbound media from whatsapp");
            None
        }
    }
}
