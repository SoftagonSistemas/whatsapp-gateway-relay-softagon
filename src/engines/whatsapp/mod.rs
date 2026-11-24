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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Image,
    Video,
    Audio,
    Document,
}

impl MediaKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Video => "video",
            Self::Audio => "audio",
            Self::Document => "document",
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

#[derive(Debug, Clone)]
pub struct InboundMessage {
    pub external_id: String,
    pub chat_jid: String,
    pub sender_jid: Option<String>,
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
        message: InboundMessage,
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
}

#[derive(Debug, Clone)]
pub struct SendTextParams {
    pub to: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct SendMediaParams {
    pub to: String,
    pub kind: MediaKind,
    pub mime_type: String,
    pub filename: Option<String>,
    pub caption: Option<String>,
    pub data: Bytes,
}

#[derive(Debug, Clone)]
pub struct SendResult {
    pub external_message_id: String,
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

    async fn get_status(&self, session_id: &str) -> Result<SessionConnectionState, EngineError>;

    async fn get_qr(&self, session_id: &str) -> Result<Option<QrPayload>, EngineError>;

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
}
