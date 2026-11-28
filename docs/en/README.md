# Official Documentation — Softagon Relay (English)

> Languages: [Português (BR)](../pt-BR/README.md) · **English** · [Español](../es/README.md) · [Français](../fr/README.md) · [Bahasa Indonesia](../id/README.md) · [Basa Betawi](../bew/README.md) · [Basa Sunda](../su/README.md) · [Basa Jawa](../jv/README.md) · [Hata Batak Toba](../bbc/README.md) · [Bahasa Melayu](../ms/README.md)

**What is Softagon Relay?**  
**Softagon Relay** is an open-source, self-hosted **WhatsApp Web HTTP API gateway** built in **Rust** by **[Softagon Sistemas](https://softagon.com.br)**. It runs as a **single Docker container (~55 MB)** with embedded **SQLite (WAL)**, a **Raycast-style Web Console** at `/console`, **Local or S3/R2/MinIO** media storage, **HMAC-SHA256 signed webhooks**, **OpenAPI 3.2.1**, and **offline multilingual voice-note transcription** powered by `whisper.cpp`.

- **Official Website & Contact:** <https://softagon.com.br>
- **Docker Hub:** <https://hub.docker.com/r/softagon/relay> (`softagon/relay:latest`)
- **Source Code (GitHub):** <https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon>
- **Changelog & Roadmap:** [CHANGELOG.md](../../CHANGELOG.md) *(including upcoming **Tauri Desktop app** for Windows, Linux & macOS — [open an Issue](https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/issues/new) if you need it!)*
- **License:** Apache-2.0

---

## 1. Quick Start (Docker)

```bash
KEY="$(openssl rand -hex 32)" && echo "RELAY_API_KEY=$KEY" && docker run -d \
  --name softagon-relay \
  --restart unless-stopped \
  -p 127.0.0.1:8080:8080 \
  -v relay-data:/data \
  -e RELAY_API_KEY="$KEY" \
  softagon/relay:latest
```

Open the **Web Console** at `http://localhost:8080/console` (or `/manager`, which redirects automatically) and enter your `RELAY_API_KEY`.

### Hardened Docker Compose
Use the repository's [compose.yaml](../../compose.yaml) (`read_only: true`, `cap_drop: ALL`, `no-new-privileges:true`, non-root UID `10001`).

---

## 2. Web Console Visual Guide (`/console`)

The embedded Web Console is designed in a **Raycast-inspired** keyboard-first layout, supporting instant language switching (**10 languages:** `PT / EN / ES / FR / ID / BEW / SU / JV / BTK / MS`) and **Light / Dark** themes.

### 2.1 Sign In (`/console/login`)
Enter only the key configured in `RELAY_API_KEY` (no username or email required). It is sent as a `Bearer` token and can optionally be remembered in your browser.

![Softagon Relay Login Screen in English](../images/en/login.png)

### 2.2 Home & Command Bar (`/console`)
Check live server health (`Healthy · N sessions`), jump to your sessions list with one click, or type any WhatsApp phone number (with automatic international formatting & validation) and press `↵` to start pairing.

![Softagon Relay Home and Command Bar in English](../images/en/home.png)

### 2.3 Connect a New Number via QR Code (`/console/connect`)
Once the session is created, a live SVG QR code appears with an automatic refresh countdown (press `r` to regenerate immediately). On your phone, open **WhatsApp → Linked devices → Link a device** and scan. The linked device appears as **Softagon Relay**.

![Softagon Relay QR Code Pairing Screen in English](../images/en/connect.png)

### 2.4 Session Management & Test Messaging (`/console/sessions`)
Manage all connected WhatsApp accounts from one place:
- Copy session **ID** and **JID** in one click.
- Click **Send message** to send a live test message with international phone validation and automatic mobile digit resolution (BR/MX/AR).
- Open **Actions (`⌘K` / `Ctrl+K`)** to restart, reconnect, log out, or **delete disconnected sessions** (connected sessions are protected against accidental deletion).

![Softagon Relay Session Management Screen in English](../images/en/session.png)

### 2.5 Events & Webhooks (`/console/events`)
Register webhook URLs to receive real-time events (`message.received`, `message.sent`, `message.ack`, `session.status`, `message.transcription.completed`, `group.updated`). Relay automatically generates an HMAC-SHA256 signing secret and lets you toggle or delete subscriptions.

![Softagon Relay Events and Webhooks Screen in English](../images/en/events.png)

### 2.6 Interactive API Reference (`/console/api`)
Generated live from `/openapi.yaml` (also available as **`/openapi.json`** for direct AI agent, n8n, Postman, and SDK integration). Filter endpoints, copy ready-to-use **cURL, JavaScript, Python, and PHP** snippets, and execute real requests against your instance (`⌘↵` / `Ctrl+↵`).

![Softagon Relay Interactive API Reference in English](../images/en/api.png)

### Keyboard Shortcuts
| Shortcut | Action |
|---|---|
| `⌘K` / `Ctrl+K` | Open command bar or session action panel |
| `/` | Focus search |
| `n` | Connect a new number |
| `g` then `s` / `e` / `a` | Go to Sessions / Events / API |
| `r` | Regenerate QR Code on Connect screen |
| `?` | Show shortcuts sheet |

---

## 3. HTTP API (`/v1`) & Authentication

All `/v1/*` routes (except public `GET /v1/health`) require:

```http
Authorization: Bearer <RELAY_API_KEY>
```

| Method | Endpoint | Purpose |
|---|---|---|
| `GET` | `/v1/health` | Server liveness/readiness (public) |
| `GET` / `POST` | `/v1/sessions` | List or create a session |
| `GET` / `DELETE` | `/v1/sessions/{id}` | Get details or delete a disconnected session (`409` if connected) |
| `GET` | `/v1/sessions/{id}/qr` · `/qr.svg` | Pairing QR payload (JSON or SVG) |
| `GET` | `/v1/sessions/{id}/status` | Live connection state |
| `POST` | `/v1/sessions/{id}/reconnect` | Restart and start a fresh QR cycle |
| `POST` | `/v1/sessions/{id}/logout` | Unlink WhatsApp device |
| `POST` | `/v1/sessions/{id}/messages/text` | Send text (supports `Idempotency-Key` & BR/MX/AR mobile JID resolution) |
| `POST` | `/v1/sessions/{id}/messages/media` | Send image, video, audio, or document |
| `GET` | `/v1/messages/{id}` | Retrieve message status & metadata |
| `GET` / `POST` | `/v1/messages/{id}/transcription` | Retrieve or retry Whisper transcription |
| `GET` / `POST` | `/v1/webhooks` | List or create webhook subscriptions |
| `GET` / `PATCH` / `DELETE` | `/v1/webhooks/{id}` | Inspect, update, or delete a webhook |

---

## 4. Signed Webhooks (HMAC-SHA256)

Events: `session.status`, `message.received`, `message.sent`, `message.ack`, `message.transcription.completed`, `message.transcription.failed`, `group.updated`.

Headers on every delivery (**at-least-once** with exponential backoff + jitter):
- `X-Relay-Event-Id`: stable UUIDv7 event ID for deduplication
- `X-Relay-Event-Type`: event name
- `X-Relay-Timestamp`: Unix timestamp in seconds
- `X-Relay-Signature`: `sha256=<hex(HMAC-SHA256(secret, "{timestamp}.{raw_body}"))>`

---

## 5. Local Transcription (Whisper) & S3 Storage

- **Voice Notes:** decoded in pure Rust (Symphonia + `symphonia-opus` + Rubato 16 kHz mono, no FFmpeg required) and transcribed locally with `whisper.cpp` (multilingual `base` model, lazy-loaded and unloaded after idle timeout).
- **S3 / R2 / MinIO:** set `MEDIA_STORAGE=s3`, `S3_BUCKET`, `S3_ACCESS_KEY_ID`, `S3_SECRET_ACCESS_KEY`, and `S3_ENDPOINT_URL`.

---

## 6. Local MCP Server for AI Agents (`v0.3.0`)

Softagon Relay ships with a native Rust **MCP Server (Model Context Protocol)** powered by `rmcp`, calling the shared `RelayService` directly (no HTTP REST dependency):

- **STDIO (default local mode):** run `softagon-relay mcp` (or `softagon-relay mcp --stdio`). All logs are routed to `stderr` so `stdout` stays clean JSON-RPC.
- **Streamable HTTP:** run `softagon-relay mcp --http` or enable `MCP_HTTP_ENABLED=true` alongside `softagon-relay serve` — binds to `127.0.0.1:8091/mcp` by default (in Docker behind `-p 127.0.0.1:8091:8091`, set `MCP_HTTP_HOST=0.0.0.0` + `MCP_ALLOW_CONTAINER_BIND=true`), always enforcing loopback `Host`/`Origin` validation (anti-DNS rebinding) and `Authorization: Bearer <RELAY_API_KEY>`.
- **Tools:** `list_sessions`, `get_session_status`, `list_chats`, `search_messages`, `get_chat_messages`, `get_message_transcription`, `send_text_message`, plus destructive tools `logout_session` / `delete_session` (**disabled by default** unless `MCP_ALLOW_DESTRUCTIVE=true` and `confirm: true`).
- **Audit trail:** every MCP tool invocation is persisted to `mcp_audit_log` in SQLite.

```json
{
  "mcpServers": {
    "softagon-relay": {
      "command": "docker",
      "args": [
        "run", "-i", "--rm",
        "-v", "relay-data:/data",
        "-e", "RELAY_API_KEY",
        "softagon/relay:latest",
        "mcp", "--stdio"
      ],
      "env": {
        "RELAY_API_KEY": "<paste-your-generated-key>"
      }
    }
  }
}
```

> For **standalone** STDIO (when no `serve` container is running), add `"-e", "MCP_RESTORE_SESSIONS=true"` to `args`. Alongside a running server container, prefer **Streamable HTTP** inside the same container (`-p 127.0.0.1:8091:8091 -e MCP_HTTP_ENABLED=true -e MCP_HTTP_HOST=0.0.0.0 -e MCP_ALLOW_CONTAINER_BIND=true`).

### 6.2 Grok Bot, Zero-Login Pairing & Agent Features (`v0.3.8`)

- **Grok Bot Turnkey Kit:** import in 1 click with **[Add to Grok Bot](https://x.ai/bot/Xe4LpCGaepuE-J2BYH363)** or see [`docs/grok-bot.md`](../grok-bot.md) and [`integrations/grok-bot/TEMPLATE.md`](../../integrations/grok-bot/TEMPLATE.md) to install the native binary under `/workspace/softagon-relay`.
- **Zero-Login Pairing Link (`/pair/{token}`):** `POST /v1/sessions/{id}/pair-link` (or MCP `create_pairing_link`) opens the **QR Code** directly without login or exposing `RELAY_API_KEY`, auto-renews expired 60s cycles for up to 10 minutes, and provides native `qr.png` plus an **8-digit code** alternative (`POST /v1/sessions/{id}/pair-code` / MCP `request_pair_code`).
- **Webhook Bearer & Custom Auth Headers:** pass `bearer_token` or `auth_header_name`/`auth_header_value` on `POST`/`PATCH /v1/webhooks` to talk directly to Grok Bot routines without a forwarder proxy. HTTP `401`/`403` responses stop retries immediately.
- **Rich Conversation Context:** webhooks and SQLite history include `sender_name` (`push_name`), `sender_phone` (resolved even for `@lid` contacts), phone-sent team replies (`from_me: true`, `source: "device_sync"`), real timestamps with `is_history_sync` webhook suppression, plus `sticker`, `reaction`, `location`, and `contact` messages.
- **Full Interactive MCP Tools & REST Endpoints (`v0.4.0`–`v0.4.1`):** `reply_to_message_id`, `send_reaction`, `send_chat_presence`, `subscribe_presence`, `as_voice_note` (PTT), `sticker`, `send_location`, `send_contact`, `edit_message`, `revoke_message`, `check_whatsapp`, `get_profile_picture`, `list_groups`, `get_group_info`, `GET /v1/media/{media_id}`, `MEDIA_RETENTION_DAYS`, online `softagon-relay backup`, Prometheus `GET /v1/metrics`, `session.disconnected` alerts, automatic legacy `@lid` history migration, SQLite `quoted.text` fallback, and burst coalescing `WEBHOOK_COALESCE_SECONDS` (global default `0`; `4s` in Grok Bot kit).

---

## 7. Real Production Benchmark (8-Hour Continuous Soak Test)

Measured in real-world production (**2026-11-27/28**, 16 checkpoints every 30 min, `linux/amd64`, `--features whisper`, 1 real WhatsApp account connected):

| Metric | Measured Result (8h Continuous) |
|---|---|
| **Steady-state RAM** | **31.7–32.1 MiB** (zero memory leak after 8h) |
| **Peak RAM (Whisper `base` active)** | **286.4 MiB** with automatic idle unload back to **~32 MiB** |
| **Average CPU** | **0.04%–0.19%** |
| **Signed webhooks delivered (Bearer + HMAC)** | **1,083** (`0` auth failures, `0` `status@broadcast` leaks) |
| **`@lid` → phone resolution** | **100% (`127/127`)** across private chats and groups |
| **Offline audio transcription (CPU)** | **100% (`5/5`)**, up to **4.5× faster than real-time** |
| **Critical errors (`ERROR`) in 8h** | **`0`** (longest unbroken connected streak: **6h20**) |

---

## 8. Support & Contact

Need dedicated support, custom integrations, or AI & messaging engineering?  
Visit **[softagon.com.br](https://softagon.com.br)** and talk to the **Softagon Sistemas** team.
