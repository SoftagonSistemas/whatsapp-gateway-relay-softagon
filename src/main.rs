use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::sync::{Notify, watch};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use softagon_relay::api::{AppState, build_router};
use softagon_relay::config::AppConfig;
use softagon_relay::core::events::EventBus;
use softagon_relay::core::queue::TranscriptionQueue;
use softagon_relay::core::service::RelayService;
use softagon_relay::core::sessions::SessionManager;
use softagon_relay::core::webhooks::{WebhookDispatcher, WebhookService};
use softagon_relay::engines::whatsapp::WhatsAppEngine;
use softagon_relay::engines::whatsapp::whatsapp_rust_adapter::WhatsAppRustEngine;
use softagon_relay::mcp;
use softagon_relay::storage::database::Database;
use softagon_relay::storage::media::{MediaStorage, ObjectStoreMediaStorage};
use softagon_relay::transcription::whisper::build_transcription_engine;
use softagon_relay::transcription::worker::TranscriptionWorkerPool;

#[derive(Debug, Clone, PartialEq, Eq)]
enum RunMode {
    Serve,
    McpStdio,
    McpHttp,
    Backup(Option<String>),
}

fn parse_cli_mode() -> anyhow::Result<RunMode> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("serve") => Ok(RunMode::Serve),
        Some("mcp") => match args.get(1).map(String::as_str) {
            None | Some("--stdio") => Ok(RunMode::McpStdio),
            Some("--http") => Ok(RunMode::McpHttp),
            Some(flag) => {
                anyhow::bail!("unknown mcp option '{flag}'; expected --stdio (default) or --http")
            }
        },
        Some("backup") => Ok(RunMode::Backup(args.get(1).cloned())),
        Some("--version" | "-V" | "version") => {
            println!("softagon-relay {}", env!("CARGO_PKG_VERSION"));
            std::process::exit(0);
        }
        Some("--help" | "-h" | "help") => {
            eprintln!(
                "Softagon Relay v{}\n\nUSAGE:\n  softagon-relay [serve]          Run REST API + Web Console (and optional MCP HTTP if MCP_HTTP_ENABLED=true)\n  softagon-relay mcp [--stdio]    Run local MCP server over STDIO (default local mode)\n  softagon-relay mcp --http       Run local MCP Streamable HTTP server on 127.0.0.1\n  softagon-relay backup [path]    Create a hot SQLite online backup of relay.db (WAL-safe)\n  softagon-relay --version        Print version information",
                env!("CARGO_PKG_VERSION")
            );
            std::process::exit(0);
        }
        Some(other) => {
            anyhow::bail!("unknown subcommand '{other}'; expected 'serve', 'mcp', or 'backup'")
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mode = parse_cli_mode()?;

    // In STDIO MCP mode, stdout is exclusively reserved for JSON-RPC frames.
    // Always route structured logs to stderr in MCP STDIO mode.
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    if mode == RunMode::McpStdio {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .json()
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .json()
            .init();
    }

    let config = AppConfig::from_env()?;
    config.ensure_directories()?;

    let db = Database::connect(&config.sqlite).await?;

    if let RunMode::Backup(dest_opt) = mode {
        let dest = dest_opt.unwrap_or_else(|| {
            let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S");
            config
                .data_dir
                .join(format!("relay-backup-{ts}.db"))
                .to_string_lossy()
                .to_string()
        });
        if std::path::Path::new(&dest).exists() {
            let _ = std::fs::remove_file(&dest);
        }
        sqlx::query("VACUUM INTO ?1")
            .bind(&dest)
            .execute(db.pool())
            .await?;
        println!("Backup created at {dest}");
        return Ok(());
    }
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

    // Only the long-running HTTP daemon (`serve`) should run background webhook delivery
    // and transcription worker pools. In `mcp --stdio` / `mcp --http` standalone modes,
    // skipping these background workers prevents duplicate webhook deliveries and SQLite
    // write contention if a gateway container is already running.
    let run_background_workers = mode == RunMode::Serve;

    let event_loop_handle = sessions
        .clone()
        .spawn_event_loop(event_rx, shutdown_rx.clone());

    let webhook_handle = if run_background_workers {
        Some(
            WebhookDispatcher::new(db.clone(), webhook_notify, config.webhooks.clone())
                .spawn(shutdown_rx.clone()),
        )
    } else {
        None
    };

    let tx_handles = if run_background_workers && config.transcription.enabled {
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

    let _retention_handle = if run_background_workers && config.media_retention_days > 0 {
        let db_ret = db.clone();
        let media_ret = media.clone();
        let days = config.media_retention_days;
        let mut sd = shutdown_rx.clone();
        Some(tokio::spawn(async move {
            info!(retention_days = days, "media retention cleaner started");
            loop {
                let cutoff =
                    (chrono::Utc::now() - chrono::Duration::days(days as i64)).to_rfc3339();
                match db_ret.list_expired_media_objects(&cutoff, 100).await {
                    Ok(expired) => {
                        let expired_found = expired.len();
                        let mut purged = 0usize;
                        for item in expired {
                            let _ = media_ret.delete(&item.object_key).await;
                            if db_ret.mark_media_purged(&item.id).await.is_ok() {
                                purged += 1;
                            }
                        }
                        info!(
                            retention_days = days,
                            expired_found, purged, "media retention cleanup sweep completed"
                        );
                    }
                    Err(err) => {
                        warn!(
                            retention_days = days,
                            error = %err,
                            "media retention cleanup sweep failed to query expired media"
                        );
                    }
                }
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_secs(3600)) => {}
                    _ = sd.changed() => {
                        if *sd.borrow() {
                            break;
                        }
                    }
                }
            }
        }))
    } else {
        None
    };

    // In `mcp --stdio` mode, only restore WhatsApp WebSocket sessions when explicitly running
    // standalone (MCP_RESTORE_SESSIONS=true). By default `mcp --stdio` avoids kicking an active
    // `serve` container off WhatsApp Web.
    let should_restore_sessions = match mode {
        RunMode::Serve | RunMode::McpHttp => true,
        RunMode::McpStdio => std::env::var("MCP_RESTORE_SESSIONS")
            .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes"))
            .unwrap_or(false),
        RunMode::Backup(_) => false,
    };

    if should_restore_sessions {
        match sessions.restore_sessions().await {
            Ok(restored) => info!(count = restored, "restored persisted whatsapp sessions"),
            Err(err) => warn!(error = %err, "failed to restore persisted whatsapp sessions"),
        }
    } else {
        info!(
            "skipping session restore in mcp --stdio mode (set MCP_RESTORE_SESSIONS=true for standalone STDIO without a running server, or use MCP_HTTP_ENABLED=true inside the server container)"
        );
    }

    let service = RelayService::new(
        config.clone(),
        db.clone(),
        media,
        sessions,
        WebhookService::new(db),
        tx_engine,
    );

    match mode {
        RunMode::McpStdio => {
            info!("softagon-relay MCP server starting on STDIO");
            mcp::run_stdio(service).await?;
        }
        RunMode::McpHttp => {
            let mcp_addr = config.mcp.bind_addr()?;
            let ct = CancellationToken::new();
            let mcp_router = mcp::http::build_mcp_http_router(service, ct.clone());
            let listener = TcpListener::bind(mcp_addr).await?;
            info!(address = %mcp_addr, "softagon-relay MCP Streamable HTTP listening at /mcp");
            let ct_shutdown = ct.clone();
            axum::serve(listener, mcp_router)
                .with_graceful_shutdown(async move {
                    shutdown_signal().await;
                    ct_shutdown.cancel();
                })
                .await?;
        }
        RunMode::Serve => {
            let bind_addr = config.bind_addr()?;
            let mcp_ct = CancellationToken::new();
            let mcp_handle = if config.mcp.http_enabled {
                let mcp_addr = config.mcp.bind_addr()?;
                let mcp_router = mcp::http::build_mcp_http_router(service.clone(), mcp_ct.clone());
                let mcp_listener = TcpListener::bind(mcp_addr).await?;
                info!(address = %mcp_addr, "softagon-relay MCP Streamable HTTP listening at /mcp");
                let ct_inner = mcp_ct.clone();
                Some(tokio::spawn(async move {
                    let _ = axum::serve(mcp_listener, mcp_router)
                        .with_graceful_shutdown(async move {
                            ct_inner.cancelled().await;
                        })
                        .await;
                }))
            } else {
                None
            };

            let state = AppState::from(service);
            let router = build_router(state);
            let listener = TcpListener::bind(bind_addr).await?;
            info!(address = %bind_addr, "softagon-relay listening");

            axum::serve(listener, router)
                .with_graceful_shutdown(shutdown_signal())
                .await?;

            mcp_ct.cancel();
            if let Some(h) = mcp_handle {
                let _ = h.await;
            }
        }
        RunMode::Backup(_) => unreachable!(),
    }

    let _ = shutdown_tx.send(true);
    let _ = event_loop_handle.await;
    if let Some(h) = webhook_handle {
        let _ = h.await;
    }
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
