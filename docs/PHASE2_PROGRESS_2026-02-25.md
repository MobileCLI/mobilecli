# Terminal Overhaul Phase 2 Progress (2026-02-25)

## Scope Completed This Pass

- Removed backend deferred replay queueing (`pending_tui_replay`) and related cleanup paths.
- Simplified `pty_resized` broadcast to ack-only fanout (no replay payload side-channel).
- Removed legacy frame-render heuristics from daemon replay decisions:
  - deleted `frame_render_mode` and related counters/scanners,
  - simplified TUI gating to runtime + `in_alt_screen` only.
- Made tmux replay source canonical:
  - attach replay now relies on `capture-pane` only (no daemon scrollback fallback path),
  - `get_session_history` for tmux no longer falls back to daemon scrollback.
- Removed wrapper jitter/no-op redraw forcing from PTY resize path (`pty_wrapper.rs`).
- Added guarded local echo on mobile terminal input (`TerminalView.tsx`):
  - printable ASCII prediction only,
  - byte-prefix reconciliation against PTY output,
  - auto-disable after repeated mismatches,
  - runtime/CLI gating (`tmux`, alt-screen, non-terminal CLIs disabled).
- Persisted session runtime on mobile (`useSync.ts`) and passed runtime/CLI metadata into `TerminalView`.

## Runtime Validation

### Validation Environment

- Workspace daemon binary: `cli/target/debug/mobilecli`
- Isolated runtime: `HOME=/tmp/mobilecli-debug-home`
- Isolated port: `9855`
- Harness command:

```bash
node scripts/terminal-overhaul-harness.mjs \
  --url ws://127.0.0.1:9855 \
  --scenario all \
  --loops 10 \
  --capture-ms 500 \
  --output docs/PHASE2_PROGRESS_REPORT_2026-02-25.json
```

### Key Results

- Protocol negotiation: **v2 confirmed on every loop**.
- Attach handshake latency (`attach_ready_latency` / handshake):
  - min: 40.6 ms
  - avg: 41.7 ms
  - p95: 42.8 ms
  - max: 42.8 ms
- First stable frame latency:
  - min: 40.6 ms
  - avg: 41.7 ms
  - p95: 42.8 ms
  - max: 42.8 ms
- Blank attaches: **0**
- Reconnect duplicate incidence:
  - reconnect stress duplicate loop rate: **0.3** (3/10)
  - duplicate detector duplicate loop rate: **0.2** (2/10)
  - total duplicate tokens: **5** across 20 reconnect-oriented loops

## Observed Remaining Issue

- Low-level duplicate incidence still exists in a minority of reconnect loops.
- Remaining issue is now concentrated in live sequence boundary behavior, not deferred replay side-channels.

## Notes

- Port `9847` is controlled by a managed daemon process (`/home/bigphoot/.local/bin/mobilecli`).
- Reliable workspace-binary validation requires isolated HOME+port runtime for now.

## Immediate Next Tasks

1. Add dedicated attach/reconnect stress test(s) around `attach_ready.last_live_seq` barriers and first live chunk ordering.
2. Validate local-echo behavior on device (terminal-only sessions first) and tune mismatch auto-disable thresholds.
3. Continue Phase 6 cleanup by removing now-obsolete rollout/docs references to deleted frame and jitter paths.
