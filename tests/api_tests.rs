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
    AppConfig, JournalMode, MediaBackendConfig, MediaConfig, SqliteConfig, TranscriptionConfig,
    WebhookConfig, WhisperModel,
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
            },
            webhooks: WebhookConfig {
                delivery_timeout_seconds: 5,
                max_attempts: 3,
                backoff_base_seconds: 1,
            },
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
