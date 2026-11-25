# Softagon Relay

**Self-hosted WhatsApp Web gateway, embedded Web Console, and local MCP Server in a single Rust container (~55 MB).**

[![License: Apache-2.0](https://img.shields.io/badge/License-Apache--2.0-blue.svg)](LICENSE)
[![Release](https://img.shields.io/github/v/release/SoftagonSistemas/whatsapp-gateway-relay-softagon)](https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/releases)
[![Docker Hub](https://img.shields.io/badge/Docker%20Hub-softagon%2Frelay-2496ED?logo=docker&logoColor=white)](https://hub.docker.com/r/softagon/relay)

> **Docs:** [Português (BR)](docs/pt-BR/README.md) · [English](docs/en/README.md) · [Español](docs/es/README.md) · [Français](docs/fr/README.md) · [Changelog & Roadmap](CHANGELOG.md)

## Quick Start (One-Line Docker Run)

```bash
docker run -d --name softagon-relay --restart unless-stopped -p 8080:8080 -v relay-data:/data -e RELAY_API_KEY="change-me-to-a-strong-secret" softagon/relay:latest
```

Open **`http://localhost:8080/console`** (or `/manager`), paste your `RELAY_API_KEY`, type a phone number, and scan the live SVG QR code.

![Softagon Relay Web Console walkthrough](docs/images/demo.gif)

---

## What It Includes

- **Single container, zero external databases:** native Rust binary (Tokio + Axum) with embedded **SQLite (WAL)** — no PostgreSQL, Redis, RabbitMQ, or Chromium required.
- **Embedded Raycast-style Web Console (`/console`):** keyboard-first Vue 3 UI (`⌘K`/`Ctrl+K`), Light/Dark theme, 4 languages (`pt-BR`, `en`, `es`, `fr`), live SVG QR pairing, test messaging, webhook management, and live OpenAPI runner.
- **Native Local MCP Server (`v0.3.0`, `rmcp`):** connects local AI agents (Claude Desktop, Cursor, Antigravity, VS Code) directly to the internal `RelayService` without going through HTTP REST:
  - **STDIO** (default local mode): `softagon-relay mcp --stdio` (logs routed to `stderr`).
  - **Streamable HTTP**: `softagon-relay mcp --http` or `MCP_HTTP_ENABLED=true` — loopback-only (`127.0.0.1`), `Origin` validation (anti-DNS rebinding), Bearer auth, destructive tools blocked by default, and SQLite audit log (`mcp_audit_log`).
- **Versioned HTTP API (`/v1`) + OpenAPI 3.2.1:** available at `/openapi.json`, `/openapi.yaml`, and `/docs` (Scalar).
- **Signed Webhooks (HMAC-SHA256):** persistent SQLite outbox with exponential backoff + jitter and stable UUIDv7 event IDs.
- **Offline Multilingual Voice-Note Transcription:** pure-Rust OGG/Opus decoding (`symphonia` + `symphonia-opus` + `rubato`, no FFmpeg) + `whisper.cpp` (lazy-loaded on demand and unloaded after idle timeout).
- **Pluggable Media Storage:** Local filesystem (`/data/media`) or S3-compatible object storage (AWS S3, Cloudflare R2, MinIO).

---

## Measured Resource Footprint (Real Scenarios)

Measured on `softagon/relay:0.3.0` (`linux/amd64`, Docker 29.1.3, Linux host):

| Scenario | RAM | CPU | Notes |
|---|---:|---:|---|
| **Boot / Idle (0 paired sessions)** | ~4–5 MiB | ~0.05% | Before pairing any WhatsApp session |
| **1 paired WhatsApp session (steady state)** | **~38 MiB** | **~0.1–0.2%** | Active WebSocket connection, receiving & sending messages |
| **Active voice-note transcription (`whisper` base)** | **~212 MiB** | Burst | Model loads lazily on first audio and unloads after `180s` idle |
| **Docker image size (`linux/amd64`)** | **55.0 MB** | — | Includes Rust binary, Console SPA, SQLite, Opus decoder, `whisper.cpp` |

> *Architecture note:* Prebuilt Docker images currently publish **`linux/amd64`**. Multi-arch **`linux/arm64`** support is tracked on our roadmap (you can also build locally on ARM64 with `docker build`).

---

## Local MCP Server Example (STDIO)

```json
{
  "mcpServers": {
    "softagon-relay": {
      "command": "softagon-relay",
      "args": ["mcp", "--stdio"],
      "env": {
        "RELAY_API_KEY": "your-strong-secret",
        "DATA_DIR": "/path/to/relay-data"
      }
    }
  }
}
```

Tools exposed: `list_sessions`, `get_session_status`, `list_chats`, `search_messages`, `get_chat_messages`, `get_message_transcription`, `send_text_message`, and (disabled unless `MCP_ALLOW_DESTRUCTIVE=true` + `confirm: true`) `logout_session` / `delete_session`.

---

## Hardened Docker Compose

Use the repository's [compose.yaml](compose.yaml) (`read_only: true`, `cap_drop: ALL`, `no-new-privileges:true`, non-root UID `10001`).

```bash
cp .env.example .env
# Edit RELAY_API_KEY in .env
docker compose up -d
```

---

## Transparency on AI-Assisted Development

Softagon Relay was built using **AI-assisted software engineering** alongside human architecture decisions, code review, security design, and live testing against real WhatsApp accounts. All automated checks (`vue-tsc`, `vite build`, `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, and integration tests covering REST, SQLite, webhooks, audio decoding, and MCP security) run locally before every push.

---

## Roadmap

- **Native Desktop App (Tauri — Windows, Linux & macOS):** bundle the Rust core, embedded Console, and local MCP Server with a system tray icon so desktop users do not need Docker.
- **Multi-arch `linux/arm64` Docker images** for Raspberry Pi, Apple Silicon, and ARM VPS servers.

If you need the Tauri Desktop edition, ARM64 images, or specific MCP tools, **[open a GitHub Issue](https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/issues/new)** and share your OS and use case!

---

## Disclaimer & License

Softagon Relay is an independent, unofficial open-source project built on [`whatsapp-rust`](https://github.com/jlucaso1/whatsapp-rust) and is **not affiliated with, authorized by, or endorsed by Meta or WhatsApp**. Review Meta's Terms of Service, deploy behind TLS, and protect backups of `/data`.

Licensed under the **[Apache License, Version 2.0](LICENSE)** · Maintained by **[Softagon Sistemas](https://softagon.com.br)**.
