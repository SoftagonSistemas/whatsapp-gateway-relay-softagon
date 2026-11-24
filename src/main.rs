use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::sync::{Notify, watch};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use softagon_relay::api::{AppState, build_router};
use softagon_relay::config::AppConfig;
use softagon_relay::core::events::EventBus;
use softagon_relay::core::queue::TranscriptionQueue;
use softagon_relay::core::sessions::SessionManager;
use softagon_relay::core::webhooks::{WebhookDispatcher, WebhookService};
use softagon_relay::engines::whatsapp::WhatsAppEngine;
use softagon_relay::engines::whatsapp::whatsapp_rust_adapter::WhatsAppRustEngine;
use softagon_relay::storage::database::Database;
use softagon_relay::storage::media::{MediaStorage, ObjectStoreMediaStorage};
use softagon_relay::transcription::whisper::build_transcription_engine;
use softagon_relay::transcription::worker::TranscriptionWorkerPool;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .json()
        .init();

    let config = AppConfig::from_env()?;
    config.ensure_directories()?;
    let bind_addr = config.bind_addr()?;

    let db = Database::connect(&config.sqlite).await?;
    let media: Arc<dyn MediaStorage> =
        Arc::new(ObjectStoreMediaStorage::from_config(&config.media)?);
    let wa_engine: Arc<dyn WhatsAppEngine> = Arc::new(WhatsAppRustEngine::new(
        config.sqlite.whatsapp_database_path.clone(),
    ));
    let tx_engine = build_transcription_engine(&config.transcription);

    let webhook_notify = Arc::new(Notify::new());
    let tx_notify = Arc::new(Notify::new());

    let event_bus = EventBus::new(
        db.clone(),
        webhook_notify.clone(),
        config.webhooks.max_attempts,
    );
    let tx_queue = TranscriptionQueue::new(db.clone(), config.transcription.max_queue, tx_notify);
    let recovered = tx_queue.recover_stale_jobs().await?;
    if recovered > 0 {
        info!(count = recovered, "recovered stale transcription jobs");
    }

    let (sessions, event_rx) = SessionManager::new(
        db.clone(),
        media.clone(),
        wa_engine,
        event_bus.clone(),
        tx_queue.clone(),
        config.clone(),
    );

    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let event_loop_handle = sessions
        .clone()
        .spawn_event_loop(event_rx, shutdown_rx.clone());

    let webhook_handle =
        WebhookDispatcher::new(db.clone(), webhook_notify, config.webhooks.clone())
            .spawn(shutdown_rx.clone());

    let tx_handles = if config.transcription.enabled {
        TranscriptionWorkerPool::new(
            db.clone(),
            media.clone(),
            tx_queue,
            tx_engine.clone(),
            event_bus,
            config.transcription.clone(),
            config.tmp_dir.clone(),
        )
        .spawn(shutdown_rx.clone())
    } else {
        Vec::new()
    };

    match sessions.restore_sessions().await {
        Ok(restored) => info!(count = restored, "restored persisted whatsapp sessions"),
        Err(err) => warn!(error = %err, "failed to restore persisted whatsapp sessions"),
    }

    let state = AppState {
        config: config.clone(),
        db: db.clone(),
        media,
        sessions,
        webhooks: WebhookService::new(db),
        transcription_engine: tx_engine,
    };

    let router = build_router(state);
    let listener = TcpListener::bind(bind_addr).await?;
    info!(address = %bind_addr, "softagon-relay listening");

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    let _ = shutdown_tx.send(true);
    let _ = event_loop_handle.await;
    let _ = webhook_handle.await;
    for h in tx_handles {
        let _ = h.await;
    }

    info!("softagon-relay stopped cleanly");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sig) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            sig.recv().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    info!("received shutdown signal");
}
