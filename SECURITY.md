# Security Policy

## Supported Versions

| Version | Supported |
|---|---|
| `0.3.x` | Yes |
| `< 0.3.0` | Upgrade to latest `0.3.x` |

## Reporting a Vulnerability

If you discover a security vulnerability in **Softagon Relay**, please **do not** open a public GitHub Issue.

Instead, report it privately via:
- **GitHub Security Advisories:** <https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/security/advisories/new>
- **Contact:** <https://softagon.com.br>

Please include:
- A clear description of the vulnerability and impact
- Steps to reproduce or a minimal proof of concept
- Affected version and deployment configuration (Docker, STDIO MCP, Streamable HTTP MCP, etc.)

We will acknowledge receipt within 72 hours and coordinate a fix and disclosure timeline.

## Deployment Security Recommendations

Because Softagon Relay holds live WhatsApp Web session credentials:

1. **Strong `RELAY_API_KEY`:** Always generate a high-entropy random key (e.g. `openssl rand -hex 32`). Known placeholder strings and short keys are rejected at startup.
2. **Never expose port `8080` directly without TLS:** Run behind a TLS-terminating reverse proxy (Caddy, Traefik, Nginx) or private network/VPN (Tailscale, WireGuard).
3. **Protect `/data` as secrets:** `/data/whatsapp-sessions.db` contains WhatsApp Web encryption keys. Encrypt backups and restrict filesystem permissions (container runs as non-root UID `10001`).
4. **Keep `DOCS_PUBLIC=false` in production** unless you intentionally want `/docs`, `/openapi.json`, and `/openapi.yaml` reachable without a Bearer token.
5. **Local MCP Server safety:**
   - Prefer **STDIO** (`mcp --stdio`) for local desktop AI agents.
   - **Streamable HTTP** (`mcp --http` / `MCP_HTTP_ENABLED=true`) refuses to bind to non-loopback addresses (`0.0.0.0` is blocked), enforces `Origin` validation to mitigate DNS rebinding, and requires `Authorization: Bearer <RELAY_API_KEY>`.
   - Keep `MCP_ALLOW_DESTRUCTIVE=false` (default) so AI agents cannot unlink or delete WhatsApp sessions.
   - Review `mcp_audit_log` in `/data/relay.db` for an audit trail of every MCP tool invocation.
6. **Verify Webhook HMAC Signatures:** Always validate `X-Relay-Signature` (`HMAC-SHA256(secret, "{X-Relay-Timestamp}.{raw_body}")`) with constant-time comparison and reject stale timestamps.
