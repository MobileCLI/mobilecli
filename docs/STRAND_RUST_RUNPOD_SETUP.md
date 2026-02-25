# Strand Rust RunPod Setup

This config deploys `Strand-Rust-Coder-14B-v1` (GGUF) as a RunPod serverless endpoint, then lets your existing review script use that endpoint.

## 1) Build and push image

```bash
cd /home/bigphoot/Desktop/Projects/MobileCLI
chmod +x scripts/runpod-build-strand-worker.sh
IMAGE=ghcr.io/<your-org>/mobilecli-strand-worker:latest PUSH=1 \
  scripts/runpod-build-strand-worker.sh
```

## 2) Create RunPod serverless endpoint

In RunPod:
1. Create a **Serverless Endpoint**.
2. Use your image:
   - `ghcr.io/<your-org>/mobilecli-strand-worker:latest`
3. Choose a GPU class suitable for 14B GGUF (24GB+ recommended for comfort).
4. Set container env vars:
   - `STRAND_MODEL_REPO=Fortytwo-Network/Strand-Rust-Coder-14B-v1-GGUF`
   - `STRAND_MODEL_FILE=Fortytwo_Strand-Rust-Coder-14B-v1-Q4_K_M.gguf`
   - `STRAND_N_CTX=8192`
   - `STRAND_N_GPU_LAYERS=-1`
   - `HF_TOKEN=<token>` (if needed by your model access policy)

## 3) Smoke test endpoint

```bash
cd /home/bigphoot/Desktop/Projects/MobileCLI
chmod +x scripts/runpod-strand-smoke.sh
export RUNPOD_API_KEY=<your-runpod-api-key>
export RUNPOD_ENDPOINT_ID=<your-endpoint-id>
scripts/runpod-strand-smoke.sh
```

Expected output includes `output.text` with a short reply.

## 4) Use endpoint from review script (Claude-compatible)

```bash
cd /home/bigphoot/Desktop/Projects/MobileCLI
export RUNPOD_API_KEY=<your-runpod-api-key>
export RUNPOD_ENDPOINT_ID=<your-endpoint-id>
STRAND_BACKEND=runpod \
  scripts/strand-rust-review.sh --file cli/src/autostart.rs
```

For large files:

```bash
STRAND_BACKEND=runpod NUM_PREDICT=400 REQUEST_TIMEOUT=1200 \
  scripts/strand-rust-review.sh --file cli/src/daemon.rs
```

## Notes

- `scripts/strand-rust-review.sh` now supports:
  - `--backend ollama` (default)
  - `--backend runpod`
- RunPod path uses `POST /v2/<endpoint-id>/runsync` and expects worker output at `output.text`.
