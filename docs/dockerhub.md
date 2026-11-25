# Softagon Relay — Self-Hosted WhatsApp API Gateway & Local MCP Server in Rust

**Softagon Relay** (`softagon/relay`) is an open-source, self-hosted **WhatsApp Web HTTP API gateway, embedded Web Console, and native MCP Server** built in **Rust** by **[Softagon Sistemas](https://softagon.com.br)**. It runs as a single **~55 MB Docker container** with embedded **SQLite (WAL)**, optional **S3-compatible media storage** (AWS S3, Cloudflare R2, MinIO), **HMAC-SHA256 signed webhooks** with persistent outbox retries, **OpenAPI 3.2.1** (`/openapi.json`, `/openapi.yaml`, `/docs`), and **offline multilingual voice-note transcription** powered by `whisper.cpp`.

- **Docker Image:** `softagon/relay:latest` (`linux/amd64`, non-root UID `10001`)
- **Official Website & Contact:** https://softagon.com.br
- **Source Code & Changelog (GitHub):** https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon · [CHANGELOG.md](https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/blob/main/CHANGELOG.md)
- **User Documentation (with screenshots):** [Português (BR)](https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/blob/main/docs/pt-BR/README.md) · [English](https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/blob/main/docs/en/README.md) · [Español](https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/blob/main/docs/es/README.md) · [Français](https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/blob/main/docs/fr/README.md)
- **Web Console & API Contract:** `/console` (or `/manager`) · OpenAPI 3.2.1 (`/openapi.json` & `/openapi.yaml`) · Scalar Interactive Docs (`/docs`)
- **License:** Apache-2.0

---

## Quick Start (One-Line Docker Run)

```bash
docker run -d --name softagon-relay --restart unless-stopped -p 8080:8080 -v relay-data:/data -e RELAY_API_KEY="replace-with-a-strong-random-key" softagon/relay:latest
```

Open **`http://localhost:8080/console`** and sign in with your `RELAY_API_KEY`, or check health:

```bash
curl http://localhost:8080/v1/health
```

---

## Measured Container Footprint (Real Scenarios)

Measurements recorded on `softagon/relay:0.3.0` (`linux/amd64`, compiled with `--features whisper`, Docker 29.1.3, Linux host):

| Scenario | Measured Value | Notes |
|---|---|---|
| **Image Size (`linux/amd64`)** | **55.0 MB** | Includes Rust binary, Vue 3 Console, SQLite, S3 client, Opus decoder, `rmcp`, and `whisper.cpp` |
| **Boot / Idle (0 paired sessions)** | **~4–5 MiB RAM** | Single Tokio/Axum process before connecting any WhatsApp session |
| **1 Paired WhatsApp Session (steady state)** | **~38 MiB RAM · ~0.1–0.2% CPU** | Live WhatsApp WebSocket connected, receiving & sending messages |
| **During Whisper `base` Transcription** | **~212 MiB RAM** | Model loads lazily on first voice note and unloads automatically after `TRANSCRIPTION_IDLE_UNLOAD_SECONDS` (default: 180s) |
| **External Databases Required** | **0** | Embedded SQLite (WAL); S3/R2/MinIO is optional |

---

## Hardened `compose.yaml` (Recommended for Production)

```yaml
services:
  relay:
    image: softagon/relay:0.3.0
    container_name: softagon-relay
    restart: unless-stopped
    ports:
      - "8080:8080"
    volumes:
      - relay-data:/data
    environment:
      RELAY_API_KEY: "replace-with-a-strong-random-key"
      RUST_LOG: "info"
      DOCS_ENABLED: "true"
      DOCS_PUBLIC: "false"
      MEDIA_STORAGE: "local"
      TRANSCRIPTION_ENABLED: "true"
      TRANSCRIPTION_MODEL: "base"
      TRANSCRIPTION_LANGUAGE: "auto"
      TRANSCRIPTION_PRELOAD: "false"
      TRANSCRIPTION_IDLE_UNLOAD_SECONDS: "180"
    read_only: true
    tmpfs:
      - /tmp:size=128m,noexec,nosuid
    security_opt:
      - no-new-privileges:true
    cap_drop:
      - ALL

volumes:
  relay-data:
```

> **Permissions note:** The container runs as non-root user `relay` (UID `10001`). Named Docker volumes work automatically. If you use a host bind mount, make sure UID `10001` has write permissions on `/data`.

---

## Supported Tags

| Tag | Architecture | Description |
|---|---|---|
| `latest` | `linux/amd64` | Latest stable release (`0.3.0`) |
| `0.3` | `linux/amd64` | Latest `0.3.x` release |
| `0.3.0` | `linux/amd64` | Native MCP Server (STDIO + loopback Streamable HTTP), `/console`, `/openapi.json` |

> *Multi-arch note:* Prebuilt images currently target `linux/amd64`. `linux/arm64` builds are on our roadmap.

---

## REST API (`/v1`) & Native MCP Server

All `/v1/*` routes except `GET /v1/health` require `Authorization: Bearer <RELAY_API_KEY>`:

| Method | Endpoint | Purpose |
|---|---|---|
| `GET` | `/v1/health` | Public liveness & readiness probe |
| `GET` / `POST` | `/v1/sessions` | List or create WhatsApp sessions |
| `GET` / `DELETE` | `/v1/sessions/{session_id}` | Get metadata or delete disconnected session (`409` if connected) |
| `GET` | `/v1/sessions/{session_id}/qr` · `/qr.svg` | Get pairing QR code (JSON or SVG) |
| `GET` | `/v1/sessions/{session_id}/status` | Get live connection state |
| `POST` | `/v1/sessions/{session_id}/reconnect` | Restart and start a fresh QR cycle |
| `POST` | `/v1/sessions/{session_id}/logout` | Disconnect and unlink device |
| `POST` | `/v1/sessions/{session_id}/messages/text` | Send text message (supports `Idempotency-Key` & BR/MX/AR mobile resolution) |
| `POST` | `/v1/sessions/{session_id}/messages/media` | Send image, video, audio, or document |
| `GET` | `/v1/messages/{message_id}` | Retrieve message status & metadata |
| `GET` / `POST` | `/v1/messages/{message_id}/transcription` | Fetch or retry voice-note transcription |
| `GET` / `POST` | `/v1/webhooks` | List or create webhook subscriptions |
| `GET` / `PATCH` / `DELETE` | `/v1/webhooks/{webhook_id}` | Inspect, update, or remove a webhook |
| `GET` | `/docs` · `/openapi.json` · `/openapi.yaml` | Scalar API Reference & OpenAPI 3.2.1 spec (JSON/YAML) |

### Local MCP Server (`v0.3.0`)
- **STDIO (default local mode):** `softagon-relay mcp --stdio` (logs routed to `stderr`)
- **Streamable HTTP:** `softagon-relay mcp --http` or `MCP_HTTP_ENABLED=true` (loopback-only `127.0.0.1:8091/mcp`, `Origin` validation, Bearer auth, destructive tools blocked by default, audited in SQLite `mcp_audit_log`).

---

## Environment Variables

| Variable | Default | Description |
|---|---|---|
| `RELAY_API_KEY` | *(required)* | Bearer API key. Startup fails if empty or set to the example placeholder. |
| `APP_HOST` / `APP_PORT` | `0.0.0.0` / `8080` | Bind host and port |
| `RUST_LOG` | `info` | Structured JSON log level (`error`, `warn`, `info`, `debug`) |
| `DATA_DIR` | `/data` | Persistent volume root |
| `DOCS_ENABLED` / `DOCS_PUBLIC` | `true` / `false` | Controls `/docs`, `/openapi.json`, and `/openapi.yaml` availability & auth |
| `MEDIA_MAX_UPLOAD_BYTES` | `26214400` | Max upload size (25 MiB) |
| `SESSION_RATE_LIMIT_PER_MINUTE` | `60` | Per-session outbound message rate limit |
| `SQLITE_JOURNAL_MODE` / `SQLITE_BUSY_TIMEOUT_MS` | `wal` / `5000` | SQLite WAL & lock timeout (use local disk, not NFS) |
| `MEDIA_STORAGE` | `local` | `local` (`/data/media`) or `s3` (AWS S3, Cloudflare R2, MinIO) |
| `S3_ENDPOINT_URL`, `S3_REGION`, `S3_BUCKET`, `S3_ACCESS_KEY_ID`, `S3_SECRET_ACCESS_KEY`, `S3_PREFIX` | `auto`, `softagon-relay/` | S3-compatible object storage settings (used when `MEDIA_STORAGE=s3`) |
| `TRANSCRIPTION_ENABLED` / `TRANSCRIPTION_MODEL` / `TRANSCRIPTION_LANGUAGE` | `true` / `base` / `auto` | Offline Whisper transcription settings (`tiny`, `base`, `small`, `medium`, `large`) |
| `TRANSCRIPTION_PRELOAD` / `TRANSCRIPTION_IDLE_UNLOAD_SECONDS` | `false` / `180` | Lazy-load on first audio and unload after idle timeout |
| `WEBHOOK_DELIVERY_TIMEOUT_SECONDS` / `WEBHOOK_MAX_ATTEMPTS` / `WEBHOOK_BACKOFF_BASE_SECONDS` | `10` / `8` / `2` | Webhook HTTP timeout and retry backoff policy |
| `MCP_HTTP_ENABLED` / `MCP_HTTP_HOST` / `MCP_HTTP_PORT` | `false` / `127.0.0.1` / `8091` | Enable local MCP Streamable HTTP server (`/mcp`, loopback-only) |
| `MCP_ALLOW_DESTRUCTIVE` / `MCP_ALLOWED_ORIGINS` | `false` / *(loopback)* | Allow destructive MCP tools (`logout_session`, `delete_session`) and extra Origins |

---

## Transparency, Roadmap & Disclaimer

- **AI-assisted development:** Softagon Relay is developed with AI-assisted coding under human architecture, review, security design, and live testing against real WhatsApp accounts.
- **Roadmap:** Native **Tauri Desktop app** (Windows, Linux, macOS) + **`linux/arm64`** Docker images. Want these sooner? Open an issue on GitHub: https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/issues/new
- **Unofficial project:** Softagon Relay is an independent open-source project built on `whatsapp-rust` and is not affiliated with, authorized by, or endorsed by Meta or WhatsApp. Always deploy behind TLS and protect backups of `/data`.
