#!/usr/bin/env bash
set -euo pipefail

# Usage examples:
#   scripts/strand-rust-review.sh --file cli/src/autostart.rs
#   scripts/strand-rust-review.sh --diff
#   scripts/strand-rust-review.sh --file cli/src/daemon.rs --model strand-rust-coder:14b-q4
#   scripts/strand-rust-review.sh --backend runpod --file cli/src/daemon.rs
#
# Optional env:
#   MODEL=strand-rust-coder:14b-q4
#   PROMPT_FILE=prompts/strand_rust_review_prompt.md
#   STRAND_BACKEND=ollama|runpod
#   RUNPOD_API_KEY=...
#   RUNPOD_ENDPOINT_ID=...
#   RUNPOD_BASE_URL=https://api.runpod.ai/v2

MODEL="${MODEL:-strand-rust-coder:14b-q4}"
PROMPT_FILE="${PROMPT_FILE:-prompts/strand_rust_review_prompt.md}"
NUM_PREDICT="${NUM_PREDICT:-768}"
REQUEST_TIMEOUT="${REQUEST_TIMEOUT:-900}"
BACKEND="${STRAND_BACKEND:-ollama}"
RUNPOD_BASE_URL="${RUNPOD_BASE_URL:-https://api.runpod.ai/v2}"

mode=""
target=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --file)
      mode="file"
      target="${2:-}"
      shift 2
      ;;
    --diff)
      mode="diff"
      shift
      ;;
    --model)
      MODEL="${2:-}"
      shift 2
      ;;
    --backend)
      BACKEND="${2:-}"
      shift 2
      ;;
    *)
      echo "Unknown arg: $1"
      exit 1
      ;;
  esac
done

if [[ ! -f "$PROMPT_FILE" ]]; then
  echo "Prompt file not found: $PROMPT_FILE"
  exit 1
fi

payload_file="$(mktemp)"
request_file="$(mktemp)"
trap 'rm -f "$payload_file" "$request_file"' EXIT

{
  cat "$PROMPT_FILE"
  echo
  echo "----- BEGIN CONTEXT -----"
  if [[ "$mode" == "file" ]]; then
    if [[ -z "$target" || ! -f "$target" ]]; then
      echo "Invalid --file target: $target"
      exit 1
    fi
    echo "Context type: file"
    echo "Path: $target"
    echo
    cat "$target"
  elif [[ "$mode" == "diff" ]]; then
    echo "Context type: git diff"
    echo
    git diff
  else
    echo "No input mode selected. Use --file <path> or --diff."
    exit 1
  fi
  echo
  echo "----- END CONTEXT -----"
} >"$payload_file"

if ! command -v curl >/dev/null 2>&1; then
  echo "curl is required but not found"
  exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required but not found"
  exit 1
fi

echo "Running model: $MODEL (num_predict=$NUM_PREDICT)"

if [[ "$BACKEND" == "ollama" ]]; then
  jq -n \
    --arg model "$MODEL" \
    --rawfile prompt "$payload_file" \
    --argjson num_predict "$NUM_PREDICT" \
    '{model: $model, prompt: $prompt, stream: false, options: {num_predict: $num_predict}}' \
    > "$request_file"

  response_json="$(
    curl -sS \
      --max-time "$REQUEST_TIMEOUT" \
      -H 'Content-Type: application/json' \
      --data-binary "@$request_file" \
      http://127.0.0.1:11434/api/generate
  )"

  if [[ "$(echo "$response_json" | jq -r '.error // empty')" != "" ]]; then
    echo "Ollama API error: $(echo "$response_json" | jq -r '.error')"
    exit 1
  fi

  echo "$response_json" | jq -r '.response // ""'
  exit 0
fi

if [[ "$BACKEND" == "runpod" ]]; then
  if [[ -z "${RUNPOD_API_KEY:-}" ]]; then
    echo "RUNPOD_API_KEY is required for backend=runpod"
    exit 1
  fi
  if [[ -z "${RUNPOD_ENDPOINT_ID:-}" ]]; then
    echo "RUNPOD_ENDPOINT_ID is required for backend=runpod"
    exit 1
  fi

  jq -n \
    --rawfile prompt "$payload_file" \
    --argjson num_predict "$NUM_PREDICT" \
    '{input: {prompt: $prompt, max_new_tokens: $num_predict}}' \
    > "$request_file"

  endpoint_url="${RUNPOD_BASE_URL%/}/${RUNPOD_ENDPOINT_ID}/runsync"
  response_json="$(
    curl -sS \
      --max-time "$REQUEST_TIMEOUT" \
      -H 'Content-Type: application/json' \
      -H "Authorization: Bearer ${RUNPOD_API_KEY}" \
      --data-binary "@$request_file" \
      "$endpoint_url"
  )"

  runpod_error="$(echo "$response_json" | jq -r '.error // empty')"
  if [[ -n "$runpod_error" ]]; then
    echo "RunPod API error: $runpod_error"
    exit 1
  fi

  # Worker returns {"text":"..."} in output. Fallback handles string/other structures.
  output_text="$(echo "$response_json" | jq -r '.output.text // .output // .response // empty')"
  if [[ -z "$output_text" ]]; then
    echo "RunPod response did not include output text:"
    echo "$response_json" | jq .
    exit 1
  fi

  echo "$output_text"
  exit 0
fi

echo "Unsupported backend: $BACKEND"
echo "Supported backends: ollama, runpod"
exit 1
