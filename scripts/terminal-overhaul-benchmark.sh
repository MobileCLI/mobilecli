#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

URL="${MOBILECLI_HARNESS_URL:-ws://127.0.0.1:9847}"
LOOPS="${MOBILECLI_HARNESS_LOOPS:-30}"
CAPTURE_MS="${MOBILECLI_HARNESS_CAPTURE_MS:-900}"
OUTPUT_PATH="${1:-docs/PHASE0_HARNESS_REPORT.json}"

node scripts/terminal-overhaul-harness.mjs \
  --url "$URL" \
  --scenario all \
  --loops "$LOOPS" \
  --capture-ms "$CAPTURE_MS" \
  --output "$OUTPUT_PATH"

echo "Phase 0 harness report written to $OUTPUT_PATH"
