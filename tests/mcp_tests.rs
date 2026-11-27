use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use chrono::Utc;
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::sync::{Notify, watch};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

use softagon_relay::config::{
    AppConfig, JournalMode, McpConfig, MediaBackendConfig, MediaConfig, SqliteConfig,
    TranscriptionConfig, WebhookConfig, WhisperModel,
};
use softagon_relay::core::events::EventBus;
use softagon_relay::core::queue::TranscriptionQueue;
use softagon_relay::core::service::RelayService;
use softagon_relay::core::sessions::SessionManager;
use softagon_relay::core::webhooks::WebhookService;
use softagon_relay::engines::whatsapp::InboundMessage;
use softagon_relay::engines::whatsapp::mock::MockWhatsAppEngine;
use softagon_relay::mcp::http::build_mcp_http_router;
use softagon_relay::storage::database::Database;
use softagon_relay::storage::media::{MediaStorage, ObjectStoreMediaStorage};
use softagon_relay::transcription::whisper::MockTranscriptionEngine;

struct McpTestHarness {
    _dir: TempDir,
    service: RelayService,
    engine: Arc<MockWhatsAppEngine>,
    shutdown_tx: watch::Sender<bool>,
}

impl McpTestHarness {
    async fn new(allow_destructive: bool) -> Self {
        let dir = TempDir::new().expect("tempdir");
        let root = dir.path().to_path_buf();
        let config = AppConfig {
            app_host: "127.0.0.1".to_string(),
            app_port: 8080,
            api_key: "mcp-test-secret-key".to_string(),
            data_dir: root.clone(),
            tmp_dir: root.join("tmp"),
            docs_enabled: true,
            docs_public: false,
            media_max_upload_bytes: 5 * 1024 * 1024,
            session_rate_limit_per_minute: 60,
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
            mcp: McpConfig {
                http_enabled: true,
                http_host: "127.0.0.1".to_string(),
                http_port: 8091,
                allow_container_bind: false,
                allow_destructive,
                allowed_origins: Vec::new(),
            },
        };
        config.ensure_directories().expect("dirs");

        let db = Database::connect(&config.sqlite).await.expect("db");
        let media: Arc<dyn MediaStorage> =
            Arc::new(ObjectStoreMediaStorage::from_config(&config.media).expect("media"));
        let engine = Arc::new(MockWhatsAppEngine::new());
        let tx_engine = Arc::new(MockTranscriptionEngine::new("transcribed", "pt"));

        let wh_notify = Arc::new(Notify::new());
        let tx_notify = Arc::new(Notify::new());
        let event_bus = EventBus::new(db.clone(), wh_notify, config.webhooks.max_attempts);
        let tx_queue =
            TranscriptionQueue::new(db.clone(), config.transcription.max_queue, tx_notify);

        let (sessions, event_rx) = SessionManager::new(
            db.clone(),
            media.clone(),
            engine.clone(),
            event_bus,
            tx_queue,
            config.clone(),
        );
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        sessions.clone().spawn_event_loop(event_rx, shutdown_rx);

        let service = RelayService::new(
            config,
            db.clone(),
            media,
            sessions,
            WebhookService::new(db),
            tx_engine,
        );

        Self {
            _dir: dir,
            service,
            engine,
            shutdown_tx,
        }
    }
}

impl Drop for McpTestHarness {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(true);
    }
}

async fn post_mcp_json(
    router: &axum::Router,
    api_key: Option<&str>,
    origin: Option<&str>,
    payload: Value,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header(header::HOST, "127.0.0.1:8091")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream");

    if let Some(key) = api_key {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {key}"));
    }
    if let Some(orig) = origin {
        builder = builder.header(header::ORIGIN, orig);
    }

    let req = builder.body(Body::from(payload.to_string())).unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let parsed = serde_json::from_slice::<Value>(&bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).to_string()));
    (status, parsed)
}

#[test]
fn mcp_config_rejects_non_loopback_http_host() {
    let bad = McpConfig {
        http_enabled: true,
        http_host: "0.0.0.0".to_string(),
        http_port: 8091,
        ..McpConfig::default()
    };
    assert!(bad.bind_addr().is_err());

    let container_opt_in = McpConfig {
        http_enabled: true,
        http_host: "0.0.0.0".to_string(),
        http_port: 8091,
        allow_container_bind: true,
        ..McpConfig::default()
    };
    assert_eq!(
        container_opt_in.bind_addr().unwrap().to_string(),
        "0.0.0.0:8091"
    );

    let ok = McpConfig::default();
    assert_eq!(ok.bind_addr().unwrap().to_string(), "127.0.0.1:8091");
}

#[tokio::test]
async fn mcp_streamable_http_enforces_origin_and_bearer_auth() {
    let h = McpTestHarness::new(false).await;
    let ct = CancellationToken::new();
    let router = build_mcp_http_router(h.service.clone(), ct.clone());

    let init_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2026-07-28",
            "capabilities": {},
            "clientInfo": { "name": "test-agent", "version": "1.0.0" }
        }
    });

    // 1. Malicious Origin (DNS rebinding attempt) must be rejected with 403 Forbidden
    let (status_evil, _) = post_mcp_json(
        &router,
        Some("mcp-test-secret-key"),
        Some("https://evil.example"),
        init_req.clone(),
    )
    .await;
    assert_eq!(status_evil, StatusCode::FORBIDDEN);

    // 2. Missing Bearer token must be rejected with 401 Unauthorized
    let (status_unauth, _) = post_mcp_json(
        &router,
        None,
        Some("http://127.0.0.1:8091"),
        init_req.clone(),
    )
    .await;
    assert_eq!(status_unauth, StatusCode::UNAUTHORIZED);

    // 3. Valid loopback Origin + Bearer token succeeds
    let (status_ok, body_ok) = post_mcp_json(
        &router,
        Some("mcp-test-secret-key"),
        Some("http://127.0.0.1:8091"),
        init_req,
    )
    .await;
    assert_eq!(status_ok, StatusCode::OK, "unexpected response: {body_ok}");
    assert_eq!(body_ok["result"]["serverInfo"]["name"], "softagon-relay");

    ct.cancel();
}

#[tokio::test]
async fn mcp_agent_workflow_finds_joao_sends_message_and_audits_blocks() {
    let h = McpTestHarness::new(false).await;

    // Create & connect a WhatsApp session
    let session = h
        .service
        .sessions
        .create_session("meu-whatsapp", Some("pt"))
        .await
        .unwrap();
    h.engine
        .simulate_connected(&session.id, "5511999990001", "5511999990001@s.whatsapp.net")
        .await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Simulate an incoming message from João so the agent can discover his chat
    h.engine
        .simulate_inbound_message(
            &session.id,
            InboundMessage {
                external_id: "wamid_joao_1".to_string(),
                chat_jid: "5511988887777@s.whatsapp.net".to_string(),
                sender_jid: Some("5511988887777@s.whatsapp.net".to_string()),
                sender_name: Some("João Silva".to_string()),
                sender_phone: Some("5511988887777".to_string()),
                from_me: false,
                is_history_sync: false,
                message_type: "text".to_string(),
                text_body: Some("Oi, aqui é o João! Que horas é nossa reunião?".to_string()),
                caption: None,
                media: None,
                timestamp: Utc::now(),
            },
        )
        .await;
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;

    let ct = CancellationToken::new();
    let router = build_mcp_http_router(h.service.clone(), ct.clone());
    let key = Some("mcp-test-secret-key");

    // 1. Agent lists tools
    let (st_list, body_list) = post_mcp_json(
        &router,
        key,
        None,
        json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {} }),
    )
    .await;
    assert_eq!(st_list, StatusCode::OK, "tools/list failed: {body_list}");
    let tool_names: Vec<&str> = body_list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert!(tool_names.contains(&"list_sessions"));
    assert!(tool_names.contains(&"list_chats"));
    assert!(tool_names.contains(&"send_text_message"));
    assert!(tool_names.contains(&"send_media_message"));
    assert!(tool_names.contains(&"get_contact"));
    assert!(tool_names.contains(&"mark_read"));
    assert!(tool_names.contains(&"create_pairing_link"));
    assert!(tool_names.contains(&"request_pair_code"));

    // 2. Agent finds João's conversation
    let (st_chats, body_chats) = post_mcp_json(
        &router,
        key,
        None,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "list_chats",
                "arguments": { "session_id": session.id, "query": "João" }
            }
        }),
    )
    .await;
    assert_eq!(st_chats, StatusCode::OK, "list_chats failed: {body_chats}");
    let chats_text = body_chats["result"]["content"][0]["text"].as_str().unwrap();
    assert!(chats_text.contains("5511988887777@s.whatsapp.net"));

    // 3. Agent sends the rescheduled meeting message to João (with idempotency_key)
    let (st_send, body_send) = post_mcp_json(
        &router,
        key,
        None,
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "send_text_message",
                "arguments": {
                    "session_id": session.id,
                    "to": "5511988887777@s.whatsapp.net",
                    "text": "Nossa reunião foi remarcada para amanhã às 14h.",
                    "idempotency_key": "mcp-idem-joao-1"
                }
            }
        }),
    )
    .await;
    assert_eq!(
        st_send,
        StatusCode::OK,
        "send_text_message failed: {body_send}"
    );
    assert_eq!(body_send["result"]["isError"], Value::Bool(false));
    let first_msg: Value =
        serde_json::from_str(body_send["result"]["content"][0]["text"].as_str().unwrap()).unwrap();

    // Retry with the same idempotency_key must return the exact same message_id without duplicating
    let (st_send_dup, body_send_dup) = post_mcp_json(
        &router,
        key,
        None,
        json!({
            "jsonrpc": "2.0",
            "id": 30,
            "method": "tools/call",
            "params": {
                "name": "send_text_message",
                "arguments": {
                    "session_id": session.id,
                    "to": "5511988887777@s.whatsapp.net",
                    "text": "Nossa reunião foi remarcada para amanhã às 14h.",
                    "idempotency_key": "mcp-idem-joao-1"
                }
            }
        }),
    )
    .await;
    assert_eq!(st_send_dup, StatusCode::OK);
    let second_msg: Value = serde_json::from_str(
        body_send_dup["result"]["content"][0]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(first_msg["message_id"], second_msg["message_id"]);

    // 4. Destructive logout_session must be blocked by default
    let (st_block, body_block) = post_mcp_json(
        &router,
        key,
        None,
        json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "logout_session",
                "arguments": { "session_id": session.id, "confirm": true }
            }
        }),
    )
    .await;
    assert_eq!(st_block, StatusCode::OK);
    assert_eq!(body_block["result"]["isError"], Value::Bool(true));

    // 5. Verify audit log recorded ok + blocked calls in SQLite
    let audits = h.service.list_mcp_audit(20).await.unwrap();
    assert!(
        audits
            .iter()
            .any(|a| a.tool_name == "list_chats" && a.status == "ok")
    );
    assert!(
        audits
            .iter()
            .any(|a| a.tool_name == "send_text_message" && a.status == "ok")
    );
    assert!(
        audits
            .iter()
            .any(|a| a.tool_name == "logout_session" && a.status == "blocked")
    );

    ct.cancel();
}
