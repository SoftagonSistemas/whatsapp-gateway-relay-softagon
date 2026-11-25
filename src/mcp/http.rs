use std::sync::Arc;

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use subtle::ConstantTimeEq;
use tokio_util::sync::CancellationToken;

use crate::core::service::RelayService;
use crate::mcp::server::RelayMcpServer;

#[derive(Clone)]
struct McpSecurityState {
    api_key: String,
    allowed_origins: Vec<String>,
}

/// Build the Streamable HTTP MCP router mounted at `/mcp`.
///
/// Enforces:
/// 1. Loopback-only `Host` validation (anti-DNS rebinding via `rmcp` + middleware)
/// 2. Strict `Origin` validation (loopback origins + optional `MCP_ALLOWED_ORIGINS`)
/// 3. Constant-time `Authorization: Bearer <RELAY_API_KEY>` authentication
pub fn build_mcp_http_router(service: RelayService, ct: CancellationToken) -> Router {
    let mut allowed_origins = vec![
        "http://127.0.0.1:*".to_string(),
        "http://localhost:*".to_string(),
        "http://[::1]:*".to_string(),
    ];
    for extra in &service.config.mcp.allowed_origins {
        if !allowed_origins.contains(extra) {
            allowed_origins.push(extra.clone());
        }
    }

    let security_state = McpSecurityState {
        api_key: service.config.api_key.clone(),
        allowed_origins: allowed_origins.clone(),
    };

    let mut cfg = StreamableHttpServerConfig::default();
    cfg.json_response = true;
    cfg.legacy_session_mode = false;
    cfg.cancellation_token = ct;
    cfg.allowed_hosts = vec![
        "127.0.0.1".to_string(),
        "localhost".to_string(),
        "[::1]".to_string(),
    ];
    cfg.allowed_origins = allowed_origins;

    let svc_for_factory = service.clone();
    let mcp_service = StreamableHttpService::new(
        move || {
            Ok(RelayMcpServer::new(
                svc_for_factory.clone(),
                "streamable-http",
            ))
        },
        Arc::new(LocalSessionManager::default()),
        cfg,
    );

    Router::new()
        .nest_service("/mcp", mcp_service)
        .route_layer(middleware::from_fn_with_state(
            security_state,
            mcp_security_middleware,
        ))
}

async fn mcp_security_middleware(
    State(state): State<McpSecurityState>,
    req: Request,
    next: Next,
) -> Response {
    if !validate_origin(req.headers(), &state.allowed_origins) {
        return (
            StatusCode::FORBIDDEN,
            "forbidden: Origin not allowed for local MCP endpoint",
        )
            .into_response();
    }

    if !validate_bearer(req.headers(), &state.api_key) {
        return (
            StatusCode::UNAUTHORIZED,
            "unauthorized: valid Authorization: Bearer <RELAY_API_KEY> required",
        )
            .into_response();
    }

    next.run(req).await
}

pub fn validate_origin(headers: &HeaderMap, allowed_origins: &[String]) -> bool {
    let Some(raw) = headers.get(header::ORIGIN) else {
        // Non-browser local MCP clients (CLI, desktop agents) do not send Origin.
        return true;
    };
    let Ok(origin) = raw.to_str() else {
        return false;
    };
    let origin = origin.trim();
    if origin.is_empty() {
        return false;
    }

    allowed_origins.iter().any(|pattern| {
        if let Some(prefix) = pattern.strip_suffix(":*") {
            origin == prefix
                || origin.strip_prefix(prefix).is_some_and(|rest| {
                    rest.starts_with(':') && rest[1..].chars().all(|c| c.is_ascii_digit())
                })
        } else {
            origin.eq_ignore_ascii_case(pattern)
        }
    })
}

fn validate_bearer(headers: &HeaderMap, expected_key: &str) -> bool {
    let Some(raw) = headers.get(header::AUTHORIZATION) else {
        return false;
    };
    let Ok(val) = raw.to_str() else {
        return false;
    };
    let Some(token) = val.strip_prefix("Bearer ").map(str::trim) else {
        return false;
    };
    token.as_bytes().ct_eq(expected_key.as_bytes()).into()
}
