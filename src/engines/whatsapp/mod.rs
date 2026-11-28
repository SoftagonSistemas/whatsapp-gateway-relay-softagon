pub mod mock;
pub mod whatsapp_rust_adapter;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::mpsc;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("session not found: {0}")]
    SessionNotFound(String),
    #[error("session not connected: {0}")]
    NotConnected(String),
    #[error("invalid recipient: {0}")]
    InvalidRecipient(String),
    #[error("media operation failed: {0}")]
    Media(String),
    #[error("protocol engine error: {0}")]
    Protocol(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionConnectionState {
    Created,
    PairingRequired,
    Connecting,
    Connected,
    Disconnected,
    LoggedOut,
    Error,
}

impl SessionConnectionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::PairingRequired => "pairing_required",
            Self::Connecting => "connecting",
            Self::Connected => "connected",
            Self::Disconnected => "disconnected",
            Self::LoggedOut => "logged_out",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QrPayload {
    pub code: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairCodePayload {
    pub code: String,
    pub formatted_code: String,
    pub expires_at: DateTime<Utc>,
}

pub fn format_pair_code(raw: &str) -> String {
    let clean: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if clean.len() == 8 {
        format!("{}-{}", &clean[..4], &clean[4..])
    } else {
        clean
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Image,
    Video,
    Audio,
    Document,
    Sticker,
}

impl MediaKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Video => "video",
            Self::Audio => "audio",
            Self::Document => "document",
            Self::Sticker => "sticker",
        }
    }
}

#[derive(Debug, Clone)]
pub struct InboundMedia {
    pub kind: MediaKind,
    pub mime_type: String,
    pub filename: Option<String>,
    pub bytes: Bytes,
    pub is_voice_note: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotedMessage {
    pub stanza_id: Option<String>,
    pub participant: Option<String>,
    pub text: Option<String>,
}

#[derive(Debug, Clone)]
pub struct InboundMessage {
    pub external_id: String,
    pub chat_jid: String,
    pub chat_lid: Option<String>,
    pub sender_jid: Option<String>,
    pub sender_name: Option<String>,
    pub sender_phone: Option<String>,
    pub from_me: bool,
    pub is_group: bool,
    pub is_history_sync: bool,
    pub quoted: Option<QuotedMessage>,
    pub message_type: String,
    pub text_body: Option<String>,
    pub caption: Option<String>,
    pub media: Option<InboundMedia>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub enum EngineEvent {
    QrUpdated {
        session_id: String,
        qr: QrPayload,
    },
    StatusChanged {
        session_id: String,
        status: SessionConnectionState,
        phone_number: Option<String>,
        jid: Option<String>,
        reason: Option<String>,
    },
    MessageReceived {
        session_id: String,
        message: Box<InboundMessage>,
    },
    MessageAck {
        session_id: String,
        external_id: String,
        chat_jid: String,
        ack_status: String,
    },
    GroupUpdated {
        session_id: String,
        group_jid: String,
        action: String,
        details: serde_json::Value,
    },
    PresenceUpdated {
        session_id: String,
        chat_jid: String,
        sender_jid: Option<String>,
        state: String,
        last_seen: Option<DateTime<Utc>>,
    },
    MessageEdited {
        session_id: String,
        chat_jid: String,
        external_id: String,
        new_text: String,
        timestamp: DateTime<Utc>,
    },
    MessageRevoked {
        session_id: String,
        chat_jid: String,
        external_id: String,
        sender_jid: Option<String>,
        timestamp: DateTime<Utc>,
    },
    ContactSynced {
        session_id: String,
        jid: String,
        lid: Option<String>,
        phone_number: Option<String>,
        full_name: Option<String>,
        first_name: Option<String>,
        push_name: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct SendTextParams {
    pub to: String,
    pub text: String,
    pub reply_to_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SendMediaParams {
    pub to: String,
    pub kind: MediaKind,
    pub mime_type: String,
    pub filename: Option<String>,
    pub caption: Option<String>,
    pub data: Bytes,
    pub as_voice_note: bool,
    pub reply_to_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SendReactionParams {
    pub chat_jid: String,
    pub message_external_id: String,
    pub emoji: String,
    pub from_me: bool,
    pub participant: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SendLocationParams {
    pub to: String,
    pub latitude: f64,
    pub longitude: f64,
    pub name: Option<String>,
    pub address: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SendContactParams {
    pub to: String,
    pub display_name: String,
    pub vcard: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatPresenceState {
    Composing,
    Recording,
    Paused,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfilePictureInfo {
    pub id: String,
    pub url: String,
    pub direct_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhatsAppCheckResult {
    pub jid: String,
    pub phone_number: Option<String>,
    pub is_registered: bool,
    pub is_business: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupParticipantInfo {
    pub jid: String,
    pub phone_number: Option<String>,
    pub is_admin: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupInfoView {
    pub jid: String,
    pub subject: String,
    pub participant_count: usize,
    pub participants: Vec<GroupParticipantInfo>,
}

#[derive(Debug, Clone)]
pub struct SendResult {
    pub external_message_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactInfo {
    pub query: String,
    pub jid: String,
    pub lid: Option<String>,
    pub phone_number: Option<String>,
    pub full_name: Option<String>,
    pub first_name: Option<String>,
    pub push_name: Option<String>,
}

#[async_trait]
pub trait WhatsAppEngine: Send + Sync {
    async fn start_session(
        &self,
        session_id: &str,
        event_tx: mpsc::Sender<EngineEvent>,
    ) -> Result<(), EngineError>;

    async fn stop_session(&self, session_id: &str) -> Result<(), EngineError>;

    async fn logout_session(&self, session_id: &str) -> Result<(), EngineError>;

    async fn delete_session_data(&self, _session_id: &str) -> Result<(), EngineError> {
        Ok(())
    }

    async fn get_status(&self, session_id: &str) -> Result<SessionConnectionState, EngineError>;

    async fn get_qr(&self, session_id: &str) -> Result<Option<QrPayload>, EngineError>;

    async fn request_pair_code(
        &self,
        _session_id: &str,
        _phone_number: &str,
    ) -> Result<PairCodePayload, EngineError> {
        Err(EngineError::Protocol(
            "pair code authentication is not supported by this engine".into(),
        ))
    }

    async fn get_pair_code(
        &self,
        _session_id: &str,
    ) -> Result<Option<PairCodePayload>, EngineError> {
        Ok(None)
    }

    async fn send_text(
        &self,
        session_id: &str,
        params: SendTextParams,
    ) -> Result<SendResult, EngineError>;

    async fn send_media(
        &self,
        session_id: &str,
        params: SendMediaParams,
    ) -> Result<SendResult, EngineError>;

    async fn mark_read(
        &self,
        _session_id: &str,
        _chat_jid: &str,
        _sender_jid: Option<&str>,
        _message_ids: &[String],
    ) -> Result<(), EngineError> {
        Ok(())
    }

    async fn resolve_contact(
        &self,
        _session_id: &str,
        jid_or_phone: &str,
    ) -> Result<ContactInfo, EngineError> {
        Ok(ContactInfo {
            query: jid_or_phone.to_string(),
            jid: jid_or_phone.to_string(),
            lid: None,
            phone_number: None,
            full_name: None,
            first_name: None,
            push_name: None,
        })
    }

    async fn save_contact(
        &self,
        _session_id: &str,
        jid_or_phone: &str,
        _full_name: Option<&str>,
        _first_name: Option<&str>,
        _save_on_primary_addressbook: bool,
    ) -> Result<String, EngineError> {
        let digits: String = jid_or_phone
            .chars()
            .filter(|c| c.is_ascii_digit())
            .collect();
        Ok(if jid_or_phone.contains('@') {
            jid_or_phone.to_string()
        } else {
            format!("{digits}@s.whatsapp.net")
        })
    }

    async fn send_reaction(
        &self,
        _session_id: &str,
        _params: SendReactionParams,
    ) -> Result<SendResult, EngineError> {
        Ok(SendResult {
            external_message_id: format!("wamid.{}", uuid::Uuid::now_v7()),
        })
    }

    async fn send_chat_presence(
        &self,
        _session_id: &str,
        _chat_jid: &str,
        _state: ChatPresenceState,
    ) -> Result<(), EngineError> {
        Ok(())
    }

    async fn subscribe_presence(&self, _session_id: &str, _jid: &str) -> Result<(), EngineError> {
        Ok(())
    }

    async fn send_location(
        &self,
        _session_id: &str,
        _params: SendLocationParams,
    ) -> Result<SendResult, EngineError> {
        Ok(SendResult {
            external_message_id: format!("wamid.{}", uuid::Uuid::now_v7()),
        })
    }

    async fn send_contact(
        &self,
        _session_id: &str,
        _params: SendContactParams,
    ) -> Result<SendResult, EngineError> {
        Ok(SendResult {
            external_message_id: format!("wamid.{}", uuid::Uuid::now_v7()),
        })
    }

    async fn edit_message(
        &self,
        _session_id: &str,
        _chat_jid: &str,
        _external_message_id: &str,
        _new_text: &str,
    ) -> Result<String, EngineError> {
        Ok(format!("wamid.{}", uuid::Uuid::now_v7()))
    }

    async fn revoke_message(
        &self,
        _session_id: &str,
        _chat_jid: &str,
        _external_message_id: &str,
    ) -> Result<(), EngineError> {
        Ok(())
    }

    async fn is_on_whatsapp(
        &self,
        _session_id: &str,
        phones: &[String],
    ) -> Result<Vec<WhatsAppCheckResult>, EngineError> {
        Ok(phones
            .iter()
            .map(|p| WhatsAppCheckResult {
                jid: format!("{p}@s.whatsapp.net"),
                phone_number: Some(p.clone()),
                is_registered: true,
                is_business: false,
            })
            .collect())
    }

    async fn get_profile_picture(
        &self,
        _session_id: &str,
        _jid_or_phone: &str,
        _preview: bool,
    ) -> Result<Option<ProfilePictureInfo>, EngineError> {
        Ok(None)
    }

    async fn list_groups(&self, _session_id: &str) -> Result<Vec<GroupInfoView>, EngineError> {
        Ok(Vec::new())
    }

    async fn get_group_info(
        &self,
        _session_id: &str,
        group_jid: &str,
    ) -> Result<GroupInfoView, EngineError> {
        Ok(GroupInfoView {
            jid: group_jid.to_string(),
            subject: String::new(),
            participant_count: 0,
            participants: Vec::new(),
        })
    }
}
