use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{Duration, Utc};
use tokio::sync::{RwLock, mpsc};

use super::{
    EngineError, EngineEvent, InboundMessage, QrPayload, SendMediaParams, SendResult,
    SendTextParams, SessionConnectionState, WhatsAppEngine,
};

struct MockSessionState {
    status: SessionConnectionState,
    qr: Option<QrPayload>,
    event_tx: mpsc::Sender<EngineEvent>,
}

#[derive(Clone, Default)]
pub struct MockWhatsAppEngine {
    sessions: Arc<RwLock<HashMap<String, MockSessionState>>>,
    pub sent_texts: Arc<RwLock<Vec<(String, SendTextParams)>>>,
    pub sent_media: Arc<RwLock<Vec<(String, SendMediaParams)>>>,
}

impl MockWhatsAppEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn simulate_connected(&self, session_id: &str, phone: &str, jid: &str) {
        let tx = {
            let mut guard = self.sessions.write().await;
            if let Some(s) = guard.get_mut(session_id) {
                s.status = SessionConnectionState::Connected;
                s.qr = None;
                Some(s.event_tx.clone())
            } else {
                None
            }
        };
        if let Some(tx) = tx {
            let _ = tx
                .send(EngineEvent::StatusChanged {
                    session_id: session_id.to_string(),
                    status: SessionConnectionState::Connected,
                    phone_number: Some(phone.to_string()),
                    jid: Some(jid.to_string()),
                    reason: None,
                })
                .await;
        }
    }

    pub async fn simulate_inbound_message(&self, session_id: &str, message: InboundMessage) {
        let tx = {
            let guard = self.sessions.read().await;
            guard.get(session_id).map(|s| s.event_tx.clone())
        };
        if let Some(tx) = tx {
            let _ = tx
                .send(EngineEvent::MessageReceived {
                    session_id: session_id.to_string(),
                    message,
                })
                .await;
        }
    }
}

#[async_trait]
impl WhatsAppEngine for MockWhatsAppEngine {
    async fn start_session(
        &self,
        session_id: &str,
        event_tx: mpsc::Sender<EngineEvent>,
    ) -> Result<(), EngineError> {
        let qr = QrPayload {
            code: format!("mock-qr-{session_id}"),
            expires_at: Utc::now() + Duration::seconds(60),
        };
        {
            let mut guard = self.sessions.write().await;
            guard.insert(
                session_id.to_string(),
                MockSessionState {
                    status: SessionConnectionState::PairingRequired,
                    qr: Some(qr.clone()),
                    event_tx: event_tx.clone(),
                },
            );
        }
        let _ = event_tx
            .send(EngineEvent::QrUpdated {
                session_id: session_id.to_string(),
                qr,
            })
            .await;
        let _ = event_tx
            .send(EngineEvent::StatusChanged {
                session_id: session_id.to_string(),
                status: SessionConnectionState::PairingRequired,
                phone_number: None,
                jid: None,
                reason: None,
            })
            .await;
        Ok(())
    }

    async fn stop_session(&self, session_id: &str) -> Result<(), EngineError> {
        let mut guard = self.sessions.write().await;
        if let Some(s) = guard.get_mut(session_id) {
            s.status = SessionConnectionState::Disconnected;
        }
        Ok(())
    }

    async fn logout_session(&self, session_id: &str) -> Result<(), EngineError> {
        let tx = {
            let mut guard = self.sessions.write().await;
            let Some(s) = guard.get_mut(session_id) else {
                return Err(EngineError::SessionNotFound(session_id.to_string()));
            };
            s.status = SessionConnectionState::LoggedOut;
            s.qr = None;
            s.event_tx.clone()
        };
        let _ = tx
            .send(EngineEvent::StatusChanged {
                session_id: session_id.to_string(),
                status: SessionConnectionState::LoggedOut,
                phone_number: None,
                jid: None,
                reason: Some("logout".to_string()),
            })
            .await;
        Ok(())
    }

    async fn get_status(&self, session_id: &str) -> Result<SessionConnectionState, EngineError> {
        let guard = self.sessions.read().await;
        guard
            .get(session_id)
            .map(|s| s.status)
            .ok_or_else(|| EngineError::SessionNotFound(session_id.to_string()))
    }

    async fn get_qr(&self, session_id: &str) -> Result<Option<QrPayload>, EngineError> {
        let guard = self.sessions.read().await;
        guard
            .get(session_id)
            .map(|s| s.qr.clone())
            .ok_or_else(|| EngineError::SessionNotFound(session_id.to_string()))
    }

    async fn send_text(
        &self,
        session_id: &str,
        params: SendTextParams,
    ) -> Result<SendResult, EngineError> {
        let status = self.get_status(session_id).await?;
        if status != SessionConnectionState::Connected {
            return Err(EngineError::NotConnected(session_id.to_string()));
        }
        if params.to.trim().is_empty() {
            return Err(EngineError::InvalidRecipient("empty recipient".to_string()));
        }
        self.sent_texts
            .write()
            .await
            .push((session_id.to_string(), params));
        Ok(SendResult {
            external_message_id: format!("wamid.{}", uuid::Uuid::now_v7()),
        })
    }

    async fn send_media(
        &self,
        session_id: &str,
        params: SendMediaParams,
    ) -> Result<SendResult, EngineError> {
        let status = self.get_status(session_id).await?;
        if status != SessionConnectionState::Connected {
            return Err(EngineError::NotConnected(session_id.to_string()));
        }
        if params.to.trim().is_empty() {
            return Err(EngineError::InvalidRecipient("empty recipient".to_string()));
        }
        self.sent_media
            .write()
            .await
            .push((session_id.to_string(), params));
        Ok(SendResult {
            external_message_id: format!("wamid.{}", uuid::Uuid::now_v7()),
        })
    }
}
