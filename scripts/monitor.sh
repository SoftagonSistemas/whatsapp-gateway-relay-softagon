#!/usr/bin/env bash
set -euo pipefail

CONTAINER="${RELAY_CONTAINER:-softagon-relay}"
HOST_PORT="${HOST_PORT:-8090}"
LOG_FILE="${MONITOR_LOG_FILE:-/tmp/softagon-relay-monitor.log}"

if [[ -f .env ]]; then
  RELAY_API_KEY="${RELAY_API_KEY:-$(grep -E '^RELAY_API_KEY=' .env | cut -d= -f2-)}"
  HOST_PORT="$(grep -E '^HOST_PORT=' .env | cut -d= -f2- || echo "$HOST_PORT")"
fi

TS="$(date '+%Y-%m-%d %H:%M:%S %Z')"
STATS="$(docker stats --no-stream --format 'CPU={{.CPUPerc}} MEM={{.MemUsage}} ({{.MemPerc}}) NET={{.NetIO}} BLOCK={{.BlockIO}} PIDS={{.PIDs}}' "$CONTAINER" 2>/dev/null || echo 'CONTAINER_NOT_RUNNING')"
HEALTH="$(curl -fsS "http://127.0.0.1:${HOST_PORT}/v1/health" 2>/dev/null || echo '{"status":"unreachable"}')"
SESSIONS="$(curl -fsS -H "Authorization: Bearer ${RELAY_API_KEY:-}" "http://127.0.0.1:${HOST_PORT}/v1/sessions" 2>/dev/null || echo '{"sessions":[]}')"
ERR_COUNT="$(docker logs --since 30m "$CONTAINER" 2>&1 | grep -ciE '"level":"ERROR"|panic|fatal' || true)"

LINE="[$TS] $STATS | ERRORS_30M=$ERR_COUNT | HEALTH=$HEALTH | SESSIONS=$SESSIONS"
echo "$LINE" | tee -a "$LOG_FILE"
