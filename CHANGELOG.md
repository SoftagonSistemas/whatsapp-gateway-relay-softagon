# Changelog — Softagon Relay

All notable changes to **Softagon Relay** are documented in this file, following [Keep a Changelog](https://keepachangelog.com/) and [Semantic Versioning](https://semver.org/).

> **Quick links:** [English Docs](docs/en/README.md) · [Português (BR)](docs/pt-BR/README.md) · [Español](docs/es/README.md) · [Français](docs/fr/README.md) · [Bahasa Indonesia](docs/id/README.md) · [Basa Betawi](docs/bew/README.md) · [Basa Sunda](docs/su/README.md) · [Basa Jawa](docs/jv/README.md) · [Hata Batak Toba](docs/bbc/README.md) · [Bahasa Melayu](docs/ms/README.md) · [Docker Hub](https://hub.docker.com/r/softagon/relay) · [Softagon Sistemas](https://softagon.com.br)

---

## [0.4.3] — 2026-11-28

### Added — Full Address-Book Sync for Existing Sessions
- **Automatic First-Connect Contact Backfill & Full Address-Book Sync:** when an already-paired session upgrades to `v0.4.3` and connects, Relay automatically backfills `contacts` from existing `messages` history and triggers a full `CriticalUnblockLow` WhatsApp address-book snapshot sync whenever no address-book `full_name` entries exist yet — **no QR re-pairing required**.
- **On-Demand Sync Endpoint & MCP Tool:** added `POST /v1/sessions/{id}/contacts/sync` and MCP `sync_contacts` to refresh the phone's full WhatsApp address book at any time.

---

## [0.4.2] — 2026-11-28

### Added — Persistent Contacts Directory & Address-Book Sync
- **Dedicated SQLite `contacts` Table (`migrations/0005_contacts.sql`):** stores `(session_id, jid, lid, phone_number, full_name, first_name, push_name, updated_at)` automatically populated from WhatsApp's `app-state` address-book sync (`Event::ContactUpdate`), real-time `Event::PushNameUpdate`, and inbound messages.
- **Smart Contact Name Resolution:** `list_chats`, `get_contact`, and inbound `message.received` webhooks now prioritize the address-book `full_name` (when saved on the phone) with automatic fallback to `push_name`.
- **Contacts Directory Endpoints & MCP Tools:**
  - `GET /v1/sessions/{id}/contacts?q=...&limit=...` & MCP `list_contacts`: search saved contacts by name, phone number, JID, or `@lid`.
  - `PUT /v1/sessions/{id}/contacts/{jid}` & MCP `save_contact`: save or rename a contact directly in the phone's WhatsApp address book (`client.chat_actions().save_contact`) and SQLite.

---

## [0.4.1] — 2026-11-28

### Added
- **Webhook Burst Coalescing (`WEBHOOK_COALESCE_SECONDS`):** optional per-chat debounce window (global default `0` = immediate; pre-configured to `4` in the Grok Bot kit) that merges rapid consecutive inbound messages from the same `chat_jid` into a single `message.received` webhook with `coalesced: true`, `coalesced_count`, `combined_text`, and `messages[]` — preserving hourly routine execution quotas.
- **Dedicated Disconnection Alerts (`session.disconnected`):** new webhook event emitted whenever a session becomes `disconnected` or `logged_out` (with `requires_reauth` flag) and included automatically in default webhook subscriptions (`["message.received", "message.sent", "session.disconnected"]`).
- **Presence Subscription (`subscribe_presence`):** automatically subscribes to WhatsApp presence updates for active 1:1 chats so `chat.presence` events (`composing`/`recording`/`online`) are delivered, plus explicit `POST /v1/sessions/{id}/contacts/{jid}/presence-subscribe` and MCP `subscribe_presence`.
- **SQLite Fallback for `quoted.text`:** when WhatsApp omits `quoted_message` content on a reply, Relay now looks up the original message in SQLite by `stanza_id` (`text_body`, `caption`, `transcription`, or `[type]`).
- **Automatic Legacy `@lid` History Migration:** when a session connects, Relay automatically migrates legacy `@lid` rows in SQLite to canonical `{phone}@s.whatsapp.net` JIDs (preserving `metadata_json.chat_lid`), unifying pre-`0.3.11` conversations.

### Fixed
- **Clean `list_chats`:** filters out `status@broadcast` and `@newsletter` entries directly in SQL.
- **Dynamic `RELAY_HOME` in Grok Bot Kit:** removed hardcoded `/workspace/softagon-relay` paths from `integrations/grok-bot/softagon-relay.service` and `integrations/grok-bot/TEMPLATE.md`, and enabled `WEBHOOK_COALESCE_SECONDS=4` by default in `setup.sh`.

---

## [0.4.0] — 2026-11-27

### Added — Full Interactive WhatsApp Messaging, Groups, Contacts & Observability
- **Rich Outbound Messaging (REST + MCP):**
  - **Quoted Replies (`reply_to_message_id`):** reply/quote any existing WhatsApp message on both `send_text` and `send_media`.
  - **Reactions (`POST /v1/sessions/{id}/messages/reaction` & MCP `send_reaction`):** react with any emoji or remove a reaction (`emoji: ""`).
  - **Chat Presence (`POST /v1/sessions/{id}/chats/presence` & MCP `send_chat_presence`):** send typing (`composing`), recording audio (`recording`), or `paused` indicators.
  - **Voice Notes (`as_voice_note: true`):** send `audio` messages as WhatsApp Push-To-Talk (PTT) voice notes.
  - **Stickers (`media_type: "sticker"`):** send WebP stickers via URL or base64.
  - **Location Pins (`POST /v1/sessions/{id}/messages/location` & MCP `send_location`):** send latitude/longitude pins with optional place name and address.
  - **Contact Cards (`POST /v1/sessions/{id}/messages/contact` & MCP `send_contact`):** send vCard 3.0 contact cards (auto-generated from `display_name` + `phone_number`, or raw `vcard`).
  - **Edit & Revoke Messages (`PATCH` / `DELETE /v1/sessions/{id}/messages/{message_id}` & MCP `edit_message` / `revoke_message`):** edit previously sent text messages or delete messages for everyone.
- **Inbound Edit & Revoke Webhook Events:** new subscribable `message.edited` and `message.revoked` events with automatic SQLite state synchronization.
- **Contacts & Groups Discovery (REST + MCP):**
  - `POST /v1/sessions/{id}/contacts/check` & MCP `check_whatsapp`: check whether phone numbers are registered on WhatsApp.
  - `GET /v1/sessions/{id}/contacts/{jid}/profile-picture` & MCP `get_profile_picture`: fetch contact/group profile picture URLs (`?preview=true` optional).
  - `GET /v1/sessions/{id}/groups` & `GET /v1/sessions/{id}/groups/{group_jid}` (+ MCP `list_groups` / `get_group_info`): inspect participating groups, subjects, descriptions, and admin/member rosters.
- **Media Download & Retention:**
  - `GET /v1/media/{media_id}`: authenticated direct binary download of stored media with proper `Content-Type` and `Content-Disposition`.
  - `MEDIA_RETENTION_DAYS`: optional automatic hourly purge of expired media files to bound disk usage (`0` = keep forever).
- **Webhook Rate-Limit Awareness & Inline Audio Transcription:**
  - Webhook dispatcher now honors HTTP `429 Too Many Requests` + `Retry-After` (clamped `1..=3600s`) without burning retry attempts against upstream rate limits.
  - `TRANSCRIPTION_INLINE_WAIT_SECONDS`: optional wait window (default `0`) so `message.received` webhooks can include `transcription` inline when local Whisper finishes within the budget.
- **Operations & Observability:**
  - `GET /v1/metrics`: Prometheus text-format metrics (`softagon_relay_sessions_total`, `softagon_relay_sessions_connected`, `softagon_relay_messages_total`, `softagon_relay_webhook_deliveries_total`, `softagon_relay_media_objects_active`, `softagon_relay_media_storage_bytes`).
  - `softagon-relay backup [output-dir]`: online WAL-safe SQLite backup via `VACUUM INTO` for `relay.db` and `whatsapp-sessions.db`.

---

## [0.3.12] — 2026-11-27

### Fixed
- **Retroactive `@lid` ↔ Phone History Unification:** `get_chat_messages` now resolves the `@lid` ↔ `{phone}@s.whatsapp.net` pair automatically and queries both JIDs, so even older pre-`0.3.11` `@lid` messages appear unified when searching by phone number (and vice versa).
- **Outbound `@lid` Canonicalization:** `send_text` and `send_media` now resolve `@lid` recipients via `get_contact` before storing `chat_jid` in SQLite (preserving the original `@lid` in `metadata_json.chat_lid`), preventing split threads when replying directly to an `@lid`.
- **Media Placeholders in `quoted.text`:** when a user replies to an audio, image, video, document, sticker, location, or contact without text/caption, `quoted.text` now falls back to `[audio]`, `[image]`, `[video]`, `[document]`, `[sticker]`, `[location]`, or `[contact]` instead of `null`.
- **Dynamic Systemd Unit Generation:** `integrations/grok-bot/setup.sh` now writes `${RELAY_HOME}/softagon-relay.service` using the actual installation directory instead of a hardcoded path.

---

## [0.3.11] — 2026-11-27

### Added
- **Systemd Unit & Auto-Detecting `ensure-running.sh`:** added `integrations/grok-bot/softagon-relay.service` and updated `integrations/grok-bot/ensure-running.sh` to auto-detect its installation directory (`SCRIPT_DIR` / `RELAY_HOME` / `APP_PORT`) so it works in any folder (e.g., `/workspace/popy/relay` or `/workspace/softagon-relay`).
- **"Open Web Console" CTA on `/pair/{token}` Success Screen:** localized link to `/console` after pairing completes.

### Fixed
- **Unified `@lid` ↔ Phone Conversation History:** 1:1 `@lid` chats are now normalized to `{phone}@s.whatsapp.net` whenever resolved (preserving the original `@lid` in `metadata_json.chat_lid`), outbound `send_text`/`send_media` normalize numeric recipients to `@s.whatsapp.net`, and `get_chat_messages` queries both `chat_jid` and `chat_lid` so client history is never split across two threads.
- **Keep Inbound Media Messages Even When Download Fails:** `convert_inbound_message` no longer drops `audio`, `image`, `video`, or `document` messages when CDN media download fails — the message is still stored and emitted with `message_type`, `caption`, and `media_id: null`.
- **Safe Webhook Defaults & `@newsletter` Filtering:** webhooks created with an empty `events` list now default to `["message.received", "message.sent"]` (explicit `"*"` still subscribes to everything), and `@newsletter` channel updates are filtered alongside `status@broadcast`.
- **Eliminate `SQLITE_BUSY` Contention Under Bursts:** serialized SQLite WAL access through a single pooled connection with a minimum 15s `busy_timeout`, eliminating transient `database is locked` warnings in the transcription worker.
- **Updated MCP Server Instructions:** `ServerHandler::get_info` now documents all 14 MCP tools.

---

## [0.3.10] — 2026-11-27

### Added — Presence Events, Quoted Replies & Group Metadata
- **Presence Events (`chat.presence`):** maps `whatsapp-rust` `Event::ChatPresence` (`composing`, `recording`, `paused`) and `Event::Presence` (`available`, `unavailable`, `last_seen`) to a new subscribable webhook event `chat.presence` (ignoring `status@broadcast`).
- **Quoted Message & Group Flag (`quoted`, `is_group`):** extracts `stanza_id`, `participant`, and quoted text/caption from WhatsApp `ContextInfo` and includes `quoted` and `is_group` in both SQLite `metadata_json` and `message.received` / `message.sent` webhook payloads.

### Fixed
- **Filter Out `status@broadcast` (WhatsApp Stories):** `Event::Messages` and `Event::Receipt` now drop `status@broadcast` events at the adapter boundary so WhatsApp Stories never flood webhooks or consume routine execution quotas.
- **Live Status in `GET /v1/sessions/{id}` & Phone/JID on `Connected`:** `SessionManager::get_session` now merges live engine status (matching `list_sessions`), and `Event::Connected` extracts the own device `phone_number` and `jid` from `persistence_manager().get_device_snapshot().pn` so both SQLite and `session.status` webhooks include them immediately.
- **Real Message Timestamp in `created_at` / `list_chats.last_message_at`:** `insert_message` now persists the real WhatsApp `info.timestamp` in `created_at` so chat ordering and `last_message_at` remain accurate even during history sync.

---

## [0.3.9] — 2026-11-27

### Added
- **Browser Language Auto-Detection on `/pair/{token}`:** the zero-login temporary pairing page now automatically detects `navigator.languages` and renders in Português (`pt`), Español (`es`), Français (`fr`), Bahasa Indonesia (`id`), Bahasa Melayu (`ms`), or English (`en`).

### Fixed
- **Reconnecting `logged_out` Sessions Generates a Fresh QR Code:** `SessionManager::reconnect_session` now clears revoked WhatsApp SQLite credentials (`delete_session_data`) when reconnecting a `logged_out` session so WhatsApp does not reject the reconnect with `401` and immediately issues a new QR code.
- **Hidden Broken QR Image on `/pair/{token}`:** fixed CSS specificity (`.hide { display: none !important; }`) so the QR `<img>` stays hidden while generating/renewing a cycle, and added `logged_out` to automatic QR renewal.

---

## [0.3.8] — 2026-11-27

### Added — Complete Grok Bot Feature Set & 10-Language Documentation Sync
- **Read Receipts (`mark_read`):** added `POST /v1/sessions/{id}/chats/read` and MCP tool `mark_read` backed by `whatsapp-rust`'s `client.mark_as_read(...)`. Automatically marks the latest inbound messages when `message_ids` is omitted so linked phones do not accumulate unread badges.
- **Contact & `@lid` Resolution (`get_contact`):** added `GET /v1/sessions/{id}/contacts/{jid}` and MCP tool `get_contact` combining WhatsApp's `get_lid_pn_entry` LID-to-PN cache with SQLite message history (`push_name`, `phone_number`, canonical `jid`). Also uses `get_lid_pn_entry` as fallback during inbound `@lid` message conversion.
- **Media Sending via URL (`media_url`) & MCP `send_media_message`:** `POST /v1/sessions/{id}/messages/media` and the new MCP tool `send_media_message` now accept either a public HTTP/HTTPS `media_url` (with MIME/filename auto-detection and size limit enforcement) or `data_base64`, with full `idempotency_key` deduplication.
- **Live Session Health in `/v1/health`:** `/v1/health` now queries live engine session states and exposes `active_sessions`, `total_sessions`, and `sessions_healthy`.
- **10-Language Regional Docs Updated:** added section `6.2` covering Grok Bot, zero-login `/pair/{token}`, 8-digit pairing codes, webhook Bearer auth, `@lid` resolution, `mark_read`, and `media_url` across all 10 language guides (`pt-BR`, `en`, `es`, `fr`, `id`, `bew`, `su`, `jv`, `bbc`, `ms`).

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
