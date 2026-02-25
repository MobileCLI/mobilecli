#!/usr/bin/env bash
# Run all 6 review passes via RunPod Strand-Rust-Coder endpoint
set -euo pipefail

export RUNPOD_API_KEY="${RUNPOD_API_KEY:?Set RUNPOD_API_KEY}"
HARNESS="/home/bigphoot/Desktop/Projects/MobileCLI/scripts/endpoint-harness.sh"
OUT="/home/bigphoot/Desktop/Projects/MobileCLI/tooling/reviews"
CLI="/home/bigphoot/Desktop/Projects/MobileCLI/cli/src"
PROFILE="runpod-strand-prod"

export TEMPERATURE=0.15
export TOP_P=0.9
export REPEAT_PENALTY=1.2

mkdir -p "$OUT"

review_file() {
  local pass="$1"
  local prompt="$2"
  local file="$3"
  local basename
  basename=$(basename "$file" .rs)
  local outfile="$OUT/${pass}-${basename}.md"
  local lines
  lines=$(wc -l < "$file")

  echo "  [$(date +%H:%M:%S)] $basename ($lines lines)..."

  echo "# Review: $basename.rs" > "$outfile"
  echo "Model: Strand-Rust-Coder-14B (RunPod) | $(date -Iseconds)" >> "$outfile"
  echo "" >> "$outfile"

  # For files ≤500 lines, single call. Otherwise chunk.
  local max_chunk=500
  if [ "$lines" -le "$max_chunk" ]; then
    local content
    content=$(<"$file")
    local full_prompt="$prompt

--- FILE: $basename.rs ---
$content
--- END FILE ---

List each finding ONCE. Format: SEVERITY | file:line_or_function | description | fix.
When done, write REVIEW_COMPLETE."

    export MAX_NEW_TOKENS=1536
    echo "## $basename.rs ($lines lines)" >> "$outfile"
    echo "" >> "$outfile"
    echo "$full_prompt" | "$HARNESS" --profile "$PROFILE" >> "$outfile" 2>/dev/null || echo "(request error)" >> "$outfile"
    echo "" >> "$outfile"
  else
    local total_chunks=$(( (lines + max_chunk - 1) / max_chunk ))
    echo "## $basename.rs ($lines lines, $total_chunks chunks)" >> "$outfile"
    echo "" >> "$outfile"

    local chunk=1 start=1
    while [ "$start" -le "$lines" ]; do
      local end=$((start + max_chunk - 1))
      [ "$end" -gt "$lines" ] && end=$lines
      local chunk_text
      chunk_text=$(sed -n "${start},${end}p" "$file")

      local full_prompt="$prompt

--- FILE: $basename.rs lines $start-$end (chunk $chunk/$total_chunks) ---
$chunk_text
--- END ---

List each finding ONCE. Format: SEVERITY | file:line_or_function | description | fix.
When done, write REVIEW_COMPLETE."

      export MAX_NEW_TOKENS=1024
      echo "### Chunk $chunk/$total_chunks (lines $start-$end)" >> "$outfile"
      echo "" >> "$outfile"
      echo "$full_prompt" | "$HARNESS" --profile "$PROFILE" >> "$outfile" 2>/dev/null || echo "(request error)" >> "$outfile"
      echo "" >> "$outfile"

      chunk=$((chunk + 1))
      start=$((end + 1))
    done
  fi

  echo "---" >> "$outfile"
  echo "Complete: $(date -Iseconds)" >> "$outfile"
  echo "  [$(date +%H:%M:%S)] Done → $(basename "$outfile")"
}

echo "[$(date +%H:%M:%S)] Starting 6-pass Rust review via RunPod..."
echo ""

# ── PASS 1: Security ──
echo "[$(date +%H:%M:%S)] ═══ PASS 1/6: Security & Access Control ═══"
SEC_PROMPT="You are a senior Rust security auditor. Review this code for:
1. Path traversal — escaping path jail
2. Auth bypass — unauthenticated access to protected endpoints
3. TOCTOU races in symlink/permission checks
4. Injection via file paths, names, or content
5. Sensitive file exposure (.ssh, .env, credentials, tokens)
6. Write/delete bypassing access controls
Report: SEVERITY | file:line | description | fix. Be concise."

review_file "p1" "$SEC_PROMPT" "$CLI/filesystem/security.rs"
review_file "p1" "$SEC_PROMPT" "$CLI/filesystem/config.rs"
review_file "p1" "$SEC_PROMPT" "$CLI/filesystem/operations.rs"
review_file "p1" "$SEC_PROMPT" "$CLI/daemon.rs"

# ── PASS 2: Daemon Core ──
echo ""
echo "[$(date +%H:%M:%S)] ═══ PASS 2/6: Daemon Core ═══"
DAEMON_PROMPT="You are a senior Rust systems engineer. Review this WebSocket daemon for:
1. Race conditions in shared state (sessions, clients, broadcast)
2. Memory leaks — unbounded vectors, hashmaps, channels
3. Connection lifecycle — cleanup on disconnect/error/timeout
4. Deadlocks — lock ordering, async mutex, channel blocking
5. WebSocket protocol correctness
6. Resource exhaustion from malicious clients
Report: SEVERITY | file:line | description | fix. Be concise."

review_file "p2" "$DAEMON_PROMPT" "$CLI/daemon.rs"

# ── PASS 3: PTY & Input ──
echo ""
echo "[$(date +%H:%M:%S)] ═══ PASS 3/6: PTY & Input Handling ═══"
PTY_PROMPT="You are a senior Rust systems engineer. Review for:
1. Command injection via session names or arguments
2. Input relay correctness — corruption, truncation, encoding
3. Resize handling — SIGWINCH/ioctl correctness
4. Protocol deserialization — malformed JSON causing panics
5. Session attach/detach lifecycle
6. Base64 edge cases
Report: SEVERITY | file:line | description | fix. Be concise."

review_file "p3" "$PTY_PROMPT" "$CLI/pty_wrapper.rs"
review_file "p3" "$PTY_PROMPT" "$CLI/link.rs"
review_file "p3" "$PTY_PROMPT" "$CLI/protocol.rs"

# ── PASS 4: Platform ──
echo ""
echo "[$(date +%H:%M:%S)] ═══ PASS 4/6: Platform & Reliability ═══"
PLAT_PROMPT="You are a senior Rust cross-platform engineer. Review for:
1. Platform edge cases (Windows paths, macOS sandbox, Linux distros)
2. Injection in generated service files (systemd/launchd/scheduler)
3. Shell config corruption from hook injection
4. PID file races, stale process detection
5. ANSI escape stripping correctness
6. Config file corruption and migration
Report: SEVERITY | file:line | description | fix. Be concise."

review_file "p4" "$PLAT_PROMPT" "$CLI/platform.rs"
review_file "p4" "$PLAT_PROMPT" "$CLI/autostart.rs"
review_file "p4" "$PLAT_PROMPT" "$CLI/shell_hook.rs"
review_file "p4" "$PLAT_PROMPT" "$CLI/detection.rs"

# ── PASS 5: Performance ──
echo ""
echo "[$(date +%H:%M:%S)] ═══ PASS 5/6: Performance ═══"
PERF_PROMPT="You are a senior Rust performance engineer. Review for:
1. Unnecessary allocations (String clones, Vec copies, redundant to_string)
2. Blocking I/O on async runtime without spawn_blocking
3. O(n²) patterns — nested loops, quadratic string building
4. Unbounded parallel work (rayon without limits)
5. File watcher event flooding
6. Memory-mapped I/O correctness
7. Rate limiter bypass
Report: SEVERITY | file:line | description | fix. Be concise."

review_file "p5" "$PERF_PROMPT" "$CLI/filesystem/search.rs"
review_file "p5" "$PERF_PROMPT" "$CLI/filesystem/watcher.rs"
review_file "p5" "$PERF_PROMPT" "$CLI/filesystem/rate_limit.rs"
review_file "p5" "$PERF_PROMPT" "$CLI/filesystem/operations.rs"

# ── PASS 6: Error Handling ──
echo ""
echo "[$(date +%H:%M:%S)] ═══ PASS 6/6: Error Handling ═══"
ERR_PROMPT="You are a senior Rust reliability engineer. Review for:
1. Every .unwrap() and .expect() — can it panic in production? List each.
2. Silent error swallowing (ignored Results, empty catch)
3. Graceful degradation when daemon/network unavailable
4. Config file corruption handling
5. Concurrent file access safety
6. Resource cleanup (file handles, sockets, PTY) on all paths
Report: SEVERITY | file:line | description | fix. Be concise."

review_file "p6" "$ERR_PROMPT" "$CLI/main.rs"
review_file "p6" "$ERR_PROMPT" "$CLI/setup.rs"
review_file "p6" "$ERR_PROMPT" "$CLI/session.rs"
review_file "p6" "$ERR_PROMPT" "$CLI/pty_wrapper.rs"
review_file "p6" "$ERR_PROMPT" "$CLI/platform.rs"
review_file "p6" "$ERR_PROMPT" "$CLI/qr.rs"

echo ""
echo "════════════════════════════════════════════"
echo "[$(date +%H:%M:%S)] ALL 6 PASSES COMPLETE"
echo "════════════════════════════════════════════"
echo ""
ls -lhS "$OUT/"*.md 2>/dev/null | awk '{print $5, $9}'
echo ""
echo "Total: $(ls "$OUT/"*.md 2>/dev/null | wc -l) review files"
