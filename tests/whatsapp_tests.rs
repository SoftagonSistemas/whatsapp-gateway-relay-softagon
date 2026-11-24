use std::sync::Arc;

use chrono::Utc;
use tempfile::TempDir;
use tokio::sync::{Notify, watch};

use softagon_relay::config::{
    AppConfig, JournalMode, MediaBackendConfig, MediaConfig, SqliteConfig, TranscriptionConfig,
    WebhookConfig, WhisperModel,
};
use softagon_relay::core::events::EventBus;
use softagon_relay::core::queue::TranscriptionQueue;
use softagon_relay::core::sessions::SessionManager;
use softagon_relay::engines::whatsapp::InboundMessage;
use softagon_relay::engines::whatsapp::mock::MockWhatsAppEngine;
use softagon_relay::storage::database::Database;
use softagon_relay::storage::media::{MediaStorage, ObjectStoreMediaStorage};

#[tokio::test]
async fn session_manager_handles_inbound_messages_and_logout() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().to_path_buf();
    let config = AppConfig {
        app_host: "127.0.0.1".to_string(),
        app_port: 8080,
        api_key: "secret-key".to_string(),
        data_dir: root.clone(),
        tmp_dir: root.join("tmp"),
        docs_enabled: false,
        docs_public: false,
        media_max_upload_bytes: 1024 * 1024,
        session_rate_limit_per_minute: 10,
        sqlite: SqliteConfig {
            relay_database_path: root.join("relay.db"),
            whatsapp_database_path: root.join("wa.db"),
            busy_timeout_ms: 5000,
            journal_mode: JournalMode::Wal,
        },
        media: MediaConfig {
            backend: MediaBackendConfig::Local {
                path: root.join("media"),
            },
        },
        transcription: TranscriptionConfig {
            enabled: false,
            engine: "whisper".to_string(),
            model: WhisperModel::Base,
            language: None,
            model_dir: root.join("models"),
            preload: false,
            idle_unload_seconds: 180,
            threads: 1,
            workers: 1,
            max_queue: 10,
            timeout_seconds: 30,
            delete_temp_audio: true,
        },
        webhooks: WebhookConfig {
            delivery_timeout_seconds: 5,
            max_attempts: 3,
            backoff_base_seconds: 1,
        },
    };
    config.ensure_directories().unwrap();

    let db = Database::connect(&config.sqlite).await.unwrap();
    let media: Arc<dyn MediaStorage> =
        Arc::new(ObjectStoreMediaStorage::from_config(&config.media).unwrap());
    let engine = Arc::new(MockWhatsAppEngine::new());
    let events = EventBus::new(db.clone(), Arc::new(Notify::new()), 3);
    let queue = TranscriptionQueue::new(db.clone(), 10, Arc::new(Notify::new()));

    let (sessions, rx) =
        SessionManager::new(db.clone(), media, engine.clone(), events, queue, config);

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let loop_handle = sessions.clone().spawn_event_loop(rx, shutdown_rx);

    let sess = sessions.create_session("support", None).await.unwrap();
    engine
        .simulate_connected(&sess.id, "551100000000", "551100000000@s.whatsapp.net")
        .await;

    engine
        .simulate_inbound_message(
            &sess.id,
            InboundMessage {
                external_id: "wamid.in1".to_string(),
                chat_jid: "551199999999@s.whatsapp.net".to_string(),
                sender_jid: Some("551199999999@s.whatsapp.net".to_string()),
                message_type: "text".to_string(),
                text_body: Some("Hello from customer".to_string()),
                caption: None,
                media: None,
                timestamp: Utc::now(),
            },
        )
        .await;

    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    let (_, status) = sessions.get_status(&sess.id).await.unwrap();
    assert_eq!(status, "connected");

    sessions.logout_session(&sess.id).await.unwrap();
    let (_, status_after) = sessions.get_status(&sess.id).await.unwrap();
    assert_eq!(status_after, "logged_out");

    let _ = shutdown_tx.send(true);
    let _ = loop_handle.await;
}
