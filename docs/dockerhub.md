# Softagon Relay — Self-Hosted WhatsApp API Gateway in Rust (Docker)

**What is Softagon Relay?**  
**Softagon Relay** (`softagon/relay`) is an open-source, self-hosted **WhatsApp Web HTTP API gateway** built in **Rust** by **Softagon Sistemas**. It runs as a single **~51 MB Docker container** with embedded **SQLite (WAL)**, optional **S3-compatible media storage** (AWS S3, Cloudflare R2, MinIO), **HMAC-SHA256 signed webhooks** with persistent outbox retries, **OpenAPI 3.2.1 + Scalar** documentation, and **offline multilingual voice-note transcription** powered by `whisper.cpp`.

Unlike multi-service Node.js or Python WhatsApp gateways that require PostgreSQL, Redis, RabbitMQ, or Chromium, Softagon Relay requires **zero external infrastructure** to start.

- **Docker Image:** `softagon/relay:latest` (`linux/amd64`, non-root UID `10001`)
- **Source Code & Documentation:** https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon
- **API Contract:** OpenAPI 3.2.1 (`/openapi.yaml`) + Scalar Interactive Docs (`/docs`)
- **License:** Apache-2.0

---

## Key Facts & Measured Container Footprint

The following baseline measurements were recorded on **2026-11-24** against `softagon/relay:0.1.0` (`linux/amd64`, compiled with `--features whisper`) running under a constrained **`1 vCPU / 2 GB RAM`** container profile (`--read-only`, `--cap-drop ALL`, non-root user, Intel Core i7-13700H host, Linux 7.0, Docker 29.1.3):

| Metric | Measured Value | Notes |
|---|---|---|
| **Compressed/Unpacked Image Size** | **50.9 MB** (`50,934,185` bytes) | Includes Rust binary, SQLite, S3 client, Opus decoder, and `whisper.cpp` |
| **Idle Container RAM** | **3.84 MiB** (`0.19%` of 2 GiB) | Measured before lazy-loading Whisper model |
| **Idle CPU / PIDs** | **0.05% CPU · 5 PIDs** | Single static-linked Tokio/Axum process |
| **Post-Burst RAM (250 API calls)** | **4.88 MiB** (`0.24%` of 2 GiB) | 200× `GET /v1/sessions` + 50× `POST /v1/webhooks` |
| **SQLite Restart Durability** | **50 / 50 webhooks preserved** | Verified across `docker restart` with WAL enabled |
| **External Dependencies Required** | **0** | Embedded SQLite; S3/R2/MinIO is optional |

> Whisper models are loaded **lazily** on the first incoming voice note and **unloaded automatically** after `TRANSCRIPTION_IDLE_UNLOAD_SECONDS` (default: 180s) to return RAM to the host.

---

## Why Choose Softagon Relay? (Architecture Comparison)

| Capability | Softagon Relay (`softagon/relay`) | Typical Node.js / Browser-Based Gateways |
|---|---|---|
| **Runtime** | Native Rust binary (Tokio + Axum) | Node.js / Chromium / Puppeteer |
| **Mandatory Services** | None (single container + embedded SQLite) | Often requires PostgreSQL + Redis / RabbitMQ |
| **Image Size** | ~51 MB | 300 MB – 1.5 GB+ |
| **Idle Memory** | ~4 MiB (lazy Whisper loading) | 150 MB – 600 MB+ |
| **Voice-Note Transcription** | Built-in offline multilingual `whisper.cpp` + pure-Rust Opus decoder | Requires external OpenAI/Whisper API or sidecar |
| **Webhook Reliability** | SQLite outbox + HMAC-SHA256 + exponential backoff & jitter | Varies; often fire-and-forget |
| **API Specification** | Hand-authored **OpenAPI 3.2.1** + Scalar UI | Varies |

---

## How to Run Softagon Relay with Docker (Quick Start)

### Option 1: Single Command (`docker run`)

```bash
docker run -d \
  --name softagon-relay \
  --restart unless-stopped \
  -p 8080:8080 \
  -v relay-data:/data \
  -e RELAY_API_KEY="replace-with-a-strong-random-key" \
  -e DOCS_ENABLED=true \
  -e DOCS_PUBLIC=false \
  softagon/relay:latest
```

Verify the container is healthy:

```bash
curl http://localhost:8080/v1/health
```

Expected response:

```json
{
  "status": "ok",
  "database": "ok",
  "media_storage": "local",
  "transcription_enabled": true,
  "transcription_available": true,
  "active_sessions": 0
}
```

### Option 2: Hardened `compose.yaml` (Recommended for Production)

```yaml
services:
  relay:
    image: softagon/relay:0.1.0
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

## How to Pair WhatsApp, Send a Message, and Receive Signed Webhooks

### 1. Create a WhatsApp Session and Fetch the QR Code

```bash
# Create a session
curl -X POST http://localhost:8080/v1/sessions \
  -H "Authorization: Bearer $RELAY_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"name": "support-bot"}'

# Retrieve the pairing QR code
curl http://localhost:8080/v1/sessions/{session_id}/qr \
  -H "Authorization: Bearer $RELAY_API_KEY"
```

### 2. Send an Idempotent Text Message

```bash
curl -X POST http://localhost:8080/v1/sessions/{session_id}/messages/text \
  -H "Authorization: Bearer $RELAY_API_KEY" \
  -H "Idempotency-Key: order-12345-confirmation" \
  -H "Content-Type: application/json" \
  -d '{"to": "5511999999999", "text": "Hello from Softagon Relay!"}'
```

### 3. Register a Signed Webhook Subscription

```bash
curl -X POST http://localhost:8080/v1/webhooks \
  -H "Authorization: Bearer $RELAY_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://your-app.example.com/webhooks/whatsapp",
    "events": ["message.received", "message.transcription.completed", "session.status"]
  }'
```

Every webhook request includes:
- `X-Relay-Event-Id` — Stable UUIDv7 event ID for consumer deduplication (**at-least-once** delivery)
- `X-Relay-Event-Type` — Event name (`session.status`, `message.received`, `message.sent`, `message.ack`, `message.transcription.completed`, `message.transcription.failed`, `group.updated`)
- `X-Relay-Timestamp` — Unix timestamp in seconds (use to reject replay attacks)
- `X-Relay-Signature` — `sha256=<hex(HMAC-SHA256(secret, "{timestamp}.{raw_body}"))>`

---

## Supported Tags

| Tag | Architecture | Description |
|---|---|---|
| `latest` | `linux/amd64` | Latest stable release (`0.1.0`) |
| `0.1` | `linux/amd64` | Latest `0.1.x` release |
| `0.1.0` | `linux/amd64` | Pinned immutable release |

---

## Complete REST API Endpoints (`/v1`)

All routes except `GET /v1/health` require `Authorization: Bearer <RELAY_API_KEY>`:

| Method | Endpoint | Purpose |
|---|---|---|
| `GET` | `/v1/health` | Public liveness & readiness probe |
| `GET` / `POST` | `/v1/sessions` | List or create WhatsApp sessions |
| `GET` | `/v1/sessions/{session_id}` | Get session metadata |
| `GET` | `/v1/sessions/{session_id}/qr` | Get pairing QR code |
| `GET` | `/v1/sessions/{session_id}/status` | Get live connection state |
| `POST` | `/v1/sessions/{session_id}/logout` | Disconnect and unlink device |
| `POST` | `/v1/sessions/{session_id}/messages/text` | Send text message (supports `Idempotency-Key`) |
| `POST` | `/v1/sessions/{session_id}/messages/media` | Send image, video, audio, or document |
| `GET` | `/v1/messages/{message_id}` | Retrieve message status & metadata |
| `GET` / `POST` | `/v1/messages/{message_id}/transcription` | Fetch or retry voice-note transcription |
| `GET` / `POST` | `/v1/webhooks` | List or create webhook subscriptions |
| `GET` / `PATCH` / `DELETE` | `/v1/webhooks/{webhook_id}` | Inspect, update, or remove a webhook |
| `GET` | `/docs` · `/openapi.yaml` | Scalar API Reference & OpenAPI 3.2.1 spec |

---

## Environment Variables

| Variable | Default | Description |
|---|---|---|
| `RELAY_API_KEY` | *(required)* | Bearer API key. Startup fails if empty or set to the example placeholder. |
| `APP_HOST` / `APP_PORT` | `0.0.0.0` / `8080` | Bind host and port |
| `RUST_LOG` | `info` | Structured JSON log level (`error`, `warn`, `info`, `debug`) |
| `DATA_DIR` | `/data` | Persistent volume root |
| `DOCS_ENABLED` / `DOCS_PUBLIC` | `true` / `false` | Controls `/docs` and `/openapi.yaml` availability & auth |
| `MEDIA_MAX_UPLOAD_BYTES` | `26214400` | Max upload size (25 MiB) |
| `SESSION_RATE_LIMIT_PER_MINUTE` | `60` | Per-session outbound message rate limit |
| `SQLITE_JOURNAL_MODE` / `SQLITE_BUSY_TIMEOUT_MS` | `wal` / `5000` | SQLite WAL & lock timeout (use local disk, not NFS) |
| `MEDIA_STORAGE` | `local` | `local` (`/data/media`) or `s3` (AWS S3, Cloudflare R2, MinIO) |
| `S3_ENDPOINT_URL`, `S3_REGION`, `S3_BUCKET`, `S3_ACCESS_KEY_ID`, `S3_SECRET_ACCESS_KEY`, `S3_PREFIX` | `auto`, `softagon-relay/` | S3-compatible object storage settings (used when `MEDIA_STORAGE=s3`) |
| `TRANSCRIPTION_ENABLED` | `true` | Automatically transcribe incoming voice notes |
| `TRANSCRIPTION_MODEL` | `base` | Whisper GGML model: `tiny`, `base`, `small`, `medium`, `large` |
| `TRANSCRIPTION_LANGUAGE` | `auto` | Automatic language detection or ISO code (`en`, `pt`, `es`, etc.) |
| `TRANSCRIPTION_PRELOAD` | `false` | Lazy-load on first audio (`false`) or load on startup (`true`) |
| `TRANSCRIPTION_IDLE_UNLOAD_SECONDS` | `180` | Unload Whisper model after idle timeout to free RAM |
| `TRANSCRIPTION_THREADS` / `TRANSCRIPTION_WORKERS` | `1` / `1` | Inference threads and concurrent workers |
| `TRANSCRIPTION_MAX_QUEUE` / `TRANSCRIPTION_TIMEOUT_SECONDS` | `20` / `180` | Queue bound and per-job timeout |
| `WEBHOOK_DELIVERY_TIMEOUT_SECONDS` / `WEBHOOK_MAX_ATTEMPTS` / `WEBHOOK_BACKOFF_BASE_SECONDS` | `10` / `8` / `2` | Webhook HTTP timeout and retry backoff policy |

---

## Frequently Asked Questions (FAQ)

### Does Softagon Relay require PostgreSQL, Redis, or FFmpeg?
No. Softagon Relay uses embedded **SQLite (WAL)** for application state, webhook outbox, and job queues, and decodes WhatsApp OGG/Opus voice notes using pure-Rust **Symphonia + `symphonia-opus`** and **Rubato** without requiring FFmpeg.

### How does local WhatsApp voice-note transcription work?
When an audio message arrives, Softagon Relay stores the decrypted media, enqueues an asynchronous job in SQLite, decodes OGG/Opus to 16 kHz mono PCM, runs `whisper.cpp` (default: multilingual **Whisper Base**) on a dedicated blocking thread, persists the transcript, deletes temporary decoded audio, and emits a signed `message.transcription.completed` webhook.

### Can I store WhatsApp media in Cloudflare R2, MinIO, or AWS S3?
Yes. Set `MEDIA_STORAGE=s3` along with `S3_BUCKET`, `S3_ACCESS_KEY_ID`, `S3_SECRET_ACCESS_KEY`, and `S3_ENDPOINT_URL` (for R2/MinIO). Media bytes stay out of SQLite while metadata and SHA-256 checksums are indexed in `/data/relay.db`.

### Where is persistent data stored inside the container?
All persistent state lives in `/data`:
- `/data/relay.db` — Relay state, messages, webhooks, outbox, and transcription jobs
- `/data/whatsapp-sessions/` — WhatsApp Web protocol session databases (**treat as secrets**)
- `/data/models/` — Lazily downloaded Whisper GGML models
- `/data/media/` — Local media files when `MEDIA_STORAGE=local`
- `/data/tmp/` — Temporary audio files (automatically removed after transcription)

### Is Softagon Relay an official Meta / WhatsApp API?
No. Softagon Relay is an independent, unofficial open-source project built on `whatsapp-rust` and is not affiliated with, authorized by, or endorsed by Meta or WhatsApp. Always review Meta's Terms of Service, deploy behind TLS (reverse proxy/ingress), and encrypt backups of `/data`.
