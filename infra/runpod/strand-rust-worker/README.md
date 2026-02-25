# Strand Rust RunPod Worker

Serverless worker that loads the GGUF Strand model and serves inference through RunPod jobs.

## Image contents
- `llama-cpp-python` with CUDA support
- `runpod` Python SDK
- model download via `huggingface_hub`

## Input contract
RunPod job `input` object:

```json
{
  "prompt": "string",
  "max_new_tokens": 768,
  "temperature": 0.1,
  "top_p": 0.9,
  "repeat_penalty": 1.05
}
```

Optional chat-style input:

```json
{
  "messages": [
    {"role": "system", "content": "..."},
    {"role": "user", "content": "..."}
  ]
}
```

## Output contract

```json
{
  "text": "model output",
  "model_repo": "...",
  "model_file": "...",
  "usage": {}
}
```

## Important environment variables
- `STRAND_MODEL_REPO` (default: `Fortytwo-Network/Strand-Rust-Coder-14B-v1-GGUF`)
- `STRAND_MODEL_FILE` (default: `Fortytwo_Strand-Rust-Coder-14B-v1-Q4_K_M.gguf`)
- `HF_TOKEN` (needed for gated/private model repos)
- `STRAND_N_CTX` (default: `8192`)
- `STRAND_N_GPU_LAYERS` (default: `-1`, try all layers on GPU)
- `STRAND_THREADS` (optional CPU tuning)
