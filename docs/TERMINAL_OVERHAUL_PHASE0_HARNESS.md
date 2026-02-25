# Terminal Overhaul Phase 0 Harness

## Purpose

Phase 0 requires baseline metrics before architecture changes:

- attach latency
- reconnect stress behavior
- duplicate/blank attach incidence

This harness runs those measurements against the current daemon protocol.

## Scripts

- `scripts/terminal-overhaul-harness.mjs`
- `scripts/terminal-overhaul-benchmark.sh`

## Prerequisites

1. Daemon is running and reachable over WebSocket.
2. Node.js runtime is available (uses built-in WebSocket API).

## Quick Run

```bash
scripts/terminal-overhaul-benchmark.sh
```

Default output:

- `docs/PHASE0_HARNESS_REPORT.json`

## Direct Run

```bash
node scripts/terminal-overhaul-harness.mjs \
  --url ws://127.0.0.1:9847 \
  --scenario all \
  --loops 30 \
  --capture-ms 900 \
  --output docs/PHASE0_HARNESS_REPORT.json
```

Supported scenarios:

- `attach_latency`
- `reconnect_stress`
- `duplicate_detector`
- `all`

Optional flags:

- `--auth-token <token>`
- `--session-id <id>` (reuse an existing session instead of spawning a harness session)

## CI-Friendly Command

```bash
MOBILECLI_HARNESS_URL=ws://127.0.0.1:9847 \
MOBILECLI_HARNESS_LOOPS=30 \
MOBILECLI_HARNESS_CAPTURE_MS=900 \
scripts/terminal-overhaul-benchmark.sh docs/PHASE0_HARNESS_REPORT.json
```

## Report Fields

The JSON report includes:

- per-scenario summary (`p50`, `p95`, avg, min/max)
- per-loop detail for debugging regressions
- duplicate loop counts and blank attach counts

Use this report as input when updating:

- `docs/BASELINE_2026-02-24.md`
