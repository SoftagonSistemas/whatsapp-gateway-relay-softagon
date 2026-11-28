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
use whatsapp_rust::pair_code::PairCodeOptions;
use whatsapp_rust::prelude::*;
use whatsapp_rust::upload::UploadOptions;
use whatsapp_rust::wacore::download::MediaType;
use whatsapp_rust::wacore::store::device::DevicePropsOverride;
use whatsapp_rust::wacore::types::events::Event;
use whatsapp_rust::wacore_binary::JidExt;

use super::{
    EngineError, EngineEvent, InboundMedia, InboundMessage, MediaKind, PairCodePayload, QrPayload,
    SendMediaParams, SendResult, SendTextParams, SessionConnectionState, WhatsAppEngine,
    format_pair_code,
};

struct ActiveSession {
    handle: BotHandle,
    status: Arc<RwLock<SessionConnectionState>>,
    qr: Arc<RwLock<Option<QrPayload>>>,
    pair_code: Arc<RwLock<Option<PairCodePayload>>>,
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

    async fn connected_client(
        &self,
        session_id: &str,
    ) -> Result<Arc<whatsapp_rust::Client>, EngineError> {
        let guard = self.sessions.read().await;
        let Some(active) = guard.get(session_id) else {
            return Err(EngineError::SessionNotFound(session_id.to_string()));
        };
        if *active.status.read().await != SessionConnectionState::Connected {
            return Err(EngineError::NotConnected(session_id.to_string()));
        }
        Ok(active.handle.client())
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
/// (e.g., `5511999990001` vs `551199990001`) via `client.contacts().is_on_whatsapp(...)`.
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
        let pair_code_cell = Arc::new(RwLock::new(None::<PairCodePayload>));

        let sid_ev = session_id.to_string();
        let tx_ev = event_tx.clone();
        let status_ev = status_cell.clone();
        let qr_ev = qr_cell.clone();
        let pair_code_ev = pair_code_cell.clone();

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
                let pair_code = pair_code_ev.clone();
                async move {
                    handle_wa_event(&sid, event, client, &tx, &status, &qr, &pair_code).await;
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
                pair_code: pair_code_cell,
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

    async fn request_pair_code(
        &self,
        session_id: &str,
        phone_number: &str,
    ) -> Result<PairCodePayload, EngineError> {
        let (client, pair_code_cell) = {
            let guard = self.sessions.read().await;
            let Some(active) = guard.get(session_id) else {
                return Err(EngineError::SessionNotFound(session_id.to_string()));
            };
            if *active.status.read().await == SessionConnectionState::Connected {
                return Err(EngineError::Protocol("session is already connected".into()));
            }
            if let Some(existing) = active
                .pair_code
                .read()
                .await
                .clone()
                .filter(|p| p.expires_at > Utc::now())
            {
                return Ok(existing);
            }
            (active.handle.client(), active.pair_code.clone())
        };

        let digits: String = phone_number
            .chars()
            .filter(|c| c.is_ascii_digit())
            .collect();
        let code = client
            .pair_with_code(PairCodeOptions {
                phone_number: digits,
                ..Default::default()
            })
            .await
            .map_err(|e| EngineError::Protocol(format!("pair code request failed: {e}")))?;

        let payload = PairCodePayload {
            formatted_code: format_pair_code(&code),
            code,
            expires_at: Utc::now() + Duration::seconds(180),
        };
        *pair_code_cell.write().await = Some(payload.clone());
        Ok(payload)
    }

    async fn get_pair_code(
        &self,
        session_id: &str,
    ) -> Result<Option<PairCodePayload>, EngineError> {
        let guard = self.sessions.read().await;
        let Some(active) = guard.get(session_id) else {
            return Err(EngineError::SessionNotFound(session_id.to_string()));
        };
        let current = active
            .pair_code
            .read()
            .await
            .clone()
            .filter(|p| p.expires_at > Utc::now());
        Ok(current)
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
        let msg = if let Some(reply_id) = params
            .reply_to_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            wa::Message {
                extended_text_message: wa::message::ExtendedTextMessage {
                    text: Some(params.text),
                    context_info: wa::ContextInfo {
                        stanza_id: Some(reply_id.to_string()),
                        ..Default::default()
                    }
                    .into(),
                    ..Default::default()
                }
                .into(),
                ..Default::default()
            }
        } else {
            wa::Message {
                conversation: Some(params.text),
                ..Default::default()
            }
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
            MediaKind::Sticker => MediaType::Sticker,
        };

        let upload = client
            .upload(params.data.to_vec(), media_type, UploadOptions::default())
            .await
            .map_err(|e| EngineError::Media(format!("media upload failed: {e}")))?;

        let ctx_info = params
            .reply_to_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|id| wa::ContextInfo {
                stanza_id: Some(id.to_string()),
                ..Default::default()
            });

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
                    context_info: ctx_info.into(),
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
                    context_info: ctx_info.into(),
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
                    ptt: Some(params.as_voice_note),
                    context_info: ctx_info.into(),
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
                    context_info: ctx_info.into(),
                    ..Default::default()
                }
                .into(),
                ..Default::default()
            },
            MediaKind::Sticker => wa::Message {
                sticker_message: wa::message::StickerMessage {
                    mimetype: Some(params.mime_type),
                    url: Some(upload.url),
                    direct_path: Some(upload.direct_path),
                    media_key: Some(upload.media_key.to_vec()),
                    file_enc_sha256: Some(upload.file_enc_sha256.to_vec()),
                    file_sha256: Some(upload.file_sha256.to_vec()),
                    file_length: Some(upload.file_length),
                    context_info: ctx_info.into(),
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

    async fn mark_read(
        &self,
        session_id: &str,
        chat_jid: &str,
        sender_jid: Option<&str>,
        message_ids: &[String],
    ) -> Result<(), EngineError> {
        if message_ids.is_empty() {
            return Ok(());
        }
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

        let chat = resolve_recipient_jid(&client, chat_jid).await?;
        let sender = match sender_jid.map(str::trim).filter(|s| !s.is_empty()) {
            Some(s) => Some(resolve_recipient_jid(&client, s).await?),
            None => None,
        };
        let id_refs: Vec<&str> = message_ids.iter().map(String::as_str).collect();
        client
            .mark_as_read(&chat, sender.as_ref(), &id_refs)
            .await
            .map_err(|e| EngineError::Protocol(format!("mark_as_read failed: {e}")))?;
        Ok(())
    }

    async fn resolve_contact(
        &self,
        session_id: &str,
        jid_or_phone: &str,
    ) -> Result<crate::engines::whatsapp::ContactInfo, EngineError> {
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

        let trimmed = jid_or_phone.trim();
        let jid = resolve_recipient_jid(&client, trimmed).await?;
        let mut lid: Option<String> = if jid.is_lid() {
            Some(jid.to_string())
        } else {
            None
        };
        let mut phone_number = extract_phone_from_jid(&jid.to_string());

        if let Ok(Some(entry)) = client.get_lid_pn_entry(&jid).await {
            if phone_number.is_none() && !entry.phone_number.is_empty() {
                phone_number = Some(entry.phone_number.to_string());
            }
            if lid.is_none() && !entry.lid.is_empty() {
                lid = Some(format!("{}@lid", entry.lid));
            }
        }

        let canonical_jid = phone_number
            .as_ref()
            .map(|p| format!("{p}@s.whatsapp.net"))
            .unwrap_or_else(|| jid.to_string());

        Ok(crate::engines::whatsapp::ContactInfo {
            query: trimmed.to_string(),
            jid: canonical_jid,
            lid,
            phone_number,
            full_name: None,
            first_name: None,
            push_name: None,
        })
    }

    async fn save_contact(
        &self,
        session_id: &str,
        jid_or_phone: &str,
        full_name: Option<&str>,
        first_name: Option<&str>,
        save_on_primary_addressbook: bool,
    ) -> Result<String, EngineError> {
        let client = self.connected_client(session_id).await?;
        let mut target = resolve_recipient_jid(&client, jid_or_phone).await?;
        if target.is_lid() {
            if let Ok(Some(entry)) = client.get_lid_pn_entry(&target).await
                && !entry.phone_number.is_empty()
            {
                target = Jid::pn(entry.phone_number.to_string());
            } else {
                return Err(EngineError::Protocol(
                    "save_contact requires a phone-number JID; @lid could not be resolved to a phone number".into(),
                ));
            }
        }
        let bare = target.to_non_ad();
        client
            .chat_actions()
            .save_contact(
                &bare,
                full_name.map(ToString::to_string),
                first_name.map(ToString::to_string),
                save_on_primary_addressbook,
            )
            .await
            .map_err(|e| EngineError::Protocol(format!("save_contact failed: {e}")))?;
        Ok(bare.to_string())
    }

    async fn sync_contacts(&self, session_id: &str) -> Result<(), EngineError> {
        let client = self.connected_client(session_id).await?;
        client
            .process_sync_task(whatsapp_rust::sync_task::MajorSyncTask::AppStateSync {
                name:
                    whatsapp_rust::wacore::appstate::patch_decode::WAPatchName::CriticalUnblockLow,
                full_sync: true,
            })
            .await;
        Ok(())
    }

    async fn send_reaction(
        &self,
        session_id: &str,
        params: crate::engines::whatsapp::SendReactionParams,
    ) -> Result<SendResult, EngineError> {
        let client = self.connected_client(session_id).await?;
        let chat = resolve_recipient_jid(&client, &params.chat_jid).await?;
        let target_key = wa::MessageKey {
            remote_jid: Some(chat.to_string()),
            from_me: Some(params.from_me),
            id: Some(params.message_external_id),
            participant: params.participant,
        };
        let res = client
            .send_reaction(chat, target_key, &params.emoji)
            .await
            .map_err(|e| EngineError::Protocol(format!("send_reaction failed: {e}")))?;
        Ok(SendResult {
            external_message_id: res.message_id,
        })
    }

    async fn send_chat_presence(
        &self,
        session_id: &str,
        chat_jid: &str,
        state: crate::engines::whatsapp::ChatPresenceState,
    ) -> Result<(), EngineError> {
        let client = self.connected_client(session_id).await?;
        let chat = resolve_recipient_jid(&client, chat_jid).await?;
        let res = match state {
            crate::engines::whatsapp::ChatPresenceState::Composing => {
                client.chatstate().send_composing(&chat).await
            }
            crate::engines::whatsapp::ChatPresenceState::Recording => {
                client.chatstate().send_recording(&chat).await
            }
            crate::engines::whatsapp::ChatPresenceState::Paused => {
                client.chatstate().send_paused(&chat).await
            }
        };
        res.map_err(|e| EngineError::Protocol(format!("send_chat_presence failed: {e}")))
    }

    async fn send_location(
        &self,
        session_id: &str,
        params: crate::engines::whatsapp::SendLocationParams,
    ) -> Result<SendResult, EngineError> {
        let client = self.connected_client(session_id).await?;
        let to = resolve_recipient_jid(&client, &params.to).await?;
        let msg = wa::Message {
            location_message: wa::message::LocationMessage {
                degrees_latitude: Some(params.latitude),
                degrees_longitude: Some(params.longitude),
                name: params.name,
                address: params.address,
                ..Default::default()
            }
            .into(),
            ..Default::default()
        };
        let res = client
            .send_message(to, msg)
            .await
            .map_err(|e| EngineError::Protocol(format!("send_location failed: {e}")))?;
        Ok(SendResult {
            external_message_id: res.message_id,
        })
    }

    async fn send_contact(
        &self,
        session_id: &str,
        params: crate::engines::whatsapp::SendContactParams,
    ) -> Result<SendResult, EngineError> {
        let client = self.connected_client(session_id).await?;
        let to = resolve_recipient_jid(&client, &params.to).await?;
        let msg = wa::Message {
            contact_message: wa::message::ContactMessage {
                display_name: Some(params.display_name),
                vcard: Some(params.vcard),
                ..Default::default()
            }
            .into(),
            ..Default::default()
        };
        let res = client
            .send_message(to, msg)
            .await
            .map_err(|e| EngineError::Protocol(format!("send_contact failed: {e}")))?;
        Ok(SendResult {
            external_message_id: res.message_id,
        })
    }

    async fn edit_message(
        &self,
        session_id: &str,
        chat_jid: &str,
        external_message_id: &str,
        new_text: &str,
    ) -> Result<String, EngineError> {
        let client = self.connected_client(session_id).await?;
        let to = resolve_recipient_jid(&client, chat_jid).await?;
        let new_msg = wa::Message {
            conversation: Some(new_text.to_string()),
            ..Default::default()
        };
        client
            .edit_message(to, external_message_id, new_msg)
            .await
            .map_err(|e| EngineError::Protocol(format!("edit_message failed: {e}")))
    }

    async fn subscribe_presence(&self, session_id: &str, jid: &str) -> Result<(), EngineError> {
        let client = self.connected_client(session_id).await?;
        let target = resolve_recipient_jid(&client, jid).await?;
        client
            .presence()
            .subscribe(target)
            .await
            .map_err(|e| EngineError::Protocol(format!("subscribe_presence failed: {e}")))
    }

    async fn revoke_message(
        &self,
        session_id: &str,
        chat_jid: &str,
        external_message_id: &str,
    ) -> Result<(), EngineError> {
        let client = self.connected_client(session_id).await?;
        let to = resolve_recipient_jid(&client, chat_jid).await?;
        client
            .revoke_message(
                to,
                external_message_id,
                whatsapp_rust::send::RevokeType::Sender,
            )
            .await
            .map_err(|e| EngineError::Protocol(format!("revoke_message failed: {e}")))
    }

    async fn is_on_whatsapp(
        &self,
        session_id: &str,
        phones: &[String],
    ) -> Result<Vec<crate::engines::whatsapp::WhatsAppCheckResult>, EngineError> {
        let client = self.connected_client(session_id).await?;
        let mut jids = Vec::with_capacity(phones.len());
        for raw in phones {
            let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
            if digits.len() >= 7 {
                jids.push(Jid::pn(&digits));
            }
        }
        let results = client
            .contacts()
            .is_on_whatsapp(&jids)
            .await
            .map_err(|e| EngineError::Protocol(format!("is_on_whatsapp failed: {e}")))?;
        Ok(results
            .into_iter()
            .map(|r| {
                let phone = r
                    .pn_jid
                    .as_ref()
                    .and_then(|j| extract_phone_from_jid(&j.to_string()))
                    .or_else(|| extract_phone_from_jid(&r.jid.to_string()));
                crate::engines::whatsapp::WhatsAppCheckResult {
                    jid: r.jid.to_string(),
                    phone_number: phone,
                    is_registered: r.is_registered,
                    is_business: r.is_business,
                }
            })
            .collect())
    }

    async fn get_profile_picture(
        &self,
        session_id: &str,
        jid_or_phone: &str,
        preview: bool,
    ) -> Result<Option<crate::engines::whatsapp::ProfilePictureInfo>, EngineError> {
        let client = self.connected_client(session_id).await?;
        let jid = resolve_recipient_jid(&client, jid_or_phone).await?;
        let pic = client
            .contacts()
            .get_profile_picture(&jid, preview)
            .await
            .map_err(|e| EngineError::Protocol(format!("get_profile_picture failed: {e}")))?;
        Ok(pic.map(|p| crate::engines::whatsapp::ProfilePictureInfo {
            id: p.id,
            url: p.url,
            direct_path: p.direct_path,
        }))
    }

    async fn list_groups(
        &self,
        session_id: &str,
    ) -> Result<Vec<crate::engines::whatsapp::GroupInfoView>, EngineError> {
        let client = self.connected_client(session_id).await?;
        let map = client
            .groups()
            .get_participating()
            .await
            .map_err(|e| EngineError::Protocol(format!("list_groups failed: {e}")))?;
        Ok(map
            .into_values()
            .map(|g| crate::engines::whatsapp::GroupInfoView {
                jid: g.id.to_string(),
                subject: g.subject,
                participant_count: g.participants.len(),
                participants: g
                    .participants
                    .into_iter()
                    .map(|p| crate::engines::whatsapp::GroupParticipantInfo {
                        jid: p.jid.to_string(),
                        phone_number: p
                            .phone_number
                            .as_ref()
                            .and_then(|j| extract_phone_from_jid(&j.to_string())),
                        is_admin: p.is_admin(),
                    })
                    .collect(),
            })
            .collect())
    }

    async fn get_group_info(
        &self,
        session_id: &str,
        group_jid: &str,
    ) -> Result<crate::engines::whatsapp::GroupInfoView, EngineError> {
        let client = self.connected_client(session_id).await?;
        let jid = resolve_recipient_jid(&client, group_jid).await?;
        let g = client
            .groups()
            .get_metadata(&jid)
            .await
            .map_err(|e| EngineError::Protocol(format!("get_group_info failed: {e}")))?;
        Ok(crate::engines::whatsapp::GroupInfoView {
            jid: g.id.to_string(),
            subject: g.subject,
            participant_count: g.participants.len(),
            participants: g
                .participants
                .into_iter()
                .map(|p| crate::engines::whatsapp::GroupParticipantInfo {
                    jid: p.jid.to_string(),
                    phone_number: p
                        .phone_number
                        .as_ref()
                        .and_then(|j| extract_phone_from_jid(&j.to_string())),
                    is_admin: p.is_admin(),
                })
                .collect(),
        })
    }
}

fn is_ignored_jid(jid: &whatsapp_rust::Jid) -> bool {
    jid.is_status_broadcast() || jid.to_string().ends_with("@newsletter")
}

async fn handle_wa_event(
    session_id: &str,
    event: Arc<Event>,
    client: Arc<whatsapp_rust::Client>,
    tx: &mpsc::Sender<EngineEvent>,
    status_cell: &Arc<RwLock<SessionConnectionState>>,
    qr_cell: &Arc<RwLock<Option<QrPayload>>>,
    pair_code_cell: &Arc<RwLock<Option<PairCodePayload>>>,
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
            *pair_code_cell.write().await = None;
            let own_pn = client
                .persistence_manager()
                .get_device_snapshot()
                .pn
                .clone();
            let jid = own_pn.as_ref().map(|j| j.to_string());
            let phone_number = own_pn
                .as_ref()
                .map(|j| j.user.to_string())
                .filter(|s| !s.is_empty());
            let _ = tx
                .send(EngineEvent::StatusChanged {
                    session_id: session_id.to_string(),
                    status: SessionConnectionState::Connected,
                    phone_number,
                    jid,
                    reason: None,
                })
                .await;
        }
        Event::PairSuccess(pair) => {
            *status_cell.write().await = SessionConnectionState::Connected;
            *qr_cell.write().await = None;
            *pair_code_cell.write().await = None;
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
            *pair_code_cell.write().await = None;
            let raw_reason = format!("{:?}", lo.reason);
            let normalized = normalize_logout_reason(&raw_reason);
            let _ = tx
                .send(EngineEvent::StatusChanged {
                    session_id: session_id.to_string(),
                    status: SessionConnectionState::LoggedOut,
                    phone_number: None,
                    jid: None,
                    reason: Some(normalized),
                })
                .await;
        }
        Event::Messages(batch) => {
            for item in batch.iter() {
                if is_ignored_jid(&item.info.source.chat) {
                    continue;
                }
                let unwrapped = unwrap_message_content(item.message.as_ref());
                // Detect message edits & revokes via protocol_message / edited_message
                if let Some(proto) = unwrapped.protocol_message.as_option() {
                    let target_id = proto.key.as_option().and_then(|k| k.id.clone());
                    if let Some(ext_id) = target_id {
                        if let Some(edited) = proto.edited_message.as_option() {
                            let inner = unwrap_message_content(edited);
                            let new_text = inner
                                .conversation
                                .clone()
                                .or_else(|| {
                                    inner
                                        .extended_text_message
                                        .as_option()
                                        .and_then(|e| e.text.clone())
                                })
                                .unwrap_or_default();
                            let _ = tx
                                .send(EngineEvent::MessageEdited {
                                    session_id: session_id.to_string(),
                                    chat_jid: item.info.source.chat.to_string(),
                                    external_id: ext_id,
                                    new_text,
                                    timestamp: item.info.timestamp,
                                })
                                .await;
                            continue;
                        }
                        if proto.r#type == Some(wa::message::protocol_message::Type::REVOKE) {
                            let _ = tx
                                .send(EngineEvent::MessageRevoked {
                                    session_id: session_id.to_string(),
                                    chat_jid: item.info.source.chat.to_string(),
                                    external_id: ext_id,
                                    sender_jid: Some(item.info.source.sender.to_string()),
                                    timestamp: item.info.timestamp,
                                })
                                .await;
                            continue;
                        }
                    }
                }
                if let Some(inbound) =
                    convert_inbound_message(item.message.as_ref(), &item.info, &client).await
                {
                    let _ = tx
                        .send(EngineEvent::MessageReceived {
                            session_id: session_id.to_string(),
                            message: Box::new(inbound),
                        })
                        .await;
                }
            }
        }
        Event::Receipt(receipt) => {
            if is_ignored_jid(&receipt.source.chat) {
                return;
            }
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
        Event::ChatPresence(cp) => {
            if is_ignored_jid(&cp.source.chat) {
                return;
            }
            let state = match (cp.state, cp.media) {
                (
                    whatsapp_rust::wacore::types::presence::ChatPresence::Composing,
                    whatsapp_rust::wacore::types::presence::ChatPresenceMedia::Audio,
                ) => "recording",
                (whatsapp_rust::wacore::types::presence::ChatPresence::Composing, _) => "composing",
                (whatsapp_rust::wacore::types::presence::ChatPresence::Paused, _) => "paused",
            };
            let _ = tx
                .send(EngineEvent::PresenceUpdated {
                    session_id: session_id.to_string(),
                    chat_jid: cp.source.chat.to_string(),
                    sender_jid: Some(cp.source.sender.to_string()),
                    state: state.to_string(),
                    last_seen: None,
                })
                .await;
        }
        Event::Presence(p) => {
            if is_ignored_jid(&p.from) {
                return;
            }
            let state = if p.unavailable {
                "unavailable"
            } else {
                "available"
            };
            let _ = tx
                .send(EngineEvent::PresenceUpdated {
                    session_id: session_id.to_string(),
                    chat_jid: p.from.to_string(),
                    sender_jid: Some(p.from.to_string()),
                    state: state.to_string(),
                    last_seen: p.last_seen,
                })
                .await;
        }
        Event::ContactUpdate(cu) => {
            if is_ignored_jid(&cu.jid) {
                return;
            }
            let raw_lid = cu
                .action
                .lid_jid
                .clone()
                .filter(|s| !s.trim().is_empty())
                .or_else(|| cu.jid.is_lid().then(|| cu.jid.to_string()));
            let raw_pn = cu
                .action
                .pn_jid
                .clone()
                .filter(|s| !s.trim().is_empty())
                .or_else(|| (!cu.jid.is_lid()).then(|| cu.jid.to_non_ad().to_string()));
            let phone_number = raw_pn
                .as_deref()
                .and_then(|j| j.split('@').next())
                .map(|u| u.split(':').next().unwrap_or(u))
                .filter(|u| !u.is_empty() && u.chars().all(|c| c.is_ascii_digit()))
                .map(ToString::to_string);
            let canonical_jid = phone_number
                .as_ref()
                .map(|p| format!("{p}@s.whatsapp.net"))
                .or(raw_pn)
                .unwrap_or_else(|| cu.jid.to_non_ad().to_string());
            let _ = tx
                .send(EngineEvent::ContactSynced {
                    session_id: session_id.to_string(),
                    jid: canonical_jid,
                    lid: raw_lid,
                    phone_number,
                    full_name: cu.action.full_name.clone().filter(|s| !s.trim().is_empty()),
                    first_name: cu
                        .action
                        .first_name
                        .clone()
                        .filter(|s| !s.trim().is_empty()),
                    push_name: None,
                })
                .await;
        }
        Event::PushNameUpdate(pnu) => {
            if is_ignored_jid(&pnu.jid) || pnu.new_push_name.trim().is_empty() {
                return;
            }
            let bare = pnu.jid.to_non_ad();
            let (jid, lid, phone_number) = if bare.is_lid() {
                if let Ok(Some(entry)) = client.get_lid_pn_entry(&bare).await
                    && !entry.phone_number.is_empty()
                {
                    let pn = entry.phone_number.to_string();
                    (
                        format!("{pn}@s.whatsapp.net"),
                        Some(bare.to_string()),
                        Some(pn),
                    )
                } else {
                    (bare.to_string(), Some(bare.to_string()), None)
                }
            } else {
                let pn = bare
                    .user
                    .chars()
                    .all(|c| c.is_ascii_digit())
                    .then(|| bare.user.to_string());
                (bare.to_string(), None, pn)
            };
            let _ = tx
                .send(EngineEvent::ContactSynced {
                    session_id: session_id.to_string(),
                    jid,
                    lid,
                    phone_number,
                    full_name: None,
                    first_name: None,
                    push_name: Some(pnu.new_push_name.clone()),
                })
                .await;
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

fn unwrap_message_content(mut msg: &wa::Message) -> &wa::Message {
    // Unwrap device-sent (messages sent from the primary phone/WhatsApp Web) and ephemeral wrappers
    for _ in 0..4 {
        if let Some(dsm) = msg.device_sent_message.as_option()
            && let Some(inner) = dsm.message.as_option()
        {
            msg = inner;
            continue;
        }
        if let Some(eph) = msg.ephemeral_message.as_option()
            && let Some(inner) = eph.message.as_option()
        {
            msg = inner;
            continue;
        }
        if let Some(vo) = msg.view_once_message.as_option()
            && let Some(inner) = vo.message.as_option()
        {
            msg = inner;
            continue;
        }
        break;
    }
    msg
}

fn extract_phone_from_jid(jid: &str) -> Option<String> {
    let (user_part, server) = jid.split_once('@')?;
    if !server.eq_ignore_ascii_case("s.whatsapp.net") {
        return None;
    }
    let user = user_part.split(':').next().unwrap_or(user_part);
    let digits: String = user.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() >= 7 {
        Some(digits)
    } else {
        None
    }
}

fn extract_quoted_message(msg: &wa::Message) -> Option<crate::engines::whatsapp::QuotedMessage> {
    let ctx = msg
        .extended_text_message
        .as_option()
        .and_then(|m| m.context_info.as_option())
        .or_else(|| {
            msg.image_message
                .as_option()
                .and_then(|m| m.context_info.as_option())
        })
        .or_else(|| {
            msg.video_message
                .as_option()
                .and_then(|m| m.context_info.as_option())
        })
        .or_else(|| {
            msg.audio_message
                .as_option()
                .and_then(|m| m.context_info.as_option())
        })
        .or_else(|| {
            msg.document_message
                .as_option()
                .and_then(|m| m.context_info.as_option())
        })
        .or_else(|| {
            msg.sticker_message
                .as_option()
                .and_then(|m| m.context_info.as_option())
        })?;

    let stanza_id = ctx.stanza_id.clone().filter(|s| !s.trim().is_empty());
    let participant = ctx.participant.clone().filter(|s| !s.trim().is_empty());
    let text = ctx.quoted_message.as_option().and_then(|qm| {
        let inner = unwrap_message_content(qm);
        inner
            .conversation
            .clone()
            .or_else(|| {
                inner
                    .extended_text_message
                    .as_option()
                    .and_then(|e| e.text.clone())
            })
            .or_else(|| {
                inner
                    .image_message
                    .as_option()
                    .and_then(|i| i.caption.clone())
            })
            .or_else(|| {
                inner
                    .video_message
                    .as_option()
                    .and_then(|v| v.caption.clone())
            })
            .or_else(|| {
                inner
                    .document_message
                    .as_option()
                    .and_then(|d| d.caption.clone().or_else(|| d.file_name.clone()))
            })
            .or_else(|| {
                if inner.audio_message.as_option().is_some() {
                    Some("[audio]".to_string())
                } else if inner.image_message.as_option().is_some() {
                    Some("[image]".to_string())
                } else if inner.video_message.as_option().is_some() {
                    Some("[video]".to_string())
                } else if inner.document_message.as_option().is_some() {
                    Some("[document]".to_string())
                } else if inner.sticker_message.as_option().is_some() {
                    Some("[sticker]".to_string())
                } else if inner.location_message.as_option().is_some() {
                    Some("[location]".to_string())
                } else if inner.contact_message.as_option().is_some()
                    || inner.contacts_array_message.as_option().is_some()
                {
                    Some("[contact]".to_string())
                } else {
                    None
                }
            })
    });

    if stanza_id.is_none() && participant.is_none() && text.is_none() {
        return None;
    }

    Some(crate::engines::whatsapp::QuotedMessage {
        stanza_id,
        participant,
        text,
    })
}

async fn convert_inbound_message(
    raw_msg: &wa::Message,
    info: &whatsapp_rust::wacore::types::message::MessageInfo,
    client: &Arc<whatsapp_rust::Client>,
) -> Option<InboundMessage> {
    let msg = unwrap_message_content(raw_msg);
    let external_id = info.id.clone();
    let chat_jid = info.source.chat.to_string();
    let sender_jid = Some(info.source.sender.to_string());
    let sender_name = {
        let trimmed = info.push_name.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    };
    let mut sender_phone = info
        .source
        .sender_alt
        .as_ref()
        .and_then(|j| extract_phone_from_jid(&j.to_string()))
        .or_else(|| extract_phone_from_jid(&info.source.sender.to_string()))
        .or_else(|| {
            info.source
                .recipient_alt
                .as_ref()
                .and_then(|j| extract_phone_from_jid(&j.to_string()))
        })
        .or_else(|| extract_phone_from_jid(&chat_jid));
    if sender_phone.is_none()
        && info.source.sender.is_lid()
        && let Ok(Some(entry)) = client.get_lid_pn_entry(&info.source.sender).await
        && !entry.phone_number.is_empty()
    {
        sender_phone = Some(entry.phone_number.to_string());
    }
    let from_me = info.source.is_from_me;
    let is_group = info.source.is_group || chat_jid.ends_with("@g.us");
    let mut chat_lid: Option<String> = None;
    let mut chat_jid = chat_jid;
    if !is_group && info.source.chat.is_lid() {
        chat_lid = Some(chat_jid.clone());
        let resolved_chat_pn = info
            .source
            .recipient_alt
            .as_ref()
            .and_then(|j| extract_phone_from_jid(&j.to_string()))
            .or_else(|| if !from_me { sender_phone.clone() } else { None });
        let resolved_chat_pn = match resolved_chat_pn {
            Some(pn) => Some(pn),
            None => client
                .get_lid_pn_entry(&info.source.chat)
                .await
                .ok()
                .flatten()
                .map(|e| e.phone_number.to_string())
                .filter(|s| !s.is_empty()),
        };
        if let Some(pn) = resolved_chat_pn {
            chat_jid = format!("{pn}@s.whatsapp.net");
        }
    }
    let quoted = extract_quoted_message(msg);
    let timestamp = info.timestamp;
    let is_history_sync = (Utc::now() - timestamp).num_seconds() > 300;

    if let Some(audio) = msg.audio_message.as_option() {
        let bytes_opt = download_media_bytes(client, audio).await;
        let mime = audio
            .mimetype
            .clone()
            .unwrap_or_else(|| "audio/ogg; codecs=opus".to_string());
        let is_voice_note = audio.ptt.unwrap_or(false) || mime.contains("ogg");
        return Some(InboundMessage {
            external_id,
            chat_jid,
            chat_lid,
            sender_jid,
            sender_name,
            sender_phone,
            from_me,
            is_group,
            is_history_sync,
            quoted,
            message_type: "audio".to_string(),
            text_body: None,
            caption: None,
            media: bytes_opt.map(|bytes| InboundMedia {
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
        let bytes_opt = download_media_bytes(client, image).await;
        return Some(InboundMessage {
            external_id,
            chat_jid,
            chat_lid,
            sender_jid,
            sender_name,
            sender_phone,
            from_me,
            is_group,
            is_history_sync,
            quoted,
            message_type: "image".to_string(),
            text_body: None,
            caption: image.caption.clone(),
            media: bytes_opt.map(|bytes| InboundMedia {
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
        let bytes_opt = download_media_bytes(client, video).await;
        return Some(InboundMessage {
            external_id,
            chat_jid,
            chat_lid,
            sender_jid,
            sender_name,
            sender_phone,
            from_me,
            is_group,
            is_history_sync,
            quoted,
            message_type: "video".to_string(),
            text_body: None,
            caption: video.caption.clone(),
            media: bytes_opt.map(|bytes| InboundMedia {
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
        let bytes_opt = download_media_bytes(client, doc).await;
        return Some(InboundMessage {
            external_id,
            chat_jid,
            chat_lid,
            sender_jid,
            sender_name,
            sender_phone,
            from_me,
            is_group,
            is_history_sync,
            quoted,
            message_type: "document".to_string(),
            text_body: None,
            caption: doc.caption.clone(),
            media: bytes_opt.map(|bytes| InboundMedia {
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

    if msg.sticker_message.as_option().is_some() {
        return Some(InboundMessage {
            external_id,
            chat_jid,
            chat_lid,
            sender_jid,
            sender_name,
            sender_phone,
            from_me,
            is_group,
            is_history_sync,
            quoted,
            message_type: "sticker".to_string(),
            text_body: None,
            caption: None,
            media: None,
            timestamp,
        });
    }

    if let Some(reaction) = msg.reaction_message.as_option() {
        return Some(InboundMessage {
            external_id,
            chat_jid,
            chat_lid,
            sender_jid,
            sender_name,
            sender_phone,
            from_me,
            is_group,
            is_history_sync,
            quoted,
            message_type: "reaction".to_string(),
            text_body: reaction.text.clone(),
            caption: None,
            media: None,
            timestamp,
        });
    }

    if let Some(loc) = msg.location_message.as_option() {
        let summary = match (loc.degrees_latitude, loc.degrees_longitude) {
            (Some(lat), Some(lon)) => {
                if let Some(name) = loc.name.as_deref().filter(|s| !s.trim().is_empty()) {
                    Some(format!("{name} ({lat:.6}, {lon:.6})"))
                } else {
                    Some(format!("{lat:.6}, {lon:.6}"))
                }
            }
            _ => loc.name.clone().or_else(|| loc.address.clone()),
        };
        return Some(InboundMessage {
            external_id,
            chat_jid,
            chat_lid,
            sender_jid,
            sender_name,
            sender_phone,
            from_me,
            is_group,
            is_history_sync,
            quoted,
            message_type: "location".to_string(),
            text_body: summary,
            caption: loc.comment.clone(),
            media: None,
            timestamp,
        });
    }

    if let Some(contact) = msg.contact_message.as_option() {
        return Some(InboundMessage {
            external_id,
            chat_jid,
            chat_lid,
            sender_jid,
            sender_name,
            sender_phone,
            from_me,
            is_group,
            is_history_sync,
            quoted,
            message_type: "contact".to_string(),
            text_body: contact.display_name.clone(),
            caption: None,
            media: None,
            timestamp,
        });
    }

    if let Some(contacts) = msg.contacts_array_message.as_option() {
        return Some(InboundMessage {
            external_id,
            chat_jid,
            chat_lid,
            sender_jid,
            sender_name,
            sender_phone,
            from_me,
            is_group,
            is_history_sync,
            quoted,
            message_type: "contacts".to_string(),
            text_body: contacts.display_name.clone(),
            caption: None,
            media: None,
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
        chat_lid,
        sender_jid,
        sender_name,
        sender_phone,
        from_me,
        is_group,
        is_history_sync,
        quoted,
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

fn normalize_logout_reason(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("removed") || lower.contains("conflict") || lower.contains("replaced") {
        "device_removed".to_string()
    } else if lower.contains("ban")
        || lower.contains("restrict")
        || lower.contains("suspend")
        || lower.contains("403")
    {
        "account_restricted".to_string()
    } else if lower.contains("401") || lower.contains("unauth") || lower.contains("loggedout") {
        "logged_out".to_string()
    } else {
        raw.to_string()
    }
}
