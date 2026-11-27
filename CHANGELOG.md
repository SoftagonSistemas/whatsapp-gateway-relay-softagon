# Changelog — Softagon Relay

All notable changes to **Softagon Relay** are documented in this file, following [Keep a Changelog](https://keepachangelog.com/) and [Semantic Versioning](https://semver.org/).

> **Quick links:** [English Docs](docs/en/README.md) · [Português (BR)](docs/pt-BR/README.md) · [Español](docs/es/README.md) · [Français](docs/fr/README.md) · [Bahasa Indonesia](docs/id/README.md) · [Basa Betawi](docs/bew/README.md) · [Basa Sunda](docs/su/README.md) · [Basa Jawa](docs/jv/README.md) · [Hata Batak Toba](docs/bbc/README.md) · [Bahasa Melayu](docs/ms/README.md) · [Docker Hub](https://hub.docker.com/r/softagon/relay) · [Softagon Sistemas](https://softagon.com.br)

---

## [0.3.7] — 2026-11-27

### Added — Grok Bot Turnkey Experience, Zero-Login Pairing & Webhook Auth
- **Zero-Login Temporary Pairing Link (`/pair/{token}`):** `POST /v1/sessions/{id}/pair-link` and MCP `create_pairing_link` mint a short-lived, cryptographically random, session-scoped URL (`GET /pair/{token}`) that displays the WhatsApp **QR Code** directly without requiring login or exposing `RELAY_API_KEY`. Auto-renews expired 60-second QR cycles continuously for up to **10 minutes** and invalidates immediately once paired.
- **Native PNG QR Endpoint (`qr.png`):** added pure-Rust PNG QR rendering on `GET /v1/sessions/{id}/qr.png` and `GET /pair/{token}/qr.png` for chat/file workflows.
- **8-Digit Phone Pairing Code (Secondary Alternative):** added `POST /v1/sessions/{id}/pair-code`, `POST /pair/{token}/pair-code`, MCP `request_pair_code`, WebMCP `relay_request_pair_code`, and a secondary tab in `ConnectView.vue` powered by `whatsapp-rust`'s native `pair_with_code`.
- **Contact Name (`sender_name` / `contact_name`) & `@lid` Phone Resolution (`sender_phone`):** extracts WhatsApp `push_name` and resolves `@s.whatsapp.net` phone digits from `sender_alt` / `recipient_alt` even when primary JIDs are `@lid`. Exposed in `message.received` / `message.sent` webhooks, `list_chats` (`contact_name`), and `get_chat_messages` (`sender_name`).
- **Phone-Sent Team Messages (`from_me` / `device_sync`):** unwraps `device_sent_message`, `ephemeral_message`, and `view_once_message` so messages sent by store staff directly from the WhatsApp phone are persisted as `direction: "outbound"` (`source: "device_sync"`), deduplicated by `external_id`, and emitted as `message.sent` webhooks.
- **Webhook Custom Auth Headers & Bearer Shorthand (`migrations/0004_webhook_auth_headers.sql`):** `POST /v1/webhooks` and `PATCH /v1/webhooks/{id}` now accept `bearer_token` (or `auth_header_name` + `auth_header_value`) so Relay talks directly to Grok Bot routines (`Authorization: Bearer ...`) without needing a proxy forwarder. Secret values are never echoed back in API responses.
- **Additional WhatsApp Message Types:** records `sticker`, `reaction`, `location`, `contact`, and `contacts` messages instead of dropping them.
- **Grok Bot Turnkey Kit:** added `integrations/grok-bot/setup.sh`, `ensure-running.sh`, `TEMPLATE.md`, and `docs/grok-bot.md`, plus MCP tools `create_session`, `create_pairing_link`, and `request_pair_code`.

### Fixed
- **Live Session Status in `list_sessions`:** `GET /v1/sessions` and MCP `list_sessions` now merge live WhatsApp engine connection status instead of returning stale SQLite values.
- **Real Message Timestamp & History Sync Suppression:** uses the real WhatsApp `info.timestamp` and marks messages older than 5 minutes as `is_history_sync: true`, storing them in SQLite for search without firing webhooks or queueing voice transcription.
- **Stop Retrying Webhooks on HTTP 401/403:** permanent authentication errors (`401`/`403`) no longer enter the retry backoff loop.
- **MCP `send_text_message` Idempotency:** properly enforces `idempotency_key` deduplication via `idempotency_keys` (matching the REST API) so retried agent calls never send duplicate messages.

---

## [0.3.6] — 2026-11-25

### Added — Chrome Modern Web Guidance & WebMCP Support
- **Imperative WebMCP API (`console/src/composables/useWebMcp.ts`):** registers client-side Web Console tools via `document.modelContext.registerTool()` (with `AbortController` lifecycle cleanup and `navigator.modelContext` fallback) so browser AI agents (Claude, ChatGPT, Chromium assistants) can inspect health (`relay_get_health`), switch language/theme (`relay_set_language`, `relay_set_theme`), navigate (`relay_navigate`), list sessions/webhooks (`relay_list_sessions`, `relay_get_session_status`, `relay_list_webhooks`), start QR pairing (`relay_create_session`), and send text messages (`relay_send_text_message`) while keeping destructive actions and API keys unexposed.
- **Declarative WebMCP Forms:** added `toolname`, `tooldescription`, and `toolparamdescription` attributes to key Console forms (`SessionView`, `ConnectView`, `EventsView`) and `.mwgrc` (`{"allowOriginTrials": true}`) following Chrome Modern Web Guidance.
- **CLI `--version` / `-V` flag:** `softagon-relay --version` prints the exact crate version.

---

## [0.3.5] — 2026-11-25

### Added — 10-Language Web Console, Localized Visual Docs & Automated Checks
- **6 new UI & documentation languages (`id`, `bew`, `su`, `jv`, `bbc`, `ms`):** added complete Web Console translations, official guides, and **60 localized screenshots** (`docs/images/{pt-BR,en,es,fr,id,bew,su,jv,bbc,ms}/*.png`) plus an updated `demo.gif` for **Bahasa Indonesia**, **Basa Betawi** (Jakarta & Bekasi), **Basa Sunda** (Bandung & Bekasi), **Basa Jawa** (Surabaya), **Hata Batak Toba** (Medan), and **Bahasa Melayu** (Medan), expanding total coverage from 4 to **10 languages**.
- **Accessible dropdown `LanguageSwitcher`:** replaced the horizontal language pill bar in `TopBar` and `LoginView` with a compact Raycast-style listbox dropdown (`🌐`) displaying all 10 languages with native names and keyboard navigation.
- **Expanded browser locale auto-detection & command palette search:** recognizes BCP 47 codes and legacy aliases (`in`, `jw`, `jav`, `btk`, `batak`, `zsm`) and indexes Indonesian/regional language and city keywords in `⌘K` / `Ctrl+K`.
- **Automated i18n, version parity, PII & screenshot validation (`scripts/check-i18n-docs.mjs`):** integrated into `.githooks/pre-push` to enforce key parity across all 10 catalogs, version sync (`Cargo.toml` ↔ `console/package.json`), zero PII sample strings, and valid localized screenshots.
- **SEO, AEO & GEO improvements:** added structured FAQ sections, regional language coverage, and updated Docker Hub metadata across `README.md`, `docs/dockerhub.md`, and `scripts/release-docker.sh`.

### Fixed
- **Localized session status badges:** `HomeView` and `SessionView` now translate all session states (`connected`, `disconnected`, `connecting`, `pairing_required`, `qr_ready`, `logged_out`) instead of showing raw English `disconnected`.
- **Dynamic console version:** injected `__APP_VERSION__` from `console/package.json` via Vite `define` and displayed it dynamically in `TopBar`, `LoginView`, and `HomeView` instead of hardcoding `v0.3.4`.
- **Completed missing translations & removed personal sample data:** localized `login.online` in `id`, `bew`, `su`, `jv`, and `bbc`; replaced personal sample names and phone numbers with synthetic placeholders across all catalogs, views, and comments; aligned section 2.4 of all 6 new guides with BR/MX/AR mobile resolution and connected-session deletion protection.

---

## [0.3.4] — 2026-11-25

### Fixed & Hardened — Docker MCP HTTP Opt-In & Safe STDIO Snippets
- **Docker MCP Streamable HTTP opt-in (`MCP_ALLOW_CONTAINER_BIND=true`):** allows binding `MCP_HTTP_HOST=0.0.0.0` inside a container published exclusively on host loopback (`-p 127.0.0.1:8091:8091`), while keeping non-loopback rejection enabled by default and preserving strict loopback `Host`, `Origin`, and Bearer token checks.
- **Unexpanded placeholder rejection:** `RELAY_API_KEY` now rejects literal unexpanded templates like `${RELAY_API_KEY}` and `<paste-your-generated-key>` at startup.
- **Safer copy-paste STDIO MCP snippets:** removed `MCP_RESTORE_SESSIONS=true` from the default JSON snippet (documented separately for standalone use without a running server) and switched to `"-e", "RELAY_API_KEY"` with an `env` block across all 4 language guides.
- **Security boundary clarification:** documented in `SECURITY.md` that with `0.0.0.0` inside Docker, other containers on the same Docker network can reach port `8091`, making `RELAY_API_KEY` (plus `Host`/`Origin` validation) the real access boundary.

---

## [0.3.3] — 2026-11-25

### Fixed — Safe `mcp --stdio` Isolation & Quickstart Consistency
- **Worker & session isolation in `mcp --stdio`:** `mcp --stdio` no longer spawns background webhook/transcription workers or automatically restores WhatsApp WebSocket sessions by default (pass `MCP_RESTORE_SESSIONS=true` for standalone STDIO without a running server), preventing session disconnects, duplicate webhook deliveries, and SQLite write contention.
- **Blocked MCP example keys:** added `your-generated-64-char-hex-key` and `sua-chave-gerada-com-openssl` to the startup denylist.
- **Updated ES/FR & Docker Hub quickstarts:** aligned all quickstart commands to `openssl rand -hex 32` / `${RELAY_API_KEY}` and `127.0.0.1:8080`.
- Fixed `/data/whatsapp-sessions.db` path in `SECURITY.md` and updated the `whatsapp-rust` link to `oxidezap/whatsapp-rust`.

---

## [0.3.2] — 2026-11-25

### Added — Friendly WhatsApp Disconnection & Restriction Diagnostics
- **Persisted `status_reason` in SQLite (`migrations/0003_session_status_reason.sql`):** stores normalized disconnect/restriction reasons (`device_removed`, `account_restricted`, `logout`, `qr_expired`) and returns them in `GET /v1/sessions`, `GET /v1/sessions/{id}/status`, and the MCP `get_session_status` tool.
- **Friendly Web Console warning banner (4 languages):** when WhatsApp removes a linked device or temporarily restricts an account, the session screen explains what happened and advises checking the phone app and waiting until any restriction expires before re-pairing.
- **Clear API & MCP send errors:** sending on a `logged_out` session returns `409 Conflict` with the specific disconnection/restriction reason.

---

## [0.3.1] — 2026-11-25

### Security & Community
- **Strict `RELAY_API_KEY` validation:** refuses startup on known documentation placeholder strings and keys shorter than 16 characters.
- **Safer quickstart:** one-line `docker run` generates a 256-bit random key via `openssl rand -hex 32` and binds to `127.0.0.1:8080`.
- **Zero-install MCP STDIO via Docker** + prebuilt `linux-x86_64` release binaries with SHA-256 checksums.
- **Open-source governance:** official verbatim **Apache-2.0** `LICENSE`, `CONTRIBUTING.md`, `SECURITY.md`, GitHub Discussions, `good first issue` tickets, and custom social preview.

---

## [0.3.0] — 2026-11-25

### Added — Native Local MCP Server (Model Context Protocol)
- **Official Rust MCP SDK (`rmcp 3.11`):** turns Softagon Relay into a local MCP Server so AI agents (Claude Desktop, Cursor, Antigravity, VS Code) can inspect sessions, search conversations, read voice-note transcriptions, and send WhatsApp messages.
- **Shared domain layer (`RelayService`):** both the REST API (`/v1/*`) and MCP server call `src/core/service.rs` directly — MCP does not depend on HTTP REST.
- **Two transports:** STDIO (`softagon-relay mcp --stdio`, logs routed to `stderr`) and Streamable HTTP (`softagon-relay mcp --http` or `MCP_HTTP_ENABLED=true`, loopback-only by default with `Origin` anti-DNS rebinding validation and Bearer auth).
- **9 MCP tools + SQLite audit log (`migrations/0002_mcp_audit.sql`):** `list_sessions`, `get_session_status`, `list_chats`, `search_messages`, `get_chat_messages`, `get_message_transcription`, `send_text_message`, plus destructive tools (`logout_session`, `delete_session`) blocked by default unless `MCP_ALLOW_DESTRUCTIVE=true` and `confirm: true`.

---

## [0.2.1] — 2026-11-25

### Added — OpenAPI JSON Endpoint & Internal Console Links
- **`GET /openapi.json`:** serves OpenAPI 3.2.1 in JSON (converted once in memory via `OnceLock`), respecting `DOCS_ENABLED` and `DOCS_PUBLIC`.
- Added direct `openapi.json` / `openapi.yaml` links in the authenticated footer and a one-click copy URL box in `/console/api`.

---

## [0.2.0] — 2026-11-25

### Added — Raycast-Style Embedded Web Console & 4-Language Docs
- **Embedded Web Console (`/console` and `/manager` redirect):** Vue 3 + TypeScript + Vite SPA embedded via `rust-embed` (~91 kB gzip initial bundle), keyboard-first (`⌘K`/`Ctrl+K`), Light/Dark theme, and 4 languages (`pt-BR`, `en`, `es`, `fr`).
- **Linked device identity:** appears as **Softagon Relay** (Chrome) in WhatsApp instead of `rust`.
- **Smart mobile JID resolution:** uses `is_on_whatsapp` to resolve mobile digit variations for Brazil (`55`), Mexico (`521`/`52`), and Argentina (`549`/`54`).
- **Safe session management:** `DELETE /v1/sessions/{id}` (`409` if connected), `POST /v1/sessions/{id}/reconnect`, and live SVG QR codes (`GET /v1/sessions/{id}/qr.svg`).
- **4-language visual documentation:** 24 screenshots (`docs/images/{pt-BR,en,es,fr}/*.png`) with zero real user PII.

---

## [0.1.0] — 2026-11-24

### Added — Initial Single-Container Rust Gateway
- **Tokio + Axum HTTP API (`/v1/*`):** multi-device WhatsApp Web via `whatsapp-rust`, idempotent text/media sends (`Idempotency-Key`), and per-session rate limiting.
- **Embedded SQLite (WAL):** zero external databases required (`migrations/0001_initial.sql`).
- **Pluggable media storage:** Local filesystem or S3-compatible object storage (AWS S3, Cloudflare R2, MinIO) via `object_store`.
- **HMAC-SHA256 signed webhooks:** persistent outbox with exponential backoff + jitter.
- **Offline multilingual audio transcription:** pure-Rust OGG/Opus decoding (`symphonia` + `symphonia-opus` + `rubato`, no FFmpeg) + lazy-loaded `whisper.cpp`.
- **OpenAPI 3.2.1 + Scalar UI (`/docs`, `/openapi.yaml`)** and hardened non-root Docker image (UID `10001`).

---

## Roadmap

- **Native Desktop App ([Tauri](https://tauri.app/) — Windows, Linux & macOS):** package the Rust core, embedded Web Console, and local MCP Server with a system tray icon so desktop users do not need Docker.
- **Multi-arch `linux/arm64` Docker images** for Raspberry Pi, Apple Silicon, and ARM VPS servers.

Want the Desktop edition, ARM64 builds, or new MCP tools?  
👉 **[Open a GitHub Issue](https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/issues/new)** or join **[GitHub Discussions](https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/discussions)**!
