#!/usr/bin/env bash
# ollama-review.sh — Feed a single Rust source file to Ollama for code review
# Usage: ./ollama-review.sh <output_file> <prompt> <file>
# For large files (>400 lines), reviews in chunks and appends all results.

set -euo pipefail

OUTPUT="$1"; shift
PROMPT="$1"; shift
FILE="$1"
MODEL="strand-rust-coder:14b-q4"
API="http://localhost:11434/api/generate"
BASENAME=$(basename "$FILE")
MAX_LINES=400

echo "# Review: $BASENAME" > "$OUTPUT"
echo "Model: $MODEL | $(date -Iseconds)" >> "$OUTPUT"
echo "" >> "$OUTPUT"

review_chunk() {
  local chunk_text="$1"
  local chunk_label="$2"
  local full_prompt="$PROMPT

--- $chunk_label ---
$chunk_text
--- END ---

List each finding ONCE. Do NOT repeat findings. Use format: SEVERITY | location | description | fix.
When done, write REVIEW_COMPLETE on its own line and stop."

  local escaped
  escaped=$(python3 -c "import json,sys; print(json.dumps(sys.stdin.read()))" <<< "$full_prompt")

  # Stream response token by token to avoid timeout
  curl -s "$API" \
    -d "{\"model\":\"$MODEL\",\"prompt\":$escaped,\"stream\":true,\"options\":{\"num_ctx\":12288,\"temperature\":0.2,\"top_p\":0.9,\"num_predict\":1024,\"repeat_penalty\":1.3,\"repeat_last_n\":256},\"stop\":[\"REVIEW_COMPLETE\"]}" \
    --max-time 300 \
  | python3 -c "
import json, sys
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    try:
        obj = json.loads(line)
        token = obj.get('response', '')
        sys.stdout.write(token)
        sys.stdout.flush()
        if obj.get('done', False):
            break
    except:
        pass
print()
"
}

TOTAL_LINES=$(wc -l < "$FILE")

if [ "$TOTAL_LINES" -le "$MAX_LINES" ]; then
  echo "## $BASENAME ($TOTAL_LINES lines)" >> "$OUTPUT"
  echo "" >> "$OUTPUT"
  CONTENT=$(<"$FILE")
  review_chunk "$CONTENT" "FILE: $BASENAME" >> "$OUTPUT"
  echo "" >> "$OUTPUT"
else
  TOTAL_CHUNKS=$(( (TOTAL_LINES + MAX_LINES - 1) / MAX_LINES ))
  echo "## $BASENAME ($TOTAL_LINES lines, $TOTAL_CHUNKS chunks)" >> "$OUTPUT"
  echo "" >> "$OUTPUT"

  CHUNK=1
  START=1
  while [ "$START" -le "$TOTAL_LINES" ]; do
    END=$((START + MAX_LINES - 1))
    if [ "$END" -gt "$TOTAL_LINES" ]; then
      END=$TOTAL_LINES
    fi
    CHUNK_TEXT=$(sed -n "${START},${END}p" "$FILE")
    echo "### Chunk $CHUNK/$TOTAL_CHUNKS (lines $START-$END)" >> "$OUTPUT"
    echo "" >> "$OUTPUT"
    review_chunk "$CHUNK_TEXT" "FILE: $BASENAME lines $START-$END (chunk $CHUNK/$TOTAL_CHUNKS)" >> "$OUTPUT"
    echo "" >> "$OUTPUT"
    CHUNK=$((CHUNK + 1))
    START=$((END + 1))
  done
fi

echo "---" >> "$OUTPUT"
echo "Complete: $(date -Iseconds)" >> "$OUTPUT"
