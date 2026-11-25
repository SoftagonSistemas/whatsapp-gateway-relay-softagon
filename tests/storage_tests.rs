use std::time::Duration;

use bytes::Bytes;
use tempfile::TempDir;

use softagon_relay::config::{
    AppConfig, JournalMode, MediaBackendConfig, MediaConfig, SqliteConfig,
};
use softagon_relay::storage::database::Database;
use softagon_relay::storage::media::{
    MediaStorage, ObjectStoreMediaStorage, build_media_object_key,
};

#[tokio::test]
async fn sqlite_wal_migrations_and_job_recovery() {
    let dir = TempDir::new().unwrap();
    let cfg = SqliteConfig {
        relay_database_path: dir.path().join("relay.db"),
        whatsapp_database_path: dir.path().join("wa.db"),
        busy_timeout_ms: 5000,
        journal_mode: JournalMode::Wal,
    };

    let db = Database::connect(&cfg).await.unwrap();
    db.health_check().await.unwrap();

    let (mode,): (String,) = sqlx::query_as("PRAGMA journal_mode;")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(mode.to_ascii_lowercase(), "wal");

    let sess = db
        .create_session("sess_1", "main", "connected", Some("pt"))
        .await
        .unwrap();
    assert_eq!(sess.name, "main");
}

#[tokio::test]
async fn local_media_storage_put_get_delete_and_checksum() {
    let dir = TempDir::new().unwrap();
    let cfg = MediaConfig {
        backend: MediaBackendConfig::Local {
            path: dir.path().join("media"),
        },
    };
    let storage = ObjectStoreMediaStorage::from_config(&cfg).unwrap();
    assert_eq!(storage.backend_name(), "local");

    let key = build_media_object_key("sess_1", "med_1", Some("ogg"));
    let payload = Bytes::from_static(b"hello-media-bytes");
    let meta = storage.put(&key, payload.clone()).await.unwrap();
    assert_eq!(meta.size_bytes, payload.len() as i64);
    assert_eq!(meta.sha256.len(), 64);

    let fetched = storage.get(&meta.object_key).await.unwrap();
    assert_eq!(fetched, payload);

    assert!(
        storage
            .presigned_get_url(&meta.object_key, Duration::from_secs(60))
            .await
            .unwrap()
            .is_none()
    );

    storage.delete(&meta.object_key).await.unwrap();
    assert!(storage.get(&meta.object_key).await.is_err());
}

#[test]
fn config_rejects_placeholder_api_key_and_invalid_s3() {
    let err = AppConfig::from_lookup(|k| match k {
        "RELAY_API_KEY" => Some("change-this-before-starting".to_string()),
        _ => None,
    });
    assert!(err.is_err());

    let err_readme_placeholder = AppConfig::from_lookup(|k| match k {
        "RELAY_API_KEY" => Some("change-me-to-a-strong-secret".to_string()),
        _ => None,
    });
    assert!(err_readme_placeholder.is_err());

    let err_short = AppConfig::from_lookup(|k| match k {
        "RELAY_API_KEY" => Some("short-key".to_string()),
        _ => None,
    });
    assert!(err_short.is_err());

    let err_unexpanded_var = AppConfig::from_lookup(|k| match k {
        "RELAY_API_KEY" => Some("${RELAY_API_KEY}".to_string()),
        _ => None,
    });
    assert!(err_unexpanded_var.is_err());

    let err_angle_placeholder = AppConfig::from_lookup(|k| match k {
        "RELAY_API_KEY" => Some("<paste-your-generated-key>".to_string()),
        _ => None,
    });
    assert!(err_angle_placeholder.is_err());

    let err_s3 = AppConfig::from_lookup(|k| match k {
        "RELAY_API_KEY" => Some("strong-secret-key".to_string()),
        "MEDIA_STORAGE" => Some("s3".to_string()),
        _ => None,
    });
    assert!(err_s3.is_err());
}
