#!/usr/bin/env bash
set -euo pipefail

# Usage:
#   scripts/setup-strand-rust.sh
#   scripts/setup-strand-rust.sh strand-rust-coder:14b-q5 Q5_K_M
#
# Creates a local Ollama model alias backed by Strand-Rust-Coder GGUF.

MODEL_ALIAS="${1:-strand-rust-coder:14b-q4}"
QUANT="${2:-Q4_K_M}"

case "$QUANT" in
  Q4_K_M|Q5_K_M|Q6_K|Q8_0) ;;
  *)
    echo "Unsupported quant: $QUANT"
    echo "Supported: Q4_K_M | Q5_K_M | Q6_K | Q8_0"
    exit 1
    ;;
esac

if ! command -v ollama >/dev/null 2>&1; then
  echo "ollama not found in PATH"
  exit 1
fi

tmp_modelfile="$(mktemp)"
trap 'rm -f "$tmp_modelfile"' EXIT

cat >"$tmp_modelfile" <<EOF
FROM hf.co/Fortytwo-Network/Strand-Rust-Coder-14B-v1-GGUF:${QUANT}

PARAMETER temperature 0.1
PARAMETER top_p 0.9
PARAMETER repeat_penalty 1.05
PARAMETER num_ctx 8192
PARAMETER num_predict 2048

SYSTEM """You are Strand-Rust-Coder, specialized in Rust code quality and refactoring.
Preserve behavior by default. Prefer small, compilable patches and explicit tradeoffs."""
EOF

echo "Creating Ollama model: $MODEL_ALIAS (quant: $QUANT)"
ollama create "$MODEL_ALIAS" -f "$tmp_modelfile"
echo "Model ready: $MODEL_ALIAS"
