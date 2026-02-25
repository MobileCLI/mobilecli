---
name: endpoint-harness
description: Use when the user wants to query or test LLM endpoints (RunPod, OpenAI-compatible, or generic JSON APIs) with reusable profiles. Executes scripts/endpoint-harness.sh and reads tooling/endpoint-harness/profiles.json.
---

# Endpoint Harness

Use this skill for endpoint testing, prompt checks, and integration verification.

## Core Commands

List profiles:

```bash
cd /home/bigphoot/Desktop/Projects/MobileCLI
scripts/endpoint-harness.sh --list
```

Run a prompt:

```bash
cd /home/bigphoot/Desktop/Projects/MobileCLI
scripts/endpoint-harness.sh --profile <profile-name> --prompt "..."
```

Run with stdin:

```bash
cat <file> | scripts/endpoint-harness.sh --profile <profile-name>
```

## Files

- Harness script: `scripts/endpoint-harness.sh`
- Profile registry: `tooling/endpoint-harness/profiles.json`

## Required Auth

Profiles define `api_key_env`. Set those env vars before running commands.

## Useful Runtime Knobs

- `TIMEOUT`
- `POLL_INTERVAL`
- `MAX_NEW_TOKENS`
- `TEMPERATURE`
- `TOP_P`
- `REPEAT_PENALTY`

## Notes

- `runpod` profiles auto-poll until terminal state.
- `openai_chat` profiles call chat-completions endpoints.
- `http_json` profiles support generic JSON endpoints with configurable `prompt_field`.
