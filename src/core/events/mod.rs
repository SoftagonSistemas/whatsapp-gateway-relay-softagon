use std::sync::Arc;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use tracing::debug;

use crate::storage::database::{Database, DbError};

pub const ALL_EVENT_TYPES: &[&str] = &[
    "session.status",
    "message.received",
    "message.sent",
    "message.ack",
    "message.transcription.completed",
    "message.transcription.failed",
    "group.updated",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub id: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub session_id: String,
    pub timestamp: String,
    pub data: serde_json::Value,
}

#[derive(Clone)]
pub struct EventBus {
    db: Database,
    webhook_notify: Arc<Notify>,
    max_attempts: u32,
}

impl EventBus {
    pub fn new(db: Database, webhook_notify: Arc<Notify>, max_attempts: u32) -> Self {
        Self {
            db,
            webhook_notify,
            max_attempts,
        }
    }

    pub async fn publish(
        &self,
        event_type: &str,
        session_id: &str,
        data: serde_json::Value,
    ) -> Result<EventEnvelope, DbError> {
        let event_id = format!("evt_{}", uuid::Uuid::now_v7());
        let envelope = EventEnvelope {
            id: event_id.clone(),
            event_type: event_type.to_string(),
            session_id: session_id.to_string(),
            timestamp: Utc::now().to_rfc3339(),
            data,
        };
        let payload_json = serde_json::to_string(&envelope)?;
        let enqueued = self
            .db
            .persist_event_and_enqueue_deliveries(
                &event_id,
                event_type,
                session_id,
                &payload_json,
                self.max_attempts,
            )
            .await?;

        debug!(
            event_id = %event_id,
            event_type = %event_type,
            session_id = %session_id,
            deliveries = enqueued,
            "persisted event and enqueued webhook deliveries"
        );

        if enqueued > 0 {
            self.webhook_notify.notify_one();
        }
        Ok(envelope)
    }
}
