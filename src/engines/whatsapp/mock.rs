use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{Duration, Utc};
use tokio::sync::{RwLock, mpsc};

use super::{
    EngineError, EngineEvent, InboundMessage, PairCodePayload, QrPayload, SendMediaParams,
    SendResult, SendTextParams, SessionConnectionState, WhatsAppEngine, format_pair_code,
};

struct MockSessionState {
    status: SessionConnectionState,
    qr: Option<QrPayload>,
    pair_code: Option<PairCodePayload>,
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
                s.pair_code = None;
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
                    message: Box::new(message),
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
                    pair_code: None,
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
            s.pair_code = None;
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

    async fn request_pair_code(
        &self,
        session_id: &str,
        phone_number: &str,
    ) -> Result<PairCodePayload, EngineError> {
        let digits: String = phone_number
            .chars()
            .filter(|c| c.is_ascii_digit())
            .collect();
        if digits.len() < 8 || digits.len() > 15 {
            return Err(EngineError::InvalidRecipient(
                "phone_number must contain 8 to 15 digits including country code".into(),
            ));
        }
        let mut guard = self.sessions.write().await;
        let Some(s) = guard.get_mut(session_id) else {
            return Err(EngineError::SessionNotFound(session_id.to_string()));
        };
        if s.status == SessionConnectionState::Connected {
            return Err(EngineError::Protocol("session is already connected".into()));
        }
        let code = "K7QX2M9P".to_string();
        let payload = PairCodePayload {
            formatted_code: format_pair_code(&code),
            code,
            expires_at: Utc::now() + Duration::seconds(180),
        };
        s.pair_code = Some(payload.clone());
        Ok(payload)
    }

    async fn get_pair_code(
        &self,
        session_id: &str,
    ) -> Result<Option<PairCodePayload>, EngineError> {
        let guard = self.sessions.read().await;
        let Some(s) = guard.get(session_id) else {
            return Err(EngineError::SessionNotFound(session_id.to_string()));
        };
        Ok(s.pair_code.clone().filter(|p| p.expires_at > Utc::now()))
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

    async fn mark_read(
        &self,
        session_id: &str,
        _chat_jid: &str,
        _sender_jid: Option<&str>,
        _message_ids: &[String],
    ) -> Result<(), EngineError> {
        let status = self.get_status(session_id).await?;
        if status != SessionConnectionState::Connected {
            return Err(EngineError::NotConnected(session_id.to_string()));
        }
        Ok(())
    }

    async fn resolve_contact(
        &self,
        session_id: &str,
        jid_or_phone: &str,
    ) -> Result<crate::engines::whatsapp::ContactInfo, EngineError> {
        let status = self.get_status(session_id).await?;
        if status != SessionConnectionState::Connected {
            return Err(EngineError::NotConnected(session_id.to_string()));
        }
        let trimmed = jid_or_phone.trim();
        let (jid, lid, phone_number) = if trimmed.ends_with("@lid") {
            (trimmed.to_string(), Some(trimmed.to_string()), None)
        } else if let Some((user, server)) = trimmed.split_once('@') {
            let digits: String = user.chars().filter(|c| c.is_ascii_digit()).collect();
            let pn = if server.eq_ignore_ascii_case("s.whatsapp.net") && digits.len() >= 7 {
                Some(digits)
            } else {
                None
            };
            (trimmed.to_string(), None, pn)
        } else {
            let digits: String = trimmed.chars().filter(|c| c.is_ascii_digit()).collect();
            let jid = format!("{digits}@s.whatsapp.net");
            (
                jid,
                None,
                if digits.len() >= 7 {
                    Some(digits)
                } else {
                    None
                },
            )
        };
        Ok(crate::engines::whatsapp::ContactInfo {
            query: trimmed.to_string(),
            jid,
            lid,
            phone_number,
            push_name: None,
        })
    }
}
