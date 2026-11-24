#!/usr/bin/env bash
set -euo pipefail

IMAGE="${BENCH_IMAGE:-softagon/relay:dev}"
API_KEY="${BENCH_API_KEY:-bench-secret-key-$(date +%s)}"
PORT="${BENCH_PORT:-18080}"
CONTAINER_NAME="softagon-relay-bench-$$"
DATA_DIR="$(mktemp -d)"

cleanup() {
  docker rm -f "$CONTAINER_NAME" >/dev/null 2>&1 || true
  rm -rf "$DATA_DIR"
}
trap cleanup EXIT

echo "=== Softagon Relay Benchmark Report ==="
echo "Timestamp: $(date -u +"%Y-%m-%dT%H:%M:%SZ")"
echo "Host OS/Kernel: $(uname -srmo)"
echo "CPU: $(grep -m1 'model name' /proc/cpuinfo 2>/dev/null | cut -d: -f2- | xargs || nproc)"
echo "CPUs: $(nproc)"
echo "Docker: $(docker --version)"
echo "Image: $IMAGE"
echo "Image Digest/ID: $(docker image inspect --format '{{.Id}} ({{.Size}} bytes)' "$IMAGE")"
echo ""

# Adjust permissions for non-root container user (UID 10001)
chmod 777 "$DATA_DIR"

echo "[1/4] Starting container (1 vCPU / 2 GB RAM limit profile)..."
docker run -d \
  --name "$CONTAINER_NAME" \
  --cpus="1.0" \
  --memory="2g" \
  --read-only \
  --tmpfs /tmp:size=128m,noexec,nosuid \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  -e RELAY_API_KEY="$API_KEY" \
  -e DOCS_ENABLED=true \
  -e DOCS_PUBLIC=true \
  -e TRANSCRIPTION_ENABLED=true \
  -e TRANSCRIPTION_PRELOAD=false \
  -p "127.0.0.1:${PORT}:8080" \
  -v "${DATA_DIR}:/data" \
  "$IMAGE" >/dev/null

# Wait for readiness
for i in $(seq 1 30); do
  if curl -fsS "http://127.0.0.1:${PORT}/v1/health" >/dev/null 2>&1; then
    break
  fi
  sleep 0.5
done

HEALTH_JSON="$(curl -fsS "http://127.0.0.1:${PORT}/v1/health")"
echo "Health response: $HEALTH_JSON"
echo ""

echo "[2/4] Idle container footprint (1 vCPU / 2 GB limit):"
docker stats --no-stream --format "  CPU: {{.CPUPerc}} | Mem: {{.MemUsage}} ({{.MemPerc}}) | PIDs: {{.PIDs}}" "$CONTAINER_NAME"
echo "  Data dir disk usage: $(du -sh "$DATA_DIR" | awk '{print $1}')"
echo ""

echo "[3/4] API burst test (200 sequential authenticated GET /v1/sessions + 50 POST /v1/webhooks):"
START_NS=$(date +%s%N)
for _ in $(seq 1 200); do
  curl -fsS -H "Authorization: Bearer ${API_KEY}" "http://127.0.0.1:${PORT}/v1/sessions" >/dev/null
done
END_NS=$(date +%s%N)
ELAPSED_MS=$(( (END_NS - START_NS) / 1000000 ))
RPS=$(( 200 * 1000 / (ELAPSED_MS > 0 ? ELAPSED_MS : 1) ))
echo "  200x GET /v1/sessions: ${ELAPSED_MS} ms (~${RPS} req/s)"

START_NS=$(date +%s%N)
for i in $(seq 1 50); do
  curl -fsS -X POST \
    -H "Authorization: Bearer ${API_KEY}" \
    -H "Content-Type: application/json" \
    -d "{\"url\":\"http://127.0.0.1:9/hook-${i}\",\"events\":[\"message.received\"]}" \
    "http://127.0.0.1:${PORT}/v1/webhooks" >/dev/null
done
END_NS=$(date +%s%N)
ELAPSED_MS=$(( (END_NS - START_NS) / 1000000 ))
echo "  50x POST /v1/webhooks (SQLite WAL writes): ${ELAPSED_MS} ms"
echo ""

echo "[4/4] Post-burst container footprint & restart recovery:"
docker stats --no-stream --format "  CPU: {{.CPUPerc}} | Mem: {{.MemUsage}} ({{.MemPerc}}) | PIDs: {{.PIDs}}" "$CONTAINER_NAME"
docker restart "$CONTAINER_NAME" >/dev/null
for i in $(seq 1 30); do
  if curl -fsS "http://127.0.0.1:${PORT}/v1/health" >/dev/null 2>&1; then
    break
  fi
  sleep 0.5
done
WEBHOOK_COUNT="$(curl -fsS -H "Authorization: Bearer ${API_KEY}" "http://127.0.0.1:${PORT}/v1/webhooks" | grep -o '"wh_' | wc -l)"
echo "  Webhooks persisted across restart: ${WEBHOOK_COUNT}/50"
echo "=== Benchmark complete ==="
