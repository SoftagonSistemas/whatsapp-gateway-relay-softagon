#!/usr/bin/env bash
set -euo pipefail

# Idempotent daemon watchdog for Grok Bot computers.
# Safe to run at the start of any task (including after "Update Bot's Computer").

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [[ -n "${RELAY_HOME:-}" ]]; then
  BASE_DIR="${RELAY_HOME}"
elif [[ -x "${SCRIPT_DIR}/bin/softagon-relay" || -f "${SCRIPT_DIR}/.env" ]]; then
  BASE_DIR="${SCRIPT_DIR}"
else
  BASE_DIR="/workspace/softagon-relay"
fi
BIN="${BASE_DIR}/bin/softagon-relay"
ENV_FILE="${BASE_DIR}/.env"
LOG_FILE="${BASE_DIR}/relay.log"
PID_FILE="${BASE_DIR}/relay.pid"

if [[ -f "${ENV_FILE}" ]]; then
  set -a
  # shellcheck disable=SC1090
  source "${ENV_FILE}"
  set +a
fi

PORT="${APP_PORT:-8080}"
if curl -fsS "http://127.0.0.1:${PORT}/v1/health" >/dev/null 2>&1; then
  exit 0
fi

if [[ ! -x "${BIN}" ]]; then
  echo "Softagon Relay binary not found at ${BIN}. Run setup.sh first." >&2
  exit 1
fi

nohup "${BIN}" serve >> "${LOG_FILE}" 2>&1 &
echo $! > "${PID_FILE}"

for _ in $(seq 1 20); do
  if curl -fsS "http://127.0.0.1:${PORT}/v1/health" >/dev/null 2>&1; then
    exit 0
  fi
  sleep 0.5
done

echo "Softagon Relay failed to start; check ${LOG_FILE}" >&2
exit 1
