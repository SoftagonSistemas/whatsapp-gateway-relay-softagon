#!/usr/bin/env bash
set -euo pipefail

# Idempotent daemon watchdog for Grok Bot computers.
# Safe to run at the start of any task (including after "Update Bot's Computer").

BASE_DIR="${RELAY_HOME:-/workspace/softagon-relay}"
BIN="${BASE_DIR}/bin/softagon-relay"
ENV_FILE="${BASE_DIR}/.env"
LOG_FILE="${BASE_DIR}/relay.log"
PID_FILE="${BASE_DIR}/relay.pid"

if curl -fsS "http://127.0.0.1:8080/v1/health" >/dev/null 2>&1; then
  exit 0
fi

if [[ ! -x "${BIN}" ]]; then
  echo "Softagon Relay binary not found at ${BIN}. Run setup.sh first." >&2
  exit 1
fi

set -a
# shellcheck disable=SC1090
source "${ENV_FILE}"
set +a

nohup "${BIN}" serve >> "${LOG_FILE}" 2>&1 &
echo $! > "${PID_FILE}"

for _ in $(seq 1 20); do
  if curl -fsS "http://127.0.0.1:8080/v1/health" >/dev/null 2>&1; then
    exit 0
  fi
  sleep 0.5
done

echo "Softagon Relay failed to start; check ${LOG_FILE}" >&2
exit 1
