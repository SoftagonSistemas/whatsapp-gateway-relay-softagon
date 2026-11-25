#!/usr/bin/env bash
set -euo pipefail

IMAGE_REPO="${DOCKERHUB_REPO:-softagon/relay}"
VERSION="${1:-$(grep -m1 '^version = ' Cargo.toml | sed 's/version = "\(.*\)"/\1/')}"
MINOR_VERSION="$(echo "$VERSION" | awk -F. '{print $1"."$2}')"
REVISION="$(git rev-parse --short HEAD 2>/dev/null || echo dev)"
CREATED="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
SHORT_DESC="Self-hosted WhatsApp Web API gateway in Rust with SQLite, S3 media, signed webhooks & local Whisper."

echo "=== Building ${IMAGE_REPO} (version=${VERSION}, minor=${MINOR_VERSION}, rev=${REVISION}) ==="

docker build \
  --platform linux/amd64 \
  --build-arg VERSION="$VERSION" \
  --build-arg REVISION="$REVISION" \
  --build-arg CREATED="$CREATED" \
  -t "${IMAGE_REPO}:${VERSION}" \
  -t "${IMAGE_REPO}:${MINOR_VERSION}" \
  -t "${IMAGE_REPO}:latest" \
  .

if [[ "${SKIP_PUSH:-0}" == "1" ]]; then
  echo "SKIP_PUSH=1 set; skipping docker push and Docker Hub README sync."
  exit 0
fi

echo "=== Pushing tags to Docker Hub ==="
docker push "${IMAGE_REPO}:${VERSION}"
docker push "${IMAGE_REPO}:${MINOR_VERSION}"
docker push "${IMAGE_REPO}:latest"

if [[ -n "${DOCKERHUB_USERNAME:-}" && -n "${DOCKERHUB_TOKEN:-}" ]]; then
  echo "=== Updating Docker Hub Overview (README) and description ==="
  JWT="$(curl -fsS -X POST https://hub.docker.com/v2/users/login/ \
    -H "Content-Type: application/json" \
    -d "$(python3 -c 'import json,os; print(json.dumps({"username": os.environ["DOCKERHUB_USERNAME"], "password": os.environ["DOCKERHUB_TOKEN"]}))')" \
    | python3 -c 'import sys,json; print(json.load(sys.stdin)["token"])')"

  PAYLOAD="$(python3 -c 'import json,sys; print(json.dumps({"description": sys.argv[1], "full_description": open(sys.argv[2], encoding="utf-8").read()}))' "$SHORT_DESC" "docs/dockerhub.md")"

  curl -fsS -X PATCH "https://hub.docker.com/v2/repositories/${IMAGE_REPO}/" \
    -H "Authorization: JWT ${JWT}" \
    -H "Content-Type: application/json" \
    -d "$PAYLOAD" >/dev/null

  echo "Docker Hub Overview updated successfully!"
else
  echo "Tip: set DOCKERHUB_USERNAME and DOCKERHUB_TOKEN to sync docs/dockerhub.md to Docker Hub Overview automatically."
fi

echo "=== Release ${IMAGE_REPO}:${VERSION} complete ==="
