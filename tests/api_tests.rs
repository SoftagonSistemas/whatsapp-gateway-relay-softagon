use std::collections::HashMap;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::routing::post;
use base64::Engine as _;
use bytes::Bytes;
use http::header;
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, Notify, watch};
use tower::ServiceExt;

use softagon_relay::api::docs::OPENAPI_SPEC_YAML;
use softagon_relay::api::{AppState, build_router};
use softagon_relay::config::{
    AppConfig, JournalMode, McpConfig, MediaBackendConfig, MediaConfig, SqliteConfig,
    TranscriptionConfig, WebhookConfig, WhisperModel,
};
use softagon_relay::core::events::EventBus;
use softagon_relay::core::queue::TranscriptionQueue;
use softagon_relay::core::sessions::SessionManager;
use softagon_relay::core::webhooks::{WebhookDispatcher, WebhookService, sign_payload};
use softagon_relay::engines::whatsapp::mock::MockWhatsAppEngine;
use softagon_relay::storage::database::Database;
use softagon_relay::storage::media::{MediaStorage, ObjectStoreMediaStorage};
use softagon_relay::transcription::whisper::MockTranscriptionEngine;

struct TestHarness {
    _dir: TempDir,
    router: Router,
    engine: Arc<MockWhatsAppEngine>,
    api_key: String,
    shutdown_tx: watch::Sender<bool>,
}

impl TestHarness {
    async fn new(docs_public: bool) -> Self {
        let dir = TempDir::new().expect("tempdir");
        let root = dir.path().to_path_buf();
        let api_key = "test-secret-api-key-12345".to_string();

        let config = AppConfig {
            app_host: "127.0.0.1".to_string(),
            app_port: 0,
            api_key: api_key.clone(),
            data_dir: root.clone(),
            tmp_dir: root.join("tmp"),
            docs_enabled: true,
            docs_public,
            media_max_upload_bytes: 5 * 1024 * 1024,
            media_retention_days: 0,
            session_rate_limit_per_minute: 60,
            sqlite: SqliteConfig {
                relay_database_path: root.join("relay.db"),
                whatsapp_database_path: root.join("whatsapp-sessions.db"),
                busy_timeout_ms: 5000,
                journal_mode: JournalMode::Wal,
            },
            media: MediaConfig {
                backend: MediaBackendConfig::Local {
                    path: root.join("media"),
                },
            },
            transcription: TranscriptionConfig {
                enabled: true,
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
                inline_wait_seconds: 0,
            },
            webhooks: WebhookConfig {
                delivery_timeout_seconds: 5,
                max_attempts: 3,
                backoff_base_seconds: 1,
                coalesce_seconds: 0,
            },
            mcp: McpConfig::default(),
        };
        config.ensure_directories().expect("dirs");

        let db = Database::connect(&config.sqlite).await.expect("db");
        let media: Arc<dyn MediaStorage> =
            Arc::new(ObjectStoreMediaStorage::from_config(&config.media).expect("media"));
        let engine = Arc::new(MockWhatsAppEngine::new());
        let tx_engine = Arc::new(MockTranscriptionEngine::new("hello world", "en"));

        let wh_notify = Arc::new(Notify::new());
        let tx_notify = Arc::new(Notify::new());

        let event_bus = EventBus::new(db.clone(), wh_notify.clone(), config.webhooks.max_attempts);
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
        let _event_loop = sessions
            .clone()
            .spawn_event_loop(event_rx, shutdown_rx.clone());
        let _dispatcher = WebhookDispatcher::new(db.clone(), wh_notify, config.webhooks.clone())
            .spawn(shutdown_rx);

        let state = AppState {
            config,
            db: db.clone(),
            media,
            sessions,
            webhooks: WebhookService::new(db),
            transcription_engine: tx_engine,
            pairing_links: softagon_relay::core::pairing::PairingLinkStore::new(),
        };

        Self {
            _dir: dir,
            router: build_router(state),
            engine,
            api_key,
            shutdown_tx,
        }
    }

    async fn request(&self, req: Request<Body>) -> (StatusCode, Value) {
        let resp = self.router.clone().oneshot(req).await.expect("oneshot");
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .expect("body");
        let val = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes)
                .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).to_string()))
        };
        (status, val)
    }

    fn auth_header(&self) -> String {
        format!("Bearer {}", self.api_key)
    }
}

impl Drop for TestHarness {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(true);
    }
}

#[tokio::test]
async fn health_is_public_and_sessions_require_bearer_auth() {
    let app = TestHarness::new(false).await;

    let (status, body) = app
        .request(
            Request::builder()
                .uri("/v1/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
    assert_eq!(body["database"], "ok");
    assert_eq!(body["media_storage"], "local");

    // Unauthenticated request to protected route must fail with 401
    let (status, _) = app
        .request(
            Request::builder()
                .uri("/v1/sessions")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Docs protected when DOCS_PUBLIC=false
    let (status, _) = app
        .request(Request::builder().uri("/docs").body(Body::empty()).unwrap())
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Authenticated request succeeds
    let (status, body) = app
        .request(
            Request::builder()
                .uri("/v1/sessions")
                .header(header::AUTHORIZATION, app.auth_header())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["sessions"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn session_lifecycle_idempotent_send_and_signed_webhook_delivery() {
    let app = TestHarness::new(true).await;

    // Spin up local webhook receiver
    type ReceivedHooks = Arc<Mutex<Vec<(HashMap<String, String>, Bytes)>>>;
    let received: ReceivedHooks = Arc::new(Mutex::new(Vec::new()));
    let recv_clone = received.clone();
    let receiver_app = Router::new().route(
        "/hook",
        post(move |headers: HeaderMap, body: Bytes| {
            let store = recv_clone.clone();
            async move {
                let mut map = HashMap::new();
                for (k, v) in headers.iter() {
                    if let Ok(s) = v.to_str() {
                        map.insert(k.as_str().to_ascii_lowercase(), s.to_string());
                    }
                }
                store.lock().await.push((map, body));
                StatusCode::OK
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, receiver_app).await;
    });

    // Register webhook via /v1/webhooks
    let (status, wh_body) = app
        .request(
            Request::builder()
                .method("POST")
                .uri("/v1/webhooks")
                .header(header::AUTHORIZATION, app.auth_header())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "url": format!("http://{addr}/hook"),
                        "events": ["message.sent", "session.status"]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let wh_id = wh_body["id"].as_str().unwrap().to_string();
    let wh_secret = wh_body["secret"].as_str().unwrap().to_string();
    assert!(wh_secret.starts_with("whsec_"));

    // Secret must not be returned on GET
    let (status, wh_get) = app
        .request(
            Request::builder()
                .uri(format!("/v1/webhooks/{wh_id}"))
                .header(header::AUTHORIZATION, app.auth_header())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(wh_get.get("secret").is_none());

    // Create session
    let (status, sess) = app
        .request(
            Request::builder()
                .method("POST")
                .uri("/v1/sessions")
                .header(header::AUTHORIZATION, app.auth_header())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "name": "primary" }).to_string()))
                .unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let session_id = sess["id"].as_str().unwrap().to_string();

    // Fetch QR
    let (status, qr_body) = app
        .request(
            Request::builder()
                .uri(format!("/v1/sessions/{session_id}/qr"))
                .header(header::AUTHORIZATION, app.auth_header())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        qr_body["qr"]["code"]
            .as_str()
            .unwrap()
            .contains(&session_id)
    );

    // Simulate connected state
    app.engine
        .simulate_connected(&session_id, "5511999999999", "5511999999999@s.whatsapp.net")
        .await;

    // Send text with Idempotency-Key
    let payload = json!({ "to": "5511888888888", "text": "Hello Relay" }).to_string();
    let (status1, msg1) = app
        .request(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{session_id}/messages/text"))
                .header(header::AUTHORIZATION, app.auth_header())
                .header(header::CONTENT_TYPE, "application/json")
                .header("Idempotency-Key", "idem-1")
                .body(Body::from(payload.clone()))
                .unwrap(),
        )
        .await;
    assert_eq!(status1, StatusCode::CREATED);

    // Repeat same request with same Idempotency-Key returns same message ID without re-sending
    let (status2, msg2) = app
        .request(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{session_id}/messages/text"))
                .header(header::AUTHORIZATION, app.auth_header())
                .header(header::CONTENT_TYPE, "application/json")
                .header("Idempotency-Key", "idem-1")
                .body(Body::from(payload))
                .unwrap(),
        )
        .await;
    assert_eq!(status2, StatusCode::CREATED);
    assert_eq!(msg1["id"], msg2["id"]);
    assert_eq!(app.engine.sent_texts.read().await.len(), 1);

    // Send media message
    let b64 = base64::engine::general_purpose::STANDARD.encode(b"fake-image-bytes");
    let (status_media, media_msg) = app
        .request(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{session_id}/messages/media"))
                .header(header::AUTHORIZATION, app.auth_header())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "to": "5511888888888",
                        "media_type": "image",
                        "mime_type": "image/png",
                        "caption": "Test",
                        "data_base64": b64
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await;
    assert_eq!(status_media, StatusCode::CREATED);
    assert!(media_msg["media_id"].as_str().is_some());

    // Wait briefly for webhook dispatcher to deliver signed events
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    let deliveries = received.lock().await;
    assert!(
        !deliveries.is_empty(),
        "expected at least one webhook delivery"
    );

    // Verify HMAC-SHA256 signature on every delivered webhook
    for (hdrs, body) in deliveries.iter() {
        let ts: i64 = hdrs.get("x-relay-timestamp").unwrap().parse().unwrap();
        let sig = hdrs.get("x-relay-signature").unwrap();
        let expected_sig = sign_payload(&wh_secret, ts, body);
        assert_eq!(sig, &expected_sig);
        assert!(hdrs.get("x-relay-event-id").unwrap().starts_with("evt_"));
    }

    // Delete webhook
    let (status_del, _) = app
        .request(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/webhooks/{wh_id}"))
                .header(header::AUTHORIZATION, app.auth_header())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(status_del, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn console_spa_serves_and_redirects_root_and_manager() {
    let app = TestHarness::new(false).await;

    let (status_root, _) = app
        .request(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await;
    assert_eq!(status_root, StatusCode::PERMANENT_REDIRECT);

    let (status_mgr, _) = app
        .request(
            Request::builder()
                .uri("/manager")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(status_mgr, StatusCode::PERMANENT_REDIRECT);

    let (status_console, body_console) = app
        .request(
            Request::builder()
                .uri("/console/")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(status_console, StatusCode::OK);
    assert!(
        body_console
            .as_str()
            .unwrap_or("")
            .contains("<div id=\"app\">")
    );

    let (status_spa, body_spa) = app
        .request(
            Request::builder()
                .uri("/console/login")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(status_spa, StatusCode::OK);
    assert!(body_spa.as_str().unwrap_or("").contains("<div id=\"app\">"));

    let (status_json_unauth, _) = app
        .request(
            Request::builder()
                .uri("/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(status_json_unauth, StatusCode::UNAUTHORIZED);

    let (status_json, body_json) = app
        .request(
            Request::builder()
                .uri("/openapi.json")
                .header(header::AUTHORIZATION, app.auth_header())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(status_json, StatusCode::OK);
    assert_eq!(body_json["openapi"], "3.2.1");
    assert!(body_json["paths"]["/v1/health"].is_object());
}

#[test]
fn openapi_spec_is_valid_3_2_1_yaml() {
    let doc: serde_yaml_ng::Value = serde_yaml_ng::from_str(OPENAPI_SPEC_YAML).expect("valid YAML");
    assert_eq!(doc["openapi"].as_str(), Some("3.2.1"));
    let paths = doc["paths"].as_mapping().expect("paths mapping");
    for expected in [
        "/v1/health",
        "/v1/sessions",
        "/v1/sessions/{session_id}",
        "/v1/sessions/{session_id}/qr",
        "/v1/sessions/{session_id}/qr.svg",
        "/v1/sessions/{session_id}/qr.png",
        "/v1/sessions/{session_id}/pair-link",
        "/v1/sessions/{session_id}/pair-code",
        "/v1/sessions/{session_id}/status",
        "/v1/sessions/{session_id}/logout",
        "/v1/sessions/{session_id}/messages/text",
        "/v1/sessions/{session_id}/messages/media",
        "/v1/messages/{message_id}",
        "/v1/messages/{message_id}/transcription",
        "/v1/webhooks",
        "/v1/webhooks/{webhook_id}",
    ] {
        assert!(
            paths.contains_key(serde_yaml_ng::Value::String(expected.to_string())),
            "missing path {expected} in openapi.yaml"
        );
    }
}

#[tokio::test]
async fn pairing_link_qr_png_and_eight_digit_code_flow() {
    let app = TestHarness::new(false).await;

    // Create a session ready for pairing
    let (status, sess) = app
        .request(
            Request::builder()
                .method("POST")
                .uri("/v1/sessions")
                .header(header::AUTHORIZATION, app.auth_header())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "name": "grok-bot" }).to_string()))
                .unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let session_id = sess["id"].as_str().unwrap().to_string();

    // Authenticated QR PNG returns valid PNG magic header
    let resp = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/sessions/{session_id}/qr.png"))
                .header(header::AUTHORIZATION, app.auth_header())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let png_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    assert!(png_bytes.starts_with(b"\x89PNG\r\n\x1a\n"));

    // Create zero-login temporary pairing link
    let (status, link_body) = app
        .request(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{session_id}/pair-link"))
                .header(header::AUTHORIZATION, app.auth_header())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "ttl_seconds": 300 }).to_string()))
                .unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let pair_path = link_body["pair_path"].as_str().unwrap().to_string();
    let qr_png_path = link_body["qr_png_path"].as_str().unwrap().to_string();
    let status_path = link_body["status_path"].as_str().unwrap().to_string();

    // Public pairing HTML page works WITHOUT Authorization header
    let resp = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri(&pair_path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = String::from_utf8(
        axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(html.contains("Scan with WhatsApp"));
    assert!(!html.contains(&app.api_key));

    // Public QR PNG works without Authorization header
    let resp = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri(&qr_png_path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let pub_png = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    assert!(pub_png.starts_with(b"\x89PNG\r\n\x1a\n"));

    // Public 8-digit pairing code works on the same pairing token
    let (status, code_body) = app
        .request(
            Request::builder()
                .method("POST")
                .uri(format!("{pair_path}/pair-code"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({ "phone_number": "5511999990001" }).to_string(),
                ))
                .unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(code_body["formatted_code"], "K7QX-2M9P");

    // Once connected, status reports connected and invalidates the pairing token
    app.engine
        .simulate_connected(&session_id, "5511999990001", "5511999990001@s.whatsapp.net")
        .await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let (status, st_body) = app
        .request(
            Request::builder()
                .uri(&status_path)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(st_body["status"], "connected");

    // Subsequent access to the same pairing link returns 404 (auto-invalidated)
    let resp = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri(&pair_path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn webhook_bearer_token_and_custom_auth_header() {
    let app = TestHarness::new(false).await;

    // Create webhook with bearer_token shorthand (for Grok Bot routines)
    let (status, body) = app
        .request(
            Request::builder()
                .method("POST")
                .uri("/v1/webhooks")
                .header(header::AUTHORIZATION, app.auth_header())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "url": "https://example.com/grok-routine",
                        "events": ["message.received", "message.sent"],
                        "bearer_token": "grok_secret_token_123"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["auth_header_name"], "Authorization");
    assert_eq!(body["has_auth_header"], true);
    // Secret token must NEVER be echoed back in JSON responses
    let body_str = body.to_string();
    assert!(!body_str.contains("grok_secret_token_123"));

    // Update to a custom header via PATCH
    let wh_id = body["id"].as_str().unwrap();
    let (status_patch, body_patch) = app
        .request(
            Request::builder()
                .method("PATCH")
                .uri(format!("/v1/webhooks/{wh_id}"))
                .header(header::AUTHORIZATION, app.auth_header())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "auth_header_name": "X-Custom-Token",
                        "auth_header_value": "custom_val_999"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await;
    assert_eq!(status_patch, StatusCode::OK);
    assert_eq!(body_patch["auth_header_name"], "X-Custom-Token");
    assert_eq!(body_patch["has_auth_header"], true);
    assert!(!body_patch.to_string().contains("custom_val_999"));
}

#[tokio::test]
async fn contacts_mark_read_media_url_and_live_health() {
    let app = TestHarness::new(false).await;

    // Create and connect a session
    let (st, sess) = app
        .request(
            Request::builder()
                .method("POST")
                .uri("/v1/sessions")
                .header(header::AUTHORIZATION, app.auth_header())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "name": "store-live" }).to_string()))
                .unwrap(),
        )
        .await;
    assert_eq!(st, StatusCode::CREATED);
    let session_id = sess["id"].as_str().unwrap().to_string();

    app.engine
        .simulate_connected(&session_id, "551100000001", "551100000001@s.whatsapp.net")
        .await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // /v1/health reports live connected session count
    let (st_health, health_body) = app
        .request(
            Request::builder()
                .uri("/v1/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(st_health, StatusCode::OK);
    assert_eq!(health_body["active_sessions"], 1);
    assert_eq!(health_body["total_sessions"], 1);
    assert_eq!(health_body["sessions_healthy"], true);

    // Simulate inbound message from an @lid contact with resolved sender_phone + sender_name
    app.engine
        .simulate_inbound_message(
            &session_id,
            softagon_relay::engines::whatsapp::InboundMessage {
                external_id: "wamid.lid_msg_1".to_string(),
                chat_jid: "123456789012345@lid".to_string(),
                chat_lid: Some("123456789012345@lid".to_string()),
                sender_jid: Some("123456789012345@lid".to_string()),
                sender_name: Some("Ana Paula".to_string()),
                sender_phone: Some("5511977776666".to_string()),
                from_me: false,
                is_group: false,
                is_history_sync: false,
                quoted: None,
                message_type: "text".to_string(),
                text_body: Some("Tem caderno em estoque?".to_string()),
                caption: None,
                media: None,
                timestamp: chrono::Utc::now(),
            },
        )
        .await;
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;

    // GET /v1/sessions/{id}/contacts/{jid} resolves @lid -> phone_number + push_name
    let (st_contact, contact_body) = app
        .request(
            Request::builder()
                .uri(format!(
                    "/v1/sessions/{session_id}/contacts/123456789012345@lid"
                ))
                .header(header::AUTHORIZATION, app.auth_header())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(st_contact, StatusCode::OK);
    assert_eq!(contact_body["phone_number"], "5511977776666");
    assert_eq!(contact_body["push_name"], "Ana Paula");
    assert_eq!(contact_body["jid"], "5511977776666@s.whatsapp.net");

    // POST /v1/sessions/{id}/chats/read marks latest inbound messages as read
    let (st_read, read_body) = app
        .request(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{session_id}/chats/read"))
                .header(header::AUTHORIZATION, app.auth_header())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({ "chat_jid": "123456789012345@lid" }).to_string(),
                ))
                .unwrap(),
        )
        .await;
    assert_eq!(st_read, StatusCode::OK);
    assert_eq!(read_body["marked_count"], 1);

    // Reject invalid media_url schemes (e.g., file://)
    let (st_bad_url, _) = app
        .request(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{session_id}/messages/media"))
                .header(header::AUTHORIZATION, app.auth_header())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "to": "5511977776666",
                        "media_type": "image",
                        "media_url": "file:///etc/passwd"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await;
    assert_eq!(st_bad_url, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn v040_phase1_and_phase2_endpoints_work_end_to_end() {
    let app = TestHarness::new(false).await;

    let (st, sess) = app
        .request(
            Request::builder()
                .method("POST")
                .uri("/v1/sessions")
                .header(header::AUTHORIZATION, app.auth_header())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "name": "v040-test" }).to_string()))
                .unwrap(),
        )
        .await;
    assert_eq!(st, StatusCode::CREATED);
    let sid = sess["id"].as_str().unwrap().to_string();
    app.engine
        .simulate_connected(&sid, "551100000002", "551100000002@s.whatsapp.net")
        .await;
    tokio::time::sleep(std::time::Duration::from_millis(40)).await;

    // 1. Send text with reply_to_message_id
    let (st, msg) = app
        .request(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{sid}/messages/text"))
                .header(header::AUTHORIZATION, app.auth_header())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "to": "5511988887777",
                        "text": "Respondendo sua mensagem!",
                        "reply_to_message_id": "wamid.orig_1"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await;
    assert_eq!(st, StatusCode::CREATED);
    let ext_id = msg["external_id"].as_str().unwrap().to_string();

    // 2. Reaction
    let (st, react) = app
        .request(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{sid}/messages/reaction"))
                .header(header::AUTHORIZATION, app.auth_header())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "chat_jid": "5511988887777@s.whatsapp.net",
                        "message_id": ext_id,
                        "emoji": "👍"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await;
    assert_eq!(st, StatusCode::CREATED);
    assert_eq!(react["emoji"], "👍");

    // 3. Chat presence (composing / recording / paused)
    let (st, pres) = app
        .request(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{sid}/chats/presence"))
                .header(header::AUTHORIZATION, app.auth_header())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "chat_jid": "5511988887777@s.whatsapp.net",
                        "state": "composing"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(pres["state"], "composing");

    // 4. Send location
    let (st, loc) = app
        .request(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{sid}/messages/location"))
                .header(header::AUTHORIZATION, app.auth_header())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "to": "5511988887777",
                        "latitude": -23.55052,
                        "longitude": -46.63331,
                        "name": "Praça da Sé"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await;
    assert_eq!(st, StatusCode::CREATED);
    assert_eq!(loc["message_type"], "location");

    // 5. Send contact
    let (st, ctc) = app
        .request(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{sid}/messages/contact"))
                .header(header::AUTHORIZATION, app.auth_header())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "to": "5511988887777",
                        "display_name": "Suporte Softagon",
                        "phone_number": "5511999990000"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await;
    assert_eq!(st, StatusCode::CREATED);
    assert_eq!(ctc["message_type"], "contact");

    // 6. Send voice note (PTT) + sticker + download via GET /v1/media/{media_id}
    let (st, voice) = app
        .request(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{sid}/messages/media"))
                .header(header::AUTHORIZATION, app.auth_header())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "to": "5511988887777",
                        "media_type": "audio",
                        "as_voice_note": true,
                        "data_base64": "T2dnUw=="
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await;
    assert_eq!(st, StatusCode::CREATED);
    let media_id = voice["media_id"].as_str().unwrap().to_string();

    let resp = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/media/{media_id}"))
                .header(header::AUTHORIZATION, app.auth_header())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let downloaded = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&downloaded[..], b"OggS");

    let (st, sticker) = app
        .request(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{sid}/messages/media"))
                .header(header::AUTHORIZATION, app.auth_header())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "to": "5511988887777",
                        "media_type": "sticker",
                        "data_base64": "UklGRg=="
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await;
    assert_eq!(st, StatusCode::CREATED);
    assert_eq!(sticker["message_type"], "sticker");

    // 7. Edit & revoke message
    let (st, edit) = app
        .request(
            Request::builder()
                .method("PATCH")
                .uri(format!("/v1/sessions/{sid}/messages/{ext_id}"))
                .header(header::AUTHORIZATION, app.auth_header())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "chat_jid": "5511988887777@s.whatsapp.net",
                        "text": "Mensagem corrigida!"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(edit["status"], "edited");

    let (st, rev) = app
        .request(
            Request::builder()
                .method("DELETE")
                .uri(format!(
                    "/v1/sessions/{sid}/messages/{ext_id}?chat_jid=5511988887777@s.whatsapp.net"
                ))
                .header(header::AUTHORIZATION, app.auth_header())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(rev["status"], "revoked");

    // 8. Check WhatsApp, profile picture, groups
    let (st, chk) = app
        .request(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{sid}/contacts/check"))
                .header(header::AUTHORIZATION, app.auth_header())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({ "phones": ["5511988887777"] }).to_string(),
                ))
                .unwrap(),
        )
        .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(chk["results"][0]["is_registered"], true);

    let (st, pic) = app
        .request(
            Request::builder()
                .uri(format!(
                    "/v1/sessions/{sid}/contacts/5511988887777@s.whatsapp.net/profile-picture?preview=true"
                ))
                .header(header::AUTHORIZATION, app.auth_header())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(st, StatusCode::OK);
    assert!(pic.get("picture").is_some());

    let (st, groups) = app
        .request(
            Request::builder()
                .uri(format!("/v1/sessions/{sid}/groups"))
                .header(header::AUTHORIZATION, app.auth_header())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(st, StatusCode::OK);
    assert!(groups["groups"].is_array());

    // 9. Prometheus metrics endpoint
    let resp = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/metrics")
                .header(header::AUTHORIZATION, app.auth_header())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let metrics_text = String::from_utf8(
        axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(metrics_text.contains("softagon_relay_sessions_total 1"));
    assert!(metrics_text.contains("softagon_relay_sessions_connected 1"));
    assert!(metrics_text.contains("softagon_relay_messages_total"));
    assert!(metrics_text.contains("softagon_relay_media_storage_bytes"));
}
