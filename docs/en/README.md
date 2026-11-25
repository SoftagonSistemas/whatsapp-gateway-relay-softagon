# Official Documentation — Softagon Relay (English)

> Languages: [Português (BR)](../pt-BR/README.md) · **English** · [Español](../es/README.md) · [Français](../fr/README.md)

**What is Softagon Relay?**  
**Softagon Relay** is an open-source, self-hosted **WhatsApp Web HTTP API gateway** built in **Rust** by **[Softagon Sistemas](https://softagon.com.br)**. It runs as a **single Docker container (~52 MB)** with embedded **SQLite (WAL)**, a **Raycast-style Web Console** at `/console`, **Local or S3/R2/MinIO** media storage, **HMAC-SHA256 signed webhooks**, **OpenAPI 3.2.1**, and **offline multilingual voice-note transcription** powered by `whisper.cpp`.

- **Official Website & Contact:** <https://softagon.com.br>
- **Docker Hub:** <https://hub.docker.com/r/softagon/relay> (`softagon/relay:latest`)
- **Source Code (GitHub):** <https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon>
- **License:** Apache-2.0

---

## 1. Quick Start (Docker)

```bash
docker run -d \
  --name softagon-relay \
  --restart unless-stopped \
  -p 8080:8080 \
  -v relay-data:/data \
  -e RELAY_API_KEY="replace-with-a-strong-secret" \
  softagon/relay:latest
```

Open the **Web Console** at `http://localhost:8080/console` (or `/manager`, which redirects automatically) and enter your `RELAY_API_KEY`.

### Hardened Docker Compose
Use the repository's [compose.yaml](../../compose.yaml) (`read_only: true`, `cap_drop: ALL`, `no-new-privileges:true`, non-root UID `10001`).

---

## 2. Web Console (`/console`)

Designed for keyboard-first productivity:
- **Login (`/console/login`):** single `RELAY_API_KEY` field (no usernames or passwords).
- **Command Bar (`/console`):** type any WhatsApp phone number (e.g. `+1 415 555 2671` or `87 99200-5656`) and press `↵` to create the session and show the live SVG QR Code.
- **Sessions (`/console/sessions`):** copy ID/JID, send test messages with international validation, reconnect/regenerate QR, log out, or delete disconnected sessions.
- **Webhooks (`/console/events`):** create, toggle, and delete webhook subscriptions with automatic HMAC secret generation.
- **API Reference (`/console/api`):** generated live from `/openapi.yaml` with cURL, JavaScript, Python, and PHP snippets plus live execution (`⌘↵`/`Ctrl+↵`).

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

## 6. Support & Contact

Need dedicated support, custom integrations, or AI & messaging engineering?  
Visit **[softagon.com.br](https://softagon.com.br)** and talk to the **Softagon Sistemas** team.
