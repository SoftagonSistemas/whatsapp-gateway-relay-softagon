pub mod http;
pub mod server;

use rmcp::ServiceExt;

use crate::core::service::RelayService;
use crate::mcp::server::RelayMcpServer;

/// Run the MCP server over STDIO (default local integration transport).
///
/// Callers MUST ensure `tracing` logs are routed to `stderr` so `stdout`
/// remains a clean JSON-RPC stream.
pub async fn run_stdio(service: RelayService) -> anyhow::Result<()> {
    let server = RelayMcpServer::new(service, "stdio");
    let transport = rmcp::transport::io::stdio_pipes();
    let running = server.serve(transport).await?;
    running.waiting().await?;
    Ok(())
}
