# Grok Bot Template Blueprint — WhatsApp Assistant with Softagon Relay

> **1-Click Import:** **[Add Softagon Relay to Grok Bot](https://x.ai/bot/Xe4LpCGaepuE-J2BYH363)**  
> Or copy the instructions below into your Grok Bot (**Conhecimento → Habilidades** / **Settings → Instructions**).
> Follows xAI's *"Recipe, Not a Meal"* template standard: **contains zero API keys, phone numbers, or personal credentials**.

---

## Bot Instructions (Copy & Paste)

You have access to **Softagon Relay**, a local self-hosted WhatsApp gateway installed in `${RELAY_HOME:-/workspace/softagon-relay}` on this persistent computer (always detect `RELAY_HOME` dynamically if the user installed it in a custom subfolder such as `/workspace/<project>/relay`).

### 1. Always Ensure Relay Is Running First
Before any WhatsApp action, run:
```bash
"${RELAY_HOME:-/workspace/softagon-relay}/ensure-running.sh"
```
(If not installed yet, ask the user for confirmation and install it with `setup.sh` — it automatically sets `WEBHOOK_COALESCE_SECONDS=4` so rapid client message bursts arrive grouped in a single routine execution, preserving Grok Bot's 200 executions/hour budget, and generates a matching `softagon-relay.service`.)

### 2. Pairing WhatsApp (QR Code First — Zero Login!)
Never ask the user to type `RELAY_API_KEY` into the browser login screen, and never build ad-hoc HTML pages.
1. Check existing sessions via MCP `list_sessions` (or `GET http://127.0.0.1:8080/v1/sessions`).
2. If no connected session exists, create one with MCP `create_session` (or `POST /v1/sessions`).
3. Call MCP `create_pairing_link` (or `POST /v1/sessions/{id}/pair-link`).
4. **Open `pair_url` (`http://127.0.0.1:8080/pair/<token>`) directly in Chrome** and let the user scan the **QR Code** with their phone (**WhatsApp → Linked devices → Link a device**).
   - The page requires **zero login**, never exposes `RELAY_API_KEY`, and **auto-renews the QR code continuously for up to 10 minutes** so the user is never blocked by WhatsApp's 60-second QR rotation.
   - If the user prefers an image in chat, download `qr_png_url` to `${RELAY_HOME:-/workspace/softagon-relay}/results/whatsapp-qr.png` and share it.
   - Only if the user says they cannot use their phone camera, offer the **8-digit code** tab on the pairing page (or call MCP `request_pair_code` with their phone number).

### 3. Webhooks, Reading, Transcribing & Sending Messages
- **Direct Webhook to Grok Bot Routines:** register a webhook with `POST /v1/webhooks` passing `"bearer_token": "<routine-token>"` (or `auth_header_name` / `auth_header_value`). When `events` is omitted, Relay defaults to `["message.received", "message.sent", "session.disconnected"]` so you also get alerted immediately if WhatsApp disconnects or logs out.
- **Burst Coalescing & Rate-Limit Safety:** with `WEBHOOK_COALESCE_SECONDS=4` (enabled by `setup.sh`), rapid consecutive messages from the same client are coalesced into one `message.received` event (`coalesced: true`, `combined_text`, `messages[]`), and Relay respects HTTP `429` + `Retry-After` headers.
- **Rich Context & Presence:** incoming events include `sender_name`, `sender_phone` (resolved and auto-migrated even for `@lid` contacts), `from_me` / `source: "device_sync"`, `quoted` (with SQLite fallback), `message.edited` / `message.revoked`, and automatic `chat.presence` subscription for active 1:1 chats (plus `subscribe_presence`).
- Prefer the local **MCP Streamable HTTP** server (`http://127.0.0.1:8091/mcp`) or REST API (`http://127.0.0.1:8080/v1/*`) using the key stored in `${RELAY_HOME:-/workspace/softagon-relay}/.env` (never print or echo `RELAY_API_KEY` in chat).
- Available MCP tools include `get_contact`, `list_contacts`, `save_contact`, `check_whatsapp`, `get_profile_picture`, `list_groups`, `get_group_info`, `subscribe_presence`, `send_chat_presence`, `mark_read`, `send_text_message` (with `reply_to_message_id`), `send_media_message` (`media_url` or `data_base64`, `as_voice_note`, `sticker`), `send_reaction`, `send_location`, `send_contact`, `edit_message`, and `revoke_message`.
- Always pass an `idempotency_key` when calling `send_text_message` or `send_media_message` so retries never send duplicate messages.
- For multi-bot handoffs or user-facing reports, save exported summaries, transcripts, or media under `${RELAY_HOME:-/workspace/softagon-relay}/results/` and reference the file path clearly.
- Always confirm recipient and message content with the user before sending outbound WhatsApp messages unless the user explicitly authorized autonomous replies.

### 4. Persistence & Troubleshooting
- All WhatsApp sessions and SQLite data live in `${RELAY_HOME:-/workspace/softagon-relay}/data/` and persist across tasks and across all bots on this computer.
- If the computer was updated (**Settings → Computer → Update Bot's Computer**), simply run `"${RELAY_HOME:-/workspace/softagon-relay}/ensure-running.sh"` — your WhatsApp session reconnects automatically without scanning a new QR code.
- Never run `logout_session` or `delete_session` unless explicitly instructed by the user.
