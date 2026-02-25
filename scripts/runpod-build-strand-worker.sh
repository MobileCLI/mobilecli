#!/usr/bin/env bash
set -euo pipefail

# Build/push helper for the RunPod Strand worker image.
#
# Examples:
#   scripts/runpod-build-strand-worker.sh
#   IMAGE=ghcr.io/<org>/mobilecli-strand-worker:latest scripts/runpod-build-strand-worker.sh
#   IMAGE=ghcr.io/<org>/mobilecli-strand-worker:latest PUSH=1 scripts/runpod-build-strand-worker.sh

IMAGE="${IMAGE:-ghcr.io/mobilecli/strand-rust-worker:latest}"
PUSH="${PUSH:-0}"

docker build \
  -f infra/runpod/strand-rust-worker/Dockerfile \
  -t "$IMAGE" \
  infra/runpod/strand-rust-worker

echo "Built image: $IMAGE"

if [[ "$PUSH" == "1" ]]; then
  docker push "$IMAGE"
  echo "Pushed image: $IMAGE"
fi
