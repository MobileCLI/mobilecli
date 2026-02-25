#!/usr/bin/env bash
set -euo pipefail

# Minimal smoke test for a deployed RunPod endpoint using the Strand worker contract.
#
# Required env:
#   RUNPOD_API_KEY
#   RUNPOD_ENDPOINT_ID
#
# Optional env:
#   RUNPOD_BASE_URL=https://api.runpod.ai/v2
#   MAX_NEW_TOKENS=64
#   REQUEST_TIMEOUT=600

RUNPOD_BASE_URL="${RUNPOD_BASE_URL:-https://api.runpod.ai/v2}"
MAX_NEW_TOKENS="${MAX_NEW_TOKENS:-64}"
REQUEST_TIMEOUT="${REQUEST_TIMEOUT:-600}"

if [[ -z "${RUNPOD_API_KEY:-}" ]]; then
  echo "RUNPOD_API_KEY is required"
  exit 1
fi

if [[ -z "${RUNPOD_ENDPOINT_ID:-}" ]]; then
  echo "RUNPOD_ENDPOINT_ID is required"
  exit 1
fi

payload="$(jq -n \
  --arg prompt "Reply with exactly READY" \
  --argjson max_new_tokens "$MAX_NEW_TOKENS" \
  '{input: {prompt: $prompt, max_new_tokens: $max_new_tokens}}'
)"

url="${RUNPOD_BASE_URL%/}/${RUNPOD_ENDPOINT_ID}/runsync"
response="$(
  curl -sS \
    --max-time "$REQUEST_TIMEOUT" \
    -H "Authorization: Bearer ${RUNPOD_API_KEY}" \
    -H "Content-Type: application/json" \
    -d "$payload" \
    "$url"
)"

echo "$response" | jq .
