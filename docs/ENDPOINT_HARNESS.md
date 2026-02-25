# Endpoint Harness

Reusable CLI to talk to multiple endpoint styles from one command.

## Files

- `scripts/endpoint-harness.sh`
- `tooling/endpoint-harness/profiles.json`

## Quick Start

```bash
cd /home/bigphoot/Desktop/Projects/MobileCLI
chmod +x scripts/endpoint-harness.sh
scripts/endpoint-harness.sh --list
```

Run the deployed RunPod profile:

```bash
cd /home/bigphoot/Desktop/Projects/MobileCLI
export RUNPOD_API_KEY=<your-runpod-api-key>
scripts/endpoint-harness.sh --profile runpod-strand-prod --prompt "Reply with exactly READY"
```

## Profile Types

`runpod`
- Calls `/run` or `/runsync` and auto-polls `/status/<id>` until terminal state.

`openai_chat`
- Calls OpenAI-compatible `/chat/completions`.

`http_json`
- Generic JSON endpoint with configurable prompt field.

## Useful Environment Knobs

- `TIMEOUT=900`
- `POLL_INTERVAL=2`
- `MAX_NEW_TOKENS=768`
- `TEMPERATURE=0.1`
- `TOP_P=0.9`
- `REPEAT_PENALTY=1.05`

## Examples

Prompt from stdin:

```bash
cat prompts/rust-review-system.md | scripts/endpoint-harness.sh --profile runpod-strand-prod
```

Raw JSON:

```bash
scripts/endpoint-harness.sh --profile runpod-strand-prod --prompt "healthcheck" --raw
```
