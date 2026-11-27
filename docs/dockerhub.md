# Softagon Relay — Self-Hosted WhatsApp API Gateway & Local MCP Server in Rust

**Softagon Relay** (`softagon/relay`) is an open-source, self-hosted **WhatsApp Web HTTP API gateway, 10-language embedded Web Console, and native MCP Server** built in **Rust** by **[Softagon Sistemas](https://softagon.com.br)**. It runs as a single **~55 MB Docker container** with embedded **SQLite (WAL)**, optional **S3-compatible media storage** (AWS S3, Cloudflare R2, MinIO), **HMAC-SHA256 signed webhooks** with persistent outbox retries, **OpenAPI 3.2.1** (`/openapi.json`, `/openapi.yaml`, `/docs`), and **offline multilingual voice-note transcription** powered by `whisper.cpp`.

- **Docker Image:** `softagon/relay:latest` (`linux/amd64`, non-root UID `10001`)
- **Official Website & Contact:** https://softagon.com.br
- **Source Code & Changelog (GitHub):** https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon · [CHANGELOG.md](https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/blob/main/CHANGELOG.md)
- **User Documentation (with screenshots):** [Português (BR)](https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/blob/main/docs/pt-BR/README.md) · [English](https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/blob/main/docs/en/README.md) · [Español](https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/blob/main/docs/es/README.md) · [Français](https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/blob/main/docs/fr/README.md) · [Bahasa Indonesia](https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/blob/main/docs/id/README.md) · [Basa Betawi](https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/blob/main/docs/bew/README.md) · [Basa Sunda](https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/blob/main/docs/su/README.md) · [Basa Jawa](https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/blob/main/docs/jv/README.md) · [Hata Batak Toba](https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/blob/main/docs/bbc/README.md) · [Bahasa Melayu](https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/blob/main/docs/ms/README.md)
- **Grok Bot Integration & Reusable Template:** [Grok Bot Guide](https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/blob/main/docs/grok-bot.md) · [Grok Bot Template (`TEMPLATE.md`)](https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/blob/main/integrations/grok-bot/TEMPLATE.md) (zero-login `/pair/{token}` auto-renewing QR pairing, Bearer webhooks, `@lid` resolution, `mark_read`, and `/workspace` persistence)
- **Web Console & API Contract:** `/console` (or `/manager`) · OpenAPI 3.2.1 (`/openapi.json` & `/openapi.yaml`) · Scalar Interactive Docs (`/docs`)
- **License:** Apache-2.0

---

## Quick Start (One-Line Docker Run)

```bash
KEY="$(openssl rand -hex 32)" && echo "RELAY_API_KEY=$KEY" && docker run -d --name softagon-relay --restart unless-stopped -p 127.0.0.1:8080:8080 -v relay-data:/data -e RELAY_API_KEY="$KEY" softagon/relay:latest
```

> Binds to `127.0.0.1:8080` and generates a random 256-bit key. Known placeholder keys and keys shorter than 16 characters are rejected at startup.

Open **`http://localhost:8080/console`** and sign in with the printed `RELAY_API_KEY`, or check health:

```bash
curl http://localhost:8080/v1/health
```

---

## Measured Container Footprint (Real Scenarios)

Measurements recorded on **2026-11-25** against `softagon/relay:0.3.0` (`linux/amd64`, compiled with `--features whisper`) using `docker stats --no-stream` on Docker 29.1.3 / Linux (x86_64):

| Scenario | Measured Value | Notes |
|---|---|---|
| **Image Size (`linux/amd64`)** | **~55 MB compressed (Docker Hub)** | Rust binary, Vue 3 Console, SQLite, S3 client, Opus decoder, `rmcp`, and `whisper.cpp` |
| **Boot / Idle (0 paired sessions)** | **~4–5 MiB RAM** | Right after startup, before pairing any WhatsApp session |
| **1 Paired WhatsApp Session (steady state)** | **~38 MiB RAM · ~0.1–0.2% CPU** | 1 real WhatsApp account connected via WebSocket |
| **During Whisper `base` Transcription** | **~212–338 MiB RAM** | Model loads lazily on incoming voice note and unloads after `180s` idle (~100 MiB RSS after unload) |
| **External Databases Required** | **0** | Embedded SQLite (WAL); S3/R2/MinIO is optional |

---

## Hardened `compose.yaml` (Recommended for Production)

```yaml
services:
  relay:
    image: softagon/relay:0.3.8
    container_name: softagon-relay
    restart: unless-stopped
    ports:
      - "127.0.0.1:8080:8080"
    volumes:
      - relay-data:/data
    environment:
      RELAY_API_KEY: "${RELAY_API_KEY:?Generate with: openssl rand -hex 32}"
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
| `latest` | `linux/amd64` | Latest stable release (`0.3.8`) |
| `0.3` | `linux/amd64` | Latest `0.3.x` release |
| `0.3.8` | `linux/amd64` | Complete Grok Bot kit: `mark_read`, `get_contact` (`@lid` resolution), `media_url` + MCP `send_media_message`, live `/v1/health`, 10-locale docs |
| `0.3.7` | `linux/amd64` | Zero-login `/pair/{token}` auto-renewing QR, `qr.png`, 8-digit pairing code, webhook `bearer_token`/custom auth, `sender_name`, `from_me` phone sync, `is_history_sync` suppression |
| `0.3.6` | `linux/amd64` | Chrome Modern Web Guidance & WebMCP (`document.modelContext` + declarative agentic forms), `--version` CLI flag |

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

### Local MCP Server & Grok Bot Template
- **STDIO (default local mode):** `softagon-relay mcp --stdio` (logs routed to `stderr`)
- **Streamable HTTP:** `softagon-relay mcp --http` or `MCP_HTTP_ENABLED=true` (binds to `127.0.0.1:8091/mcp` by default; in Docker behind `-p 127.0.0.1:8091:8091`, opt in with `MCP_HTTP_HOST=0.0.0.0` + `MCP_ALLOW_CONTAINER_BIND=true`; always enforces loopback `Host`/`Origin` validation, Bearer auth, destructive tools blocked by default, and SQLite audit in `mcp_audit_log`).
- **Grok Bot Turnkey Template:** copy our ready-to-use **[Grok Bot Template (`TEMPLATE.md`)](https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/blob/main/integrations/grok-bot/TEMPLATE.md)** and follow the **[Grok Bot Guide](https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/blob/main/docs/grok-bot.md)** for zero-login `/pair/{token}` QR pairing, `bearer_token` routine webhooks, `get_contact` (`@lid` resolution), `mark_read`, and `send_media_message` (`media_url` or base64).

---

## Environment Variables

| Variable | Default | Description |
|---|---|---|
| `RELAY_API_KEY` | *(required)* | Bearer API key. Startup fails if empty, shorter than 16 characters, a known placeholder, or an unexpanded `${…}`/`<…>` template. Generate with `openssl rand -hex 32`. |
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
| `MCP_HTTP_ENABLED` / `MCP_HTTP_HOST` / `MCP_HTTP_PORT` | `false` / `127.0.0.1` / `8091` | Enable local MCP Streamable HTTP server (`/mcp`, loopback-only by default) |
| `MCP_ALLOW_CONTAINER_BIND` / `MCP_ALLOW_DESTRUCTIVE` / `MCP_ALLOWED_ORIGINS` | `false` / `false` / *(loopback)* | Opt in to `0.0.0.0` bind inside Docker behind `-p 127.0.0.1:8091:8091`, destructive tools, and extra Origins |

---

## Frequently Asked Questions (FAQ)

- **Why Rust instead of Chromium / Puppeteer / Node.js?** Softagon Relay talks directly to WhatsApp Web over WebSocket (`whatsapp-rust`), keeping steady-state usage at **~38 MiB RAM** and **~55 MB** compressed Docker image size with zero external databases (PostgreSQL/Redis/RabbitMQ) or FFmpeg.
- **Which languages are supported?** The embedded `/console` UI and visual docs support **10 languages**: Português (`pt-BR`), English (`en`), Español (`es`), Français (`fr`), Bahasa Indonesia (`id`), Basa Betawi (`bew` — Jakarta & Bekasi), Basa Sunda (`su` — Bandung & Bekasi), Basa Jawa (`jv` — Surabaya), Hata Batak Toba (`bbc` — Medan), and Bahasa Melayu (`ms` — Medan).
- **Can I connect AI agents (Claude Desktop, Cursor, Antigravity) or n8n?** Yes — use the built-in **MCP Server** (`mcp --stdio` or Streamable HTTP `/mcp`) for local AI agents, **WebMCP** (`document.modelContext` + declarative forms in `/console`) for browser-based AI agents, and the `/v1` HTTP API + `/openapi.json` + HMAC-SHA256 signed webhooks for n8n and custom apps.

---

## Transparency, Roadmap & Disclaimer

- **AI-assisted development:** Softagon Relay is developed with AI-assisted coding under human architecture, review, security design, and live testing against real WhatsApp accounts.
- **Roadmap:** Native **Tauri Desktop app** (Windows, Linux, macOS) + **`linux/arm64`** Docker images. Want these sooner? Open an issue on GitHub: https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/issues/new
- **Unofficial project:** Softagon Relay is an independent open-source project built on `whatsapp-rust` and is not affiliated with, authorized by, or endorsed by Meta or WhatsApp. Always deploy behind TLS and protect backups of `/data`.
