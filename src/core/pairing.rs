use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

pub const DEFAULT_PAIR_LINK_TTL_SECONDS: i64 = 600;
pub const MIN_PAIR_LINK_TTL_SECONDS: i64 = 60;
pub const MAX_PAIR_LINK_TTL_SECONDS: i64 = 1800;
pub const PAIR_LINK_RECONNECT_COOLDOWN_SECONDS: i64 = 10;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingLinkRecord {
    pub token: String,
    pub session_id: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    #[serde(skip)]
    pub last_reconnect_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default)]
pub struct PairingLinkStore {
    inner: Arc<RwLock<HashMap<String, PairingLinkRecord>>>,
}

impl PairingLinkStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn create(&self, session_id: &str, ttl_seconds: Option<i64>) -> PairingLinkRecord {
        let ttl = ttl_seconds
            .unwrap_or(DEFAULT_PAIR_LINK_TTL_SECONDS)
            .clamp(MIN_PAIR_LINK_TTL_SECONDS, MAX_PAIR_LINK_TTL_SECONDS);

        let mut bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut bytes);
        let token = hex::encode(bytes);

        let now = Utc::now();
        let record = PairingLinkRecord {
            token: token.clone(),
            session_id: session_id.to_string(),
            created_at: now,
            expires_at: now + Duration::seconds(ttl),
            last_reconnect_at: None,
        };

        let mut guard = self.inner.write().await;
        // Prune expired tokens on write
        guard.retain(|_, v| v.expires_at > now);
        guard.insert(token, record.clone());
        record
    }

    pub async fn get_valid(&self, token: &str) -> Option<PairingLinkRecord> {
        let now = Utc::now();
        let guard = self.inner.read().await;
        guard.get(token).filter(|r| r.expires_at > now).cloned()
    }

    /// Checks and records a reconnect attempt for this pairing token.
    /// Returns `Ok(true)` if allowed, `Ok(false)` if still within cooldown,
    /// or `Err(())` if token is missing/expired.
    pub async fn try_mark_reconnect(&self, token: &str) -> Result<bool, ()> {
        let now = Utc::now();
        let mut guard = self.inner.write().await;
        let Some(rec) = guard.get_mut(token) else {
            return Err(());
        };
        if rec.expires_at <= now {
            guard.remove(token);
            return Err(());
        }
        if let Some(last) = rec.last_reconnect_at
            && (now - last).num_seconds() < PAIR_LINK_RECONNECT_COOLDOWN_SECONDS
        {
            return Ok(false);
        }
        rec.last_reconnect_at = Some(now);
        Ok(true)
    }

    pub async fn invalidate(&self, token: &str) {
        self.inner.write().await.remove(token);
    }

    pub async fn invalidate_for_session(&self, session_id: &str) {
        self.inner
            .write()
            .await
            .retain(|_, v| v.session_id != session_id);
    }
}
