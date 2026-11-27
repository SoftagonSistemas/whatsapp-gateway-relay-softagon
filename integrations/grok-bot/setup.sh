#!/usr/bin/env bash
set -euo pipefail

# Softagon Relay — One-command setup for Grok Bot persistent Linux computer (/workspace)
# Persists binary, .env, SQLite DB, media, and Whisper models in /workspace/softagon-relay
# so WhatsApp sessions survive across tasks and are shared across all your Grok Bots.

BASE_DIR="${RELAY_HOME:-/workspace/softagon-relay}"
BIN_DIR="${BASE_DIR}/bin"
DATA_DIR="${BASE_DIR}/data"
RESULTS_DIR="${BASE_DIR}/results"
ENV_FILE="${BASE_DIR}/.env"
REPO="SoftagonSistemas/whatsapp-gateway-relay-softagon"

mkdir -p "${BIN_DIR}" "${DATA_DIR}/media" "${DATA_DIR}/models" "${RESULTS_DIR}"

echo "=== Installing Softagon Relay into ${BASE_DIR} ==="
LATEST_TAG="${RELAY_VERSION:-$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep -m1 '"tag_name"' | cut -d'"' -f4)}"
TARBALL="softagon-relay-${LATEST_TAG}-linux-x86_64.tar.gz"
DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${LATEST_TAG}/${TARBALL}"

curl -fsSL "${DOWNLOAD_URL}" -o "/tmp/${TARBALL}"
tar -xzf "/tmp/${TARBALL}" -C "${BIN_DIR}"
chmod +x "${BIN_DIR}/softagon-relay"
rm -f "/tmp/${TARBALL}"

if [[ ! -f "${ENV_FILE}" ]]; then
  KEY="$(openssl rand -hex 32)"
  cat > "${ENV_FILE}" <<EOF
APP_HOST=127.0.0.1
APP_PORT=8080
RUST_LOG=info
RELAY_API_KEY=${KEY}
DATA_DIR=${DATA_DIR}
RELAY_DATABASE_PATH=${DATA_DIR}/relay.db
WHATSAPP_DATABASE_PATH=${DATA_DIR}/whatsapp-sessions.db
MEDIA_STORAGE=local
MEDIA_LOCAL_PATH=${DATA_DIR}/media
TRANSCRIPTION_ENABLED=true
TRANSCRIPTION_MODEL=base
TRANSCRIPTION_LANGUAGE=auto
TRANSCRIPTION_MODEL_DIR=${DATA_DIR}/models
TRANSCRIPTION_PRELOAD=false
TRANSCRIPTION_IDLE_UNLOAD_SECONDS=180
MCP_HTTP_ENABLED=true
MCP_HTTP_HOST=127.0.0.1
MCP_HTTP_PORT=8091
EOF
  chmod 600 "${ENV_FILE}"
  echo "Created ${ENV_FILE} with a random 256-bit RELAY_API_KEY."
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [[ -f "${SCRIPT_DIR}/ensure-running.sh" ]]; then
  cp "${SCRIPT_DIR}/ensure-running.sh" "${BASE_DIR}/ensure-running.sh"
  chmod +x "${BASE_DIR}/ensure-running.sh"
  "${BASE_DIR}/ensure-running.sh"
fi

echo ""
echo "Softagon Relay is installed and running!"
echo "  Health:   http://127.0.0.1:8080/v1/health"
echo "  MCP HTTP: http://127.0.0.1:8091/mcp"
echo "  Data dir: ${DATA_DIR}"
echo "  Tip:      Create a session and call POST /v1/sessions/{id}/pair-link (or MCP create_pairing_link)"
echo "            to open a zero-login, auto-renewing QR page in Chrome!"
