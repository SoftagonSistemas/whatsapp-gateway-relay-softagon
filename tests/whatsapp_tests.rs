use std::sync::Arc;

use chrono::Utc;
use tempfile::TempDir;
use tokio::sync::{Notify, watch};

use softagon_relay::config::{
    AppConfig, JournalMode, McpConfig, MediaBackendConfig, MediaConfig, SqliteConfig,
    TranscriptionConfig, WebhookConfig, WhisperModel,
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
        mcp: McpConfig::default(),
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
                sender_name: Some("Maria Clara".to_string()),
                sender_phone: Some("551199999999".to_string()),
                from_me: false,
                is_history_sync: false,
                message_type: "text".to_string(),
                text_body: Some("Hello from customer".to_string()),
                caption: None,
                media: None,
                timestamp: Utc::now(),
            },
        )
        .await;

    // Simulate a human reply sent directly from the store's WhatsApp phone (from_me = true)
    engine
        .simulate_inbound_message(
            &sess.id,
            InboundMessage {
                external_id: "wamid.phone_reply_1".to_string(),
                chat_jid: "551199999999@s.whatsapp.net".to_string(),
                sender_jid: Some("551100000000@s.whatsapp.net".to_string()),
                sender_name: Some("Atendimento Loja".to_string()),
                sender_phone: Some("551100000000".to_string()),
                from_me: true,
                is_history_sync: false,
                message_type: "text".to_string(),
                text_body: Some("Oi Maria, ja estamos verificando!".to_string()),
                caption: None,
                media: None,
                timestamp: Utc::now(),
            },
        )
        .await;

    // Duplicate echo of the same phone reply must be deduplicated
    engine
        .simulate_inbound_message(
            &sess.id,
            InboundMessage {
                external_id: "wamid.phone_reply_1".to_string(),
                chat_jid: "551199999999@s.whatsapp.net".to_string(),
                sender_jid: Some("551100000000@s.whatsapp.net".to_string()),
                sender_name: Some("Atendimento Loja".to_string()),
                sender_phone: Some("551100000000".to_string()),
                from_me: true,
                is_history_sync: false,
                message_type: "text".to_string(),
                text_body: Some("Oi Maria, ja estamos verificando!".to_string()),
                caption: None,
                media: None,
                timestamp: Utc::now(),
            },
        )
        .await;

    // Simulate a history-sync message (older than 5 minutes): stored in DB, does NOT emit webhook
    engine
        .simulate_inbound_message(
            &sess.id,
            InboundMessage {
                external_id: "wamid.hist1".to_string(),
                chat_jid: "551199999999@s.whatsapp.net".to_string(),
                sender_jid: Some("551199999999@s.whatsapp.net".to_string()),
                sender_name: Some("Maria Clara".to_string()),
                sender_phone: Some("551199999999".to_string()),
                from_me: false,
                is_history_sync: true,
                message_type: "text".to_string(),
                text_body: Some("Old message from yesterday".to_string()),
                caption: None,
                media: None,
                timestamp: Utc::now() - chrono::Duration::hours(12),
            },
        )
        .await;

    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    let (_, status) = sessions.get_status(&sess.id).await.unwrap();
    assert_eq!(status, "connected");
    let listed = sessions.list_sessions().await.unwrap();
    assert_eq!(listed[0].status, "connected");

    // Verify contact_name, sender_phone, and outbound phone reply are persisted
    let chats = db.list_chats(&sess.id, None, 10).await.unwrap();
    assert_eq!(chats.len(), 1);
    assert_eq!(chats[0].contact_name.as_deref(), Some("Maria Clara"));
    assert_eq!(chats[0].message_count, 3);

    let history = db
        .get_chat_messages(&sess.id, "551199999999@s.whatsapp.net", 10)
        .await
        .unwrap();
    assert_eq!(history.len(), 3);
    assert!(history.iter().any(|m| {
        m.direction == "inbound"
            && m.sender_name.as_deref() == Some("Maria Clara")
            && m.metadata_json
                .contains("\"sender_phone\":\"551199999999\"")
    }));
    assert!(history.iter().any(|m| {
        m.direction == "outbound"
            && m.text_body.as_deref() == Some("Oi Maria, ja estamos verificando!")
            && m.metadata_json.contains("\"device_sync\"")
    }));
    assert!(history.iter().any(|m| {
        m.text_body.as_deref() == Some("Old message from yesterday")
            && m.metadata_json.contains("\"is_history_sync\":true")
    }));

    sessions.logout_session(&sess.id).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let (rec_after, status_after) = sessions.get_status(&sess.id).await.unwrap();
    assert_eq!(status_after, "logged_out");
    assert_eq!(rec_after.status_reason.as_deref(), Some("logout"));

    // Simulate WhatsApp disconnecting the device due to restriction/removal
    db.update_session_status(&sess.id, "logged_out", None, None, Some("device_removed"))
        .await
        .unwrap();
    let send_err = sessions
        .send_text(&sess.id, "551199999999", "ping")
        .await
        .unwrap_err()
        .to_string();
    assert!(
        send_err.contains("device_removed"),
        "expected friendly device_removed error, got: {send_err}"
    );

    let _ = shutdown_tx.send(true);
    let _ = loop_handle.await;
}
