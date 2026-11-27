# Using Softagon Relay with Grok Bot

**Softagon Relay** is designed to be the fastest, cleanest way to connect **WhatsApp** to **Grok Bot** on its persistent Linux cloud computer — with **zero Docker overhead**, **zero browser login friction**, **auto-renewing QR codes**, and **local persistence in `/workspace`**.

---

## Why Softagon Relay Fits Grok Bot Perfectly

| Grok Bot Concept | How Softagon Relay Adapts |
|---|---|
| **Computer & Apps** (`/workspace`) | Runs as a single native Rust binary (~38 MiB RAM) stored in `/workspace/softagon-relay/` — no Chromium, Node.js, PostgreSQL, or Redis. |
| **Zero-Login QR Pairing** | `create_pairing_link` (`POST /v1/sessions/{id}/pair-link`) opens `http://127.0.0.1:8080/pair/<token>` directly in Chrome **without asking for `RELAY_API_KEY`**. |
| **10-Minute Auto-Renewing QR** | WhatsApp rotates QR tokens every ~20–60s. Our `/pair/<token>` page automatically renews expired QR cycles for up to **10 minutes** so you never hit a dead QR screen. |
| **8-Digit Code Fallback** | If you don't have a camera handy, `/pair/<token>` and MCP `request_pair_code` also support WhatsApp's native 8-character phone pairing code. |
| **Files & Results** | Download `qr.png` (`/pair/<token>/qr.png`) or save voice-note transcriptions and chat reports to `/workspace/softagon-relay/results/` for clean handoffs across bots. |
| **Multi-Bot Shared Computer** | One `softagon-relay serve` process owns the WhatsApp WebSocket and SQLite DB; multiple Grok Bots connect safely via REST (`127.0.0.1:8080`) or MCP Streamable HTTP (`127.0.0.1:8091/mcp`). |
| **Direct Routine Webhooks** | `POST /v1/webhooks` supports `bearer_token` (or custom auth headers) out of the box, emits `sender_name`, `sender_phone`, and `from_me` phone replies, suppresses history-sync floods, and stops retrying on `401`/`403`. |

---

## 1. Quick Install on Your Grok Bot Computer

Run inside your Grok Bot's terminal (installs into `/workspace/softagon-relay` so data persists across tasks):

```bash
curl -fsSL https://raw.githubusercontent.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/main/integrations/grok-bot/setup.sh -o /tmp/setup-relay.sh
curl -fsSL https://raw.githubusercontent.com/SoftagonSistemas/whatsapp-gateway-relay-softagon/main/integrations/grok-bot/ensure-running.sh -o /tmp/ensure-running.sh
chmod +x /tmp/setup-relay.sh /tmp/ensure-running.sh
/tmp/setup-relay.sh
```

---

## 2. Pair WhatsApp in 10 Seconds (QR Code First, Zero Login)

Ask your Grok Bot to run (or call via MCP `create_session` + `create_pairing_link`):

```bash
source /workspace/softagon-relay/.env
SESS_ID=$(curl -fsS -X POST http://127.0.0.1:8080/v1/sessions \
  -H "Authorization: Bearer $RELAY_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"name":"grok-bot-whatsapp"}' | jq -r '.id')

curl -fsS -X POST "http://127.0.0.1:8080/v1/sessions/${SESS_ID}/pair-link" \
  -H "Authorization: Bearer $RELAY_API_KEY"
```

Open the returned `http://127.0.0.1:8080/pair/<token>` directly in Chrome on the Grok Bot computer:
- **No login screen, no pasting `RELAY_API_KEY`.**
- Point your phone (**WhatsApp → Linked devices → Link a device**) at the QR code on your computer screen.
- If you take more than 1 minute, the page **automatically generates a fresh QR cycle** until paired (up to 10 minutes).
- Once connected, the temporary token is **invalidated immediately**.

---

## 3. Ready-to-Use Grok Bot Template (1-Click Import)

- **1-Click Import on xAI:** **[Add Softagon Relay to Grok Bot](https://x.ai/bot/Xe4LpCGaepuE-J2BYH363)**
- **Manual copy:** [`integrations/grok-bot/TEMPLATE.md`](../integrations/grok-bot/TEMPLATE.md) — follows xAI's *"Recipe, Not a Meal"* guideline (zero personal secrets or hardcoded keys).

---

## 4. Persistence, Updates & Troubleshooting

- **After `Settings → Computer → Update Bot's Computer`:** background processes may restart, while `/workspace/softagon-relay/data/` persists. Run `/workspace/softagon-relay/ensure-running.sh` — Relay starts back up and restores your WhatsApp session automatically without re-scanning a QR code.
- **Avoid `Reset Computer`** unless necessary: resetting wipes `/workspace`, which would delete `/workspace/softagon-relay/data/` and require re-pairing WhatsApp.
- **Multiple Grok Bots on the same account:** all your bots share `/workspace` and `127.0.0.1`. Keep a **single** `softagon-relay serve` running (via `ensure-running.sh`) and let every bot talk to it over `http://127.0.0.1:8080/v1/*` or MCP Streamable HTTP `http://127.0.0.1:8091/mcp`.
- **Check logs:** `tail -n 50 /workspace/softagon-relay/relay.log`
