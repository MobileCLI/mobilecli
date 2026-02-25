#!/usr/bin/env bash
set -euo pipefail

PROFILES_FILE="${PROFILES_FILE:-tooling/endpoint-harness/profiles.json}"
PROFILE=""
PROMPT=""
PROMPT_FILE=""
LIST_ONLY=0
RAW_JSON=0

TIMEOUT="${TIMEOUT:-900}"
POLL_INTERVAL="${POLL_INTERVAL:-2}"

MAX_NEW_TOKENS="${MAX_NEW_TOKENS:-768}"
TEMPERATURE="${TEMPERATURE:-0.1}"
TOP_P="${TOP_P:-0.9}"
REPEAT_PENALTY="${REPEAT_PENALTY:-1.05}"

usage() {
  cat <<'EOF'
Usage:
  scripts/endpoint-harness.sh --list [--profiles-file path]
  scripts/endpoint-harness.sh --profile NAME --prompt "text"
  scripts/endpoint-harness.sh --profile NAME --prompt-file prompt.txt
  cat prompt.txt | scripts/endpoint-harness.sh --profile NAME

Options:
  --profiles-file PATH   Profiles JSON file (default: tooling/endpoint-harness/profiles.json)
  --profile NAME         Profile key from profiles JSON
  --prompt TEXT          Prompt text
  --prompt-file PATH     Read prompt from file
  --list                 List profile names and types
  --raw                  Print full JSON response (default prints extracted text)
  -h, --help             Show help

Environment knobs:
  TIMEOUT                Request timeout in seconds (default: 900)
  POLL_INTERVAL          Poll interval for async jobs in seconds (default: 2)
  MAX_NEW_TOKENS         Generation cap (default: 768)
  TEMPERATURE            Sampling temperature (default: 0.1)
  TOP_P                  Top-p sampling (default: 0.9)
  REPEAT_PENALTY         Repeat penalty (default: 1.05)
EOF
}

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing required command: $1" >&2
    exit 1
  fi
}

emit_response() {
  local json="$1"
  local jq_expr="$2"

  if [[ "$RAW_JSON" == "1" ]]; then
    echo "$json" | jq .
    return
  fi

  local extracted
  extracted="$(echo "$json" | jq -er "$jq_expr" 2>/dev/null || true)"
  if [[ -n "$extracted" && "$extracted" != "null" ]]; then
    echo "$extracted"
    return
  fi

  echo "$json" | jq .
}

build_headers() {
  local profile_json="$1"
  local -n _headers="$2"

  _headers=(-H "Content-Type: application/json")

  local api_key_env
  api_key_env="$(jq -r '.api_key_env // empty' <<<"$profile_json")"

  if [[ -n "$api_key_env" ]]; then
    local api_key="${!api_key_env:-}"
    if [[ -z "$api_key" ]]; then
      echo "Missing required env var for profile auth: $api_key_env" >&2
      exit 1
    fi

    local header_name auth_scheme
    header_name="$(jq -r '.auth_header_name // "Authorization"' <<<"$profile_json")"
    auth_scheme="$(jq -r '.auth_scheme // "Bearer"' <<<"$profile_json")"

    if [[ -n "$auth_scheme" ]]; then
      _headers+=(-H "${header_name}: ${auth_scheme} ${api_key}")
    else
      _headers+=(-H "${header_name}: ${api_key}")
    fi
  fi
}

wait_for_runpod_terminal() {
  local initial_json="$1"
  local base_url="$2"
  local endpoint_id="$3"
  local -a headers=("${@:4}")

  local status id now deadline current
  current="$initial_json"
  status="$(echo "$current" | jq -r '.status // empty')"
  id="$(echo "$current" | jq -r '.id // empty')"

  case "$status" in
    COMPLETED|FAILED|CANCELLED|TIMED_OUT)
      echo "$current"
      return
      ;;
  esac

  if [[ -z "$id" ]]; then
    echo "$current"
    return
  fi

  deadline=$((SECONDS + TIMEOUT))
  while true; do
    now=$SECONDS
    if (( now >= deadline )); then
      echo "$current"
      return
    fi

    sleep "$POLL_INTERVAL"
    current="$(
      curl -sS \
        --max-time "$TIMEOUT" \
        "${headers[@]}" \
        "${base_url%/}/${endpoint_id}/status/${id}"
    )"
    status="$(echo "$current" | jq -r '.status // empty')"
    case "$status" in
      COMPLETED|FAILED|CANCELLED|TIMED_OUT)
        echo "$current"
        return
        ;;
    esac
  done
}

call_runpod_profile() {
  local profile_json="$1"
  local -a headers=()
  build_headers "$profile_json" headers

  local base_url endpoint_id mode response_jq input_defaults payload url initial final
  base_url="$(jq -r '.base_url // "https://api.runpod.ai/v2"' <<<"$profile_json")"
  endpoint_id="$(jq -r '.endpoint_id // empty' <<<"$profile_json")"
  mode="$(jq -r '.mode // "runsync"' <<<"$profile_json")"
  response_jq="$(jq -r '.response_jq // ".output.text // .output // .status"' <<<"$profile_json")"
  input_defaults="$(jq -c '.input_defaults // {}' <<<"$profile_json")"

  if [[ -z "$endpoint_id" ]]; then
    echo "Profile is missing endpoint_id" >&2
    exit 1
  fi

  if [[ "$mode" != "run" && "$mode" != "runsync" ]]; then
    echo "Invalid runpod mode in profile: $mode (use run or runsync)" >&2
    exit 1
  fi

  payload="$(jq -nc \
    --arg prompt "$PROMPT" \
    --argjson max_new_tokens "$MAX_NEW_TOKENS" \
    --argjson temperature "$TEMPERATURE" \
    --argjson top_p "$TOP_P" \
    --argjson repeat_penalty "$REPEAT_PENALTY" \
    --argjson defaults "$input_defaults" \
    '{input: ($defaults + {prompt: $prompt, max_new_tokens: $max_new_tokens, temperature: $temperature, top_p: $top_p, repeat_penalty: $repeat_penalty})}'
  )"

  url="${base_url%/}/${endpoint_id}/${mode}"
  initial="$(
    curl -sS \
      --max-time "$TIMEOUT" \
      "${headers[@]}" \
      -X POST \
      -d "$payload" \
      "$url"
  )"

  final="$(wait_for_runpod_terminal "$initial" "$base_url" "$endpoint_id" "${headers[@]}")"
  emit_response "$final" "$response_jq"
}

call_openai_profile() {
  local profile_json="$1"
  local -a headers=()
  build_headers "$profile_json" headers

  local url model system_prompt response_jq request_defaults payload
  url="$(jq -r '.url // "https://api.openai.com/v1/chat/completions"' <<<"$profile_json")"
  model="$(jq -r '.model // empty' <<<"$profile_json")"
  system_prompt="$(jq -r '.system_prompt // empty' <<<"$profile_json")"
  response_jq="$(jq -r '.response_jq // ".choices[0].message.content"' <<<"$profile_json")"
  request_defaults="$(jq -c '.request_defaults // {}' <<<"$profile_json")"

  if [[ -z "$model" ]]; then
    echo "OpenAI profile is missing model" >&2
    exit 1
  fi

  if [[ -n "$system_prompt" ]]; then
    payload="$(jq -nc \
      --arg model "$model" \
      --arg system_prompt "$system_prompt" \
      --arg prompt "$PROMPT" \
      --argjson max_tokens "$MAX_NEW_TOKENS" \
      --argjson temperature "$TEMPERATURE" \
      --argjson top_p "$TOP_P" \
      --argjson defaults "$request_defaults" \
      '$defaults + {
        model: $model,
        messages: [
          {role: "system", content: $system_prompt},
          {role: "user", content: $prompt}
        ],
        max_tokens: $max_tokens,
        temperature: $temperature,
        top_p: $top_p
      }'
    )"
  else
    payload="$(jq -nc \
      --arg model "$model" \
      --arg prompt "$PROMPT" \
      --argjson max_tokens "$MAX_NEW_TOKENS" \
      --argjson temperature "$TEMPERATURE" \
      --argjson top_p "$TOP_P" \
      --argjson defaults "$request_defaults" \
      '$defaults + {
        model: $model,
        messages: [{role: "user", content: $prompt}],
        max_tokens: $max_tokens,
        temperature: $temperature,
        top_p: $top_p
      }'
    )"
  fi

  local response
  response="$(
    curl -sS \
      --max-time "$TIMEOUT" \
      "${headers[@]}" \
      -X POST \
      -d "$payload" \
      "$url"
  )"

  emit_response "$response" "$response_jq"
}

call_http_json_profile() {
  local profile_json="$1"
  local -a headers=()
  build_headers "$profile_json" headers

  local url method prompt_field response_jq request_defaults payload response
  url="$(jq -r '.url // empty' <<<"$profile_json")"
  method="$(jq -r '.method // "POST"' <<<"$profile_json")"
  prompt_field="$(jq -r '.prompt_field // "prompt"' <<<"$profile_json")"
  response_jq="$(jq -r '.response_jq // ".text // .output // .result // .message // ."' <<<"$profile_json")"
  request_defaults="$(jq -c '.request_defaults // {}' <<<"$profile_json")"

  if [[ -z "$url" ]]; then
    echo "http_json profile is missing url" >&2
    exit 1
  fi

  payload="$(jq -nc \
    --arg prompt "$PROMPT" \
    --arg prompt_field "$prompt_field" \
    --argjson defaults "$request_defaults" \
    '$defaults + {($prompt_field): $prompt}'
  )"

  response="$(
    curl -sS \
      --max-time "$TIMEOUT" \
      "${headers[@]}" \
      -X "$method" \
      -d "$payload" \
      "$url"
  )"

  emit_response "$response" "$response_jq"
}

main() {
  require_cmd jq
  require_cmd curl

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --profiles-file)
        PROFILES_FILE="$2"
        shift 2
        ;;
      --profile)
        PROFILE="$2"
        shift 2
        ;;
      --prompt)
        PROMPT="$2"
        shift 2
        ;;
      --prompt-file)
        PROMPT_FILE="$2"
        shift 2
        ;;
      --list)
        LIST_ONLY=1
        shift
        ;;
      --raw)
        RAW_JSON=1
        shift
        ;;
      -h|--help)
        usage
        exit 0
        ;;
      *)
        echo "Unknown argument: $1" >&2
        usage
        exit 1
        ;;
    esac
  done

  if [[ ! -f "$PROFILES_FILE" ]]; then
    echo "Profiles file not found: $PROFILES_FILE" >&2
    exit 1
  fi

  if [[ "$LIST_ONLY" == "1" ]]; then
    jq -r '.profiles | to_entries[] | "\(.key)\t\(.value.type)"' "$PROFILES_FILE"
    exit 0
  fi

  if [[ -z "$PROFILE" ]]; then
    echo "--profile is required (or use --list)" >&2
    exit 1
  fi

  if [[ -n "$PROMPT" && -n "$PROMPT_FILE" ]]; then
    echo "Use either --prompt or --prompt-file, not both" >&2
    exit 1
  fi

  if [[ -n "$PROMPT_FILE" ]]; then
    PROMPT="$(cat "$PROMPT_FILE")"
  elif [[ -z "$PROMPT" && ! -t 0 ]]; then
    PROMPT="$(cat)"
  fi

  if [[ -z "$PROMPT" ]]; then
    echo "Prompt is required (use --prompt, --prompt-file, or stdin)" >&2
    exit 1
  fi

  local profile_json profile_type
  profile_json="$(jq -c --arg p "$PROFILE" '.profiles[$p] // empty' "$PROFILES_FILE")"
  if [[ -z "$profile_json" ]]; then
    echo "Unknown profile: $PROFILE" >&2
    exit 1
  fi

  profile_type="$(jq -r '.type // empty' <<<"$profile_json")"
  case "$profile_type" in
    runpod)
      call_runpod_profile "$profile_json"
      ;;
    openai_chat)
      call_openai_profile "$profile_json"
      ;;
    http_json)
      call_http_json_profile "$profile_json"
      ;;
    *)
      echo "Unsupported profile type: $profile_type" >&2
      exit 1
      ;;
  esac
}

main "$@"
