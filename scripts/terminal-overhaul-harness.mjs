#!/usr/bin/env node

import { performance } from 'node:perf_hooks';
import { setTimeout as sleep } from 'node:timers/promises';
import { writeFile } from 'node:fs/promises';

const DEFAULT_URL = 'ws://127.0.0.1:9847';
const DEFAULT_LOOPS = 30;
const DEFAULT_CAPTURE_MS = 900;
const DEFAULT_CONNECT_TIMEOUT_MS = 5000;
const DEFAULT_WAIT_TIMEOUT_MS = 4000;
const CLIENT_CAP_ATTACH_V2 = 1 << 0;
const CLEAR_MAIN_BASE64 = 'G1syShtbM0obW0g=';
const STRESS_COMMAND =
  `perl -e '$|=1; for($i=1;;$i++){printf("stress-%08d\\n",$i); select(undef,undef,undef,0.03);}'`;

function usage() {
  console.log(`Terminal Overhaul Harness

Usage:
  node scripts/terminal-overhaul-harness.mjs --scenario attach_latency
  node scripts/terminal-overhaul-harness.mjs --scenario reconnect_stress --loops 30
  node scripts/terminal-overhaul-harness.mjs --scenario duplicate_detector --loops 30
  node scripts/terminal-overhaul-harness.mjs --scenario all --output docs/phase0-harness.json

Options:
  --url <ws_url>            WebSocket URL (default: ${DEFAULT_URL})
  --auth-token <token>      Optional auth token for hello
  --scenario <name>         attach_latency | reconnect_stress | duplicate_detector | all
  --loops <n>               Loop count (default: ${DEFAULT_LOOPS})
  --capture-ms <n>          Capture window per attach in milliseconds (default: ${DEFAULT_CAPTURE_MS})
  --session-id <id>         Reuse existing session instead of spawning a harness session
  --output <path>           Optional JSON report output path
  --help                    Show this help
`);
}

function parseArgs(argv) {
  const args = {
    url: DEFAULT_URL,
    authToken: undefined,
    scenario: 'all',
    loops: DEFAULT_LOOPS,
    captureMs: DEFAULT_CAPTURE_MS,
    sessionId: undefined,
    output: undefined,
  };

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--help' || arg === '-h') {
      args.help = true;
      continue;
    }
    const value = argv[i + 1];
    if (!value || value.startsWith('--')) {
      throw new Error(`Missing value for ${arg}`);
    }
    if (arg === '--url') {
      args.url = value;
      i += 1;
      continue;
    }
    if (arg === '--auth-token') {
      args.authToken = value;
      i += 1;
      continue;
    }
    if (arg === '--scenario') {
      args.scenario = value;
      i += 1;
      continue;
    }
    if (arg === '--loops') {
      args.loops = Math.max(1, Number.parseInt(value, 10) || DEFAULT_LOOPS);
      i += 1;
      continue;
    }
    if (arg === '--capture-ms') {
      args.captureMs = Math.max(200, Number.parseInt(value, 10) || DEFAULT_CAPTURE_MS);
      i += 1;
      continue;
    }
    if (arg === '--session-id') {
      args.sessionId = value;
      i += 1;
      continue;
    }
    if (arg === '--output') {
      args.output = value;
      i += 1;
      continue;
    }
    throw new Error(`Unknown argument: ${arg}`);
  }

  const allowed = new Set(['attach_latency', 'reconnect_stress', 'duplicate_detector', 'all']);
  if (!allowed.has(args.scenario)) {
    throw new Error(
      `Invalid --scenario '${args.scenario}'. Allowed: attach_latency, reconnect_stress, duplicate_detector, all`
    );
  }
  return args;
}

function decodeUtf8Base64(data) {
  if (typeof data !== 'string' || data.length === 0) return '';
  try {
    return Buffer.from(data, 'base64').toString('utf8');
  } catch {
    return '';
  }
}

function stripAnsi(input) {
  return input.replace(/\u001b\[[0-?]*[ -/]*[@-~]/g, '');
}

function quantile(values, q) {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const idx = Math.min(sorted.length - 1, Math.max(0, Math.ceil(sorted.length * q) - 1));
  return Number(sorted[idx].toFixed(1));
}

function summarize(values) {
  if (!values.length) return null;
  const total = values.reduce((sum, value) => sum + value, 0);
  return {
    count: values.length,
    min_ms: Number(Math.min(...values).toFixed(1)),
    avg_ms: Number((total / values.length).toFixed(1)),
    p50_ms: quantile(values, 0.5),
    p95_ms: quantile(values, 0.95),
    max_ms: Number(Math.max(...values).toFixed(1)),
  };
}

class HarnessClient {
  constructor(url, authToken) {
    this.url = url;
    this.authToken = authToken;
    this.socket = null;
    this.queue = [];
    this.waiters = [];
    this.lastSeenSeqBySession = {};
  }

  async connect(timeoutMs = DEFAULT_CONNECT_TIMEOUT_MS) {
    if (this.socket && this.socket.readyState === WebSocket.OPEN) return;
    await new Promise((resolve, reject) => {
      const ws = new WebSocket(this.url);
      this.socket = ws;
      const timeout = setTimeout(() => {
        reject(new Error(`WebSocket connect timeout after ${timeoutMs}ms`));
      }, timeoutMs);

      ws.onopen = () => {
        clearTimeout(timeout);
        resolve();
      };
      ws.onerror = () => {
        clearTimeout(timeout);
        reject(new Error('WebSocket connection failed'));
      };
      ws.onclose = () => {
        if (this.socket === ws) this.socket = null;
      };
      ws.onmessage = async (event) => {
        try {
          const text =
            typeof event.data === 'string'
              ? event.data
              : Buffer.from(await event.data.arrayBuffer()).toString('utf8');
          const msg = JSON.parse(text);
          this.#ingest(msg);
        } catch {
          // Ignore non-JSON payloads.
        }
      };
    });

    this.send({
      type: 'hello',
      auth_token: this.authToken,
      client_version: 'phase0-harness',
      sender_id: `phase0-harness-${Date.now().toString(36)}`,
      client_capabilities: CLIENT_CAP_ATTACH_V2,
    });
    this.send({ type: 'get_sessions' });
    await this.waitFor((msg) => msg.type === 'welcome', DEFAULT_WAIT_TIMEOUT_MS, 'welcome');
  }

  #ingest(msg) {
    const wrapped = {
      msg,
      receivedAtMs: performance.now(),
    };

    for (let i = 0; i < this.waiters.length; i += 1) {
      const waiter = this.waiters[i];
      let matches = false;
      try {
        matches = waiter.predicate(msg);
      } catch {
        matches = false;
      }
      if (!matches) continue;
      clearTimeout(waiter.timeout);
      this.waiters.splice(i, 1);
      waiter.resolve(wrapped);
      return;
    }

    this.queue.push(wrapped);
    if (this.queue.length > 4000) {
      this.queue.splice(0, this.queue.length - 4000);
    }
  }

  send(payload) {
    if (!this.socket || this.socket.readyState !== WebSocket.OPEN) {
      throw new Error('WebSocket is not open');
    }
    this.socket.send(JSON.stringify(payload));
  }

  clearQueue() {
    this.queue = [];
  }

  async waitFor(predicate, timeoutMs = DEFAULT_WAIT_TIMEOUT_MS, label = 'message') {
    for (let i = 0; i < this.queue.length; i += 1) {
      const wrapped = this.queue[i];
      let matches = false;
      try {
        matches = predicate(wrapped.msg);
      } catch {
        matches = false;
      }
      if (matches) {
        this.queue.splice(i, 1);
        return wrapped;
      }
    }

    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        const idx = this.waiters.indexOf(waiter);
        if (idx >= 0) this.waiters.splice(idx, 1);
        reject(new Error(`Timed out waiting for ${label} (${timeoutMs}ms)`));
      }, timeoutMs);

      const waiter = {
        predicate,
        resolve,
        timeout,
      };
      this.waiters.push(waiter);
    });
  }

  async nextMessage(timeoutMs = DEFAULT_WAIT_TIMEOUT_MS) {
    const wrapped = await this.waitFor(() => true, timeoutMs, 'next message');
    return wrapped;
  }

  async close() {
    if (!this.socket) return;
    try {
      this.socket.close();
    } catch {
      // Ignore close failures.
    }
    this.socket = null;
    this.queue = [];
    this.waiters = [];
  }
}

async function ensureHarnessSession(client, requestedSessionId) {
  if (requestedSessionId) {
    return { sessionId: requestedSessionId, spawned: false, sessionName: null };
  }

  const sessionName = `phase0-harness-${Date.now()}`;
  client.send({
    type: 'spawn_session',
    command: 'bash',
    args: ['-lc', STRESS_COMMAND],
    name: sessionName,
  });
  const spawnResult = await client.waitFor(
    (msg) => msg.type === 'spawn_result',
    10000,
    'spawn_result'
  );
  if (!spawnResult.msg.success) {
    throw new Error(`spawn_session failed: ${spawnResult.msg.error || 'unknown error'}`);
  }

  for (let attempt = 0; attempt < 40; attempt += 1) {
    client.send({ type: 'get_sessions' });
    const sessionsMsg = await client.waitFor((msg) => msg.type === 'sessions', 3000, 'sessions');
    const sessions = Array.isArray(sessionsMsg.msg.sessions) ? sessionsMsg.msg.sessions : [];
    const found = sessions.find((session) => (session.name || '').trim() === sessionName);
    if (found) {
      const sessionId = found.id || found.session_id;
      if (sessionId) {
        return { sessionId, spawned: true, sessionName };
      }
    }
    await sleep(150);
  }

  throw new Error(`Could not locate spawned session '${sessionName}' in sessions list`);
}

async function closeHarnessSession(client, sessionId) {
  try {
    client.send({ type: 'close_session', session_id: sessionId });
    await client.waitFor(
      (msg) =>
        (msg.type === 'session_closed' || msg.type === 'session_ended') &&
        msg.session_id === sessionId,
      3000,
      'session_closed/session_ended'
    );
  } catch {
    // Session may already be closed; ignore.
  }
}

function collectStressTokens(text, state) {
  const priorTail = state.tail;
  const combined = priorTail + text;
  const boundaryIndex = priorTail.length;
  const matches = combined.matchAll(/stress-(\d{4,12})/g);
  for (const match of matches) {
    const matchStart = Number(match.index || 0);
    const matchEnd = matchStart + match[0].length;
    // Ignore tokens fully contained in prior tail (already counted).
    if (matchEnd <= boundaryIndex) {
      continue;
    }
    const token = match[1];
    if (state.seen.has(token)) {
      state.duplicates += 1;
    } else {
      state.seen.add(token);
    }
  }
  state.tail = combined.slice(-32);
}

async function captureAttachWindow(client, sessionId, captureMs) {
  client.clearQueue();
  const subscribeStart = performance.now();
  client.send({
    type: 'subscribe',
    session_id: sessionId,
    client_capabilities: CLIENT_CAP_ATTACH_V2,
    last_seen_seq:
      typeof client.lastSeenSeqBySession[sessionId] === 'number'
        ? client.lastSeenSeqBySession[sessionId]
        : undefined,
  });

  let protocol = 'v1';
  let attachId = null;
  let lastLiveSeq = client.lastSeenSeqBySession[sessionId] || 0;
  let handshakeMs = null;
  let handshakeEvent = null;
  const preReady = [];
  const handshakeDeadline = subscribeStart + 5000;
  while (performance.now() < handshakeDeadline) {
    const remaining = Math.ceil(handshakeDeadline - performance.now());
    if (remaining <= 0) break;
    const wrapped = await client.nextMessage(remaining);
    const msg = wrapped.msg;
    if (msg.session_id !== sessionId) continue;

    if (msg.type === 'attach_begin') {
      protocol = 'v2';
      const incomingAttach = Number(msg.attach_id);
      if (Number.isFinite(incomingAttach)) attachId = incomingAttach;
      continue;
    }
    if (msg.type === 'attach_ready') {
      protocol = 'v2';
      const incomingAttach = Number(msg.attach_id);
      if (Number.isFinite(incomingAttach)) attachId = incomingAttach;
      const incomingSeq = Number(msg.last_live_seq);
      if (Number.isFinite(incomingSeq) && incomingSeq >= 0) {
        lastLiveSeq = incomingSeq;
        client.lastSeenSeqBySession[sessionId] = incomingSeq;
      }
      handshakeMs = wrapped.receivedAtMs - subscribeStart;
      handshakeEvent = 'attach_ready';
      break;
    }
    if (msg.type === 'subscribe_ack') {
      handshakeMs = wrapped.receivedAtMs - subscribeStart;
      handshakeEvent = 'subscribe_ack';
      break;
    }
    preReady.push(wrapped);
  }

  if (handshakeMs === null) {
    throw new Error(`Timed out waiting for subscribe_ack/attach_ready (${sessionId})`);
  }

  const tokenState = {
    seen: new Set(),
    duplicates: 0,
    tail: '',
  };
  let stableFrameMs = null;
  let frameMessages = 0;
  let payloadBytes = 0;

  const consumePayload = (wrapped) => {
    const msg = wrapped.msg;
    if (msg.session_id !== sessionId) return;

    let payload = null;
    if (msg.type === 'pty_bytes' || msg.type === 'session_history') {
      payload = msg.data;
    } else if (msg.type === 'attach_snapshot_chunk') {
      if (attachId !== null && Number(msg.attach_id) !== attachId) return;
      payload = msg.data;
    } else if (msg.type === 'pty_chunk') {
      if (attachId !== null && Number(msg.attach_id) !== attachId) return;
      const seq = Number(msg.seq);
      if (Number.isFinite(seq) && seq <= lastLiveSeq) return;
      if (Number.isFinite(seq) && seq > 0) {
        lastLiveSeq = seq;
        client.lastSeenSeqBySession[sessionId] = seq;
      }
      payload = msg.data;
    } else if (msg.type === 'attach_clear') {
      payload = CLEAR_MAIN_BASE64;
    }

    if (typeof payload !== 'string') return;
    const decoded = decodeUtf8Base64(payload);
    if (!decoded) return;

    frameMessages += 1;
    payloadBytes += Buffer.byteLength(decoded);
    const stripped = stripAnsi(decoded);
    if (stableFrameMs === null && stripped.trim().length > 0) {
      stableFrameMs = wrapped.receivedAtMs - subscribeStart;
    }
    collectStressTokens(stripped, tokenState);
  };

  preReady.forEach(consumePayload);

  const deadline = performance.now() + captureMs;

  while (performance.now() < deadline) {
    const remaining = Math.ceil(deadline - performance.now());
    if (remaining <= 0) break;
    let wrapped;
    try {
      wrapped = await client.nextMessage(remaining);
    } catch {
      break;
    }
    consumePayload(wrapped);
  }

  client.send({ type: 'unsubscribe', session_id: sessionId });
  await sleep(60);

  return {
    protocol,
    handshake_event: handshakeEvent,
    handshake_ms: Number(handshakeMs.toFixed(1)),
    subscribe_ack_ms: Number(handshakeMs.toFixed(1)),
    first_stable_frame_ms:
      stableFrameMs === null ? null : Number(stableFrameMs.toFixed(1)),
    frame_messages: frameMessages,
    payload_bytes: payloadBytes,
    duplicate_tokens: tokenState.duplicates,
    unique_tokens: tokenState.seen.size,
    blank_attach: stableFrameMs === null,
  };
}

async function runAttachLatency(client, sessionId, loops, captureMs) {
  const perLoop = [];
  for (let i = 0; i < loops; i += 1) {
    const result = await captureAttachWindow(client, sessionId, captureMs);
    perLoop.push(result);
    await sleep(80);
  }

  const handshakeLatencies = perLoop.map((loop) => loop.handshake_ms);
  const attachReadyLatencies = perLoop
    .filter((loop) => loop.handshake_event === 'attach_ready')
    .map((loop) => loop.handshake_ms);
  const subscribeAckLatencies = perLoop
    .filter((loop) => loop.handshake_event === 'subscribe_ack')
    .map((loop) => loop.handshake_ms);
  const stableFrameLatencies = perLoop
    .map((loop) => loop.first_stable_frame_ms)
    .filter((value) => value !== null);

  return {
    loops,
    capture_ms: captureMs,
    handshake_latency: summarize(handshakeLatencies),
    attach_ready_latency: summarize(attachReadyLatencies),
    subscribe_ack_latency: summarize(subscribeAckLatencies),
    first_stable_frame_latency: summarize(stableFrameLatencies),
    blank_attach_count: perLoop.filter((loop) => loop.blank_attach).length,
    per_loop: perLoop,
  };
}

async function runReconnectStress(client, sessionId, loops, captureMs) {
  const perLoop = [];
  for (let i = 0; i < loops; i += 1) {
    const result = await captureAttachWindow(client, sessionId, captureMs);
    perLoop.push(result);
    await sleep(90);
  }

  const duplicateLoopCount = perLoop.filter((loop) => loop.duplicate_tokens > 0).length;
  const totalDuplicateTokens = perLoop.reduce(
    (sum, loop) => sum + loop.duplicate_tokens,
    0
  );
  const blankAttachCount = perLoop.filter((loop) => loop.blank_attach).length;

  return {
    loops,
    capture_ms: captureMs,
    duplicate_loop_count: duplicateLoopCount,
    duplicate_loop_rate: Number((duplicateLoopCount / loops).toFixed(4)),
    total_duplicate_tokens: totalDuplicateTokens,
    blank_attach_count: blankAttachCount,
    blank_attach_rate: Number((blankAttachCount / loops).toFixed(4)),
    handshake_latency: summarize(perLoop.map((loop) => loop.handshake_ms)),
    attach_ready_latency: summarize(
      perLoop
        .filter((loop) => loop.handshake_event === 'attach_ready')
        .map((loop) => loop.handshake_ms)
    ),
    subscribe_ack_latency: summarize(
      perLoop
        .filter((loop) => loop.handshake_event === 'subscribe_ack')
        .map((loop) => loop.handshake_ms)
    ),
    first_stable_frame_latency: summarize(
      perLoop
        .map((loop) => loop.first_stable_frame_ms)
        .filter((value) => value !== null)
    ),
    per_loop: perLoop,
  };
}

async function runDuplicateDetector(client, sessionId, loops, captureMs) {
  const stress = await runReconnectStress(client, sessionId, loops, captureMs);
  return {
    loops: stress.loops,
    duplicate_loop_count: stress.duplicate_loop_count,
    duplicate_loop_rate: stress.duplicate_loop_rate,
    total_duplicate_tokens: stress.total_duplicate_tokens,
    blank_attach_count: stress.blank_attach_count,
    per_loop: stress.per_loop,
  };
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.help) {
    usage();
    return;
  }

  const client = new HarnessClient(args.url, args.authToken);
  let sessionMeta = null;
  let runError = null;
  const report = {
    generated_at: new Date().toISOString(),
    url: args.url,
    scenario: args.scenario,
    loops: args.loops,
    capture_ms: args.captureMs,
    session: null,
    results: {},
  };

  try {
    await client.connect();
    sessionMeta = await ensureHarnessSession(client, args.sessionId);
    report.session = {
      session_id: sessionMeta.sessionId,
      spawned: sessionMeta.spawned,
      session_name: sessionMeta.sessionName,
    };

    const scenarios =
      args.scenario === 'all'
        ? ['attach_latency', 'reconnect_stress', 'duplicate_detector']
        : [args.scenario];

    for (const scenario of scenarios) {
      if (scenario === 'attach_latency') {
        report.results.attach_latency = await runAttachLatency(
          client,
          sessionMeta.sessionId,
          Math.min(args.loops, 15),
          args.captureMs
        );
      } else if (scenario === 'reconnect_stress') {
        report.results.reconnect_stress = await runReconnectStress(
          client,
          sessionMeta.sessionId,
          args.loops,
          args.captureMs
        );
      } else if (scenario === 'duplicate_detector') {
        report.results.duplicate_detector = await runDuplicateDetector(
          client,
          sessionMeta.sessionId,
          args.loops,
          args.captureMs
        );
      }
    }
  } catch (error) {
    runError = error;
  } finally {
    if (sessionMeta?.spawned) {
      await closeHarnessSession(client, sessionMeta.sessionId);
    }
    await client.close();
  }

  if (runError) {
    report.error = runError instanceof Error ? runError.message : String(runError);
  }

  const rendered = JSON.stringify(report, null, 2);
  console.log(rendered);
  if (args.output) {
    await writeFile(args.output, `${rendered}\n`, 'utf8');
  }

  if (runError) {
    process.exitCode = 1;
  }
}

main().catch((error) => {
  const message = error instanceof Error ? error.stack || error.message : String(error);
  console.error(message);
  process.exit(1);
});
