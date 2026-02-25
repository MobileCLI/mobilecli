# Terminal Overhaul Execution Bootstrap

Date: 2026-02-24
Purpose: Fast resume point after conversation compaction
Owner: Codex execution agent

## Canonical Plan

Primary source of truth:
- `docs/TERMINAL_OVERHAUL_FINAL_MASTER_PLAN_2026-02-24.md`

Superseded reference docs (keep for traceability only):
- `docs/TERMINAL_OVERHAUL_EXECUTION_MASTER_PLAN_2026-02-24.md`
- `docs/TERMINAL_OVERHAUL_REVIEW_AND_ADDENDUM.md`
- `docs/TERMINAL_OVERHAUL_TASK_BOARD.md`

## Execution Defaults (Locked)

1. WS-A and WS-B are merged into one protocol/replay rewrite stream.
2. No ack/resend retransmit in first protocol-v2 cut.
3. Phase 0 includes observability and benchmark harness setup.
4. Resize simplification happens after attach/replay path stabilizes.
5. Local echo is guarded and can be disabled by flag at runtime.

## Immediate Phase-0 Start Checklist

1. Add feature-flag plumbing:
- `MOBILECLI_ATTACH_PROTOCOL=v1|v2`
- `MOBILECLI_ENABLE_LOCAL_ECHO=0|1`
- `MOBILECLI_RESIZE_SIMPLIFIED=0|1`
- `MOBILECLI_TMUX_SNAPSHOT_ONLY=0|1`

2. Add structured logs around:
- attach start/end (`session_id`, `attach_id`, runtime, source, duration)
- sequence handling (`seq`, duplicates dropped, stale attach ignored)
- resize decisions (`reason`, `epoch`, forwarded/ignored)

3. Build benchmark and stress harness stubs:
- attach latency scenario
- reconnect loop scenario (30x)
- duplicate frame detector
- implementation: `scripts/terminal-overhaul-harness.mjs`
- wrapper: `scripts/terminal-overhaul-benchmark.sh`

4. Create baseline report:
- `docs/BASELINE_2026-02-24.md`
- capture current attach latency, reconnect time, duplicate incidence, keyboard resize count
- raw benchmark JSON: `docs/PHASE0_HARNESS_REPORT.json`

## Phase Gates (Short Form)

Phase 0 exit:
- Baseline report committed
- Flags active
- Harness runnable

Phase 1-2 exit:
- v2 attach path works end-to-end
- 30 reconnect loops with zero duplicate/blank incidents

Phase 3 exit:
- Native scroll rewrite complete, no custom touch pan

Phase 4 exit:
- Keyboard-only PTY resize count is zero

Phase 5 exit:
- Local echo meets latency and correction-rate thresholds

Phase 6 exit:
- Obsolete paths removed
- Docs updated
- Full matrix pass

## First Implementation Files to Touch

Backend:
- `cli/src/protocol.rs`
- `cli/src/daemon.rs`

Mobile:
- `mobile/hooks/useSync.ts`

## Notes for Resume

- Start with Phase 0 only; do not begin protocol rewrite until baseline metrics are captured.
- Preserve existing behavior behind flags while introducing v2 scaffolding.
- Use this file + final master plan as the startup context if chat history is compacted.

## Progress Snapshot (2026-02-25)

Completed:
- Phase 0 baseline/harness/docs delivered.
- Backend feature flags + structured observability added.
- Attach v2 scaffolding landed:
  - protocol v2 message variants,
  - daemon attach_id allocation and per-client attach context,
  - live `pty_chunk` sequence wiring.
- Attach protocol default switched to `v2` (with capability-gated fallback to v1).
- Phase 2 replay cleanup started:
  - removed backend `pending_tui_replay` path,
  - removed resize-triggered deferred replay fanout (`pty_resized` now ack-only).
- Phase 2 replay canonicalization advanced:
  - removed daemon frame-render heuristics and counters,
  - tmux replay now capture-pane canonical (attach/history fallback to daemon scrollback removed).
- Phase 4 resize simplification advanced:
  - removed wrapper jitter/no-op redraw forcing from `pty_wrapper.rs`.
- Phase 5 local echo started:
  - mobile terminal now supports guarded printable local echo prediction + reconciliation + auto-disable.
- Mobile `useSync` now parses attach v2 messages and applies attach/seq stale-drop logic.
- Mobile/WebView scroll cleanup started:
  - removed `xterm.html` custom touch pan layer and manual `preventDefault` drag loop,
  - retained native viewport scroll listener and tap-to-focus path.
- Attach v2 runtime validation completed against workspace daemon on isolated runtime (`HOME=/tmp/mobilecli-debug-home`, port `9855`):
  - harness report: `docs/PHASE2_PROGRESS_REPORT_2026-02-25.json`
  - summary: `docs/PHASE2_PROGRESS_2026-02-25.md`
- Harness replay metrics updated:
  - duplicate detector boundary bug fixed (tail overlap no longer double-counted),
  - handshake metrics promoted to v2-first (`handshake_latency` / `attach_ready_latency`).

Remaining focus:
- harden attach-v2 first-live sequence boundary behavior until reconnect duplicate rate reaches zero in 30-loop runs.
- device-validate local echo (terminal-only path first), then tune/ship defaults.
- finish Phase 6 cleanup by deleting obsolete rollout notes/tests tied to removed frame/jitter paths.
