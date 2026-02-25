#!/usr/bin/env python3
"""RunPod serverless worker for Strand Rust GGUF via llama.cpp."""

from __future__ import annotations

import os
from pathlib import Path
from typing import Any, Dict, List, Optional

import runpod
from huggingface_hub import hf_hub_download
from llama_cpp import Llama


def _env_int(name: str, default: int) -> int:
    value = os.getenv(name)
    if value is None:
        return default
    try:
        return int(value)
    except ValueError:
        return default


def _env_float(name: str, default: float) -> float:
    value = os.getenv(name)
    if value is None:
        return default
    try:
        return float(value)
    except ValueError:
        return default


MODEL_REPO = os.getenv(
    "STRAND_MODEL_REPO",
    "Fortytwo-Network/Strand-Rust-Coder-14B-v1-GGUF",
)
MODEL_FILE = os.getenv(
    "STRAND_MODEL_FILE",
    "Fortytwo_Strand-Rust-Coder-14B-v1-Q4_K_M.gguf",
)
MODEL_CACHE_DIR = os.getenv("MODEL_CACHE_DIR", "/workspace/models")
HF_TOKEN = os.getenv("HF_TOKEN")

N_CTX = _env_int("STRAND_N_CTX", 8192)
N_BATCH = _env_int("STRAND_N_BATCH", 512)
N_GPU_LAYERS = _env_int("STRAND_N_GPU_LAYERS", -1)
THREADS = _env_int("STRAND_THREADS", 0)

DEFAULT_MAX_TOKENS = _env_int("STRAND_DEFAULT_MAX_TOKENS", 768)
DEFAULT_TEMPERATURE = _env_float("STRAND_DEFAULT_TEMPERATURE", 0.1)
DEFAULT_TOP_P = _env_float("STRAND_DEFAULT_TOP_P", 0.9)
DEFAULT_REPEAT_PENALTY = _env_float("STRAND_DEFAULT_REPEAT_PENALTY", 1.05)

SYSTEM_PROMPT = os.getenv(
    "STRAND_SYSTEM_PROMPT",
    (
        "You are Strand-Rust-Coder, specialized in Rust refactoring and code review. "
        "Preserve behavior unless explicitly asked to change behavior."
    ),
)

_model: Optional[Llama] = None


def _model_path() -> str:
    Path(MODEL_CACHE_DIR).mkdir(parents=True, exist_ok=True)
    return hf_hub_download(
        repo_id=MODEL_REPO,
        filename=MODEL_FILE,
        token=HF_TOKEN,
        local_dir=MODEL_CACHE_DIR,
        local_dir_use_symlinks=False,
    )


def _get_model() -> Llama:
    global _model
    if _model is None:
        path = _model_path()
        _model = Llama(
            model_path=path,
            n_ctx=N_CTX,
            n_batch=N_BATCH,
            n_gpu_layers=N_GPU_LAYERS,
            n_threads=THREADS if THREADS > 0 else None,
            verbose=False,
        )
    return _model


def _build_messages(payload: Dict[str, Any]) -> List[Dict[str, str]]:
    provided_messages = payload.get("messages")
    if isinstance(provided_messages, list) and provided_messages:
        # Ensure a system message is present at the front for consistency.
        first = provided_messages[0]
        if not isinstance(first, dict) or first.get("role") != "system":
            return [{"role": "system", "content": SYSTEM_PROMPT}, *provided_messages]
        return provided_messages

    prompt = payload.get("prompt")
    if not isinstance(prompt, str) or not prompt.strip():
        raise ValueError("`prompt` (string) or `messages` (array) is required")

    return [
        {"role": "system", "content": SYSTEM_PROMPT},
        {"role": "user", "content": prompt},
    ]


def handler(job: Dict[str, Any]) -> Dict[str, Any]:
    payload = job.get("input") or {}
    if not isinstance(payload, dict):
        return {"error": "`input` must be an object"}

    try:
        model = _get_model()
        messages = _build_messages(payload)

        max_tokens = int(payload.get("max_new_tokens", DEFAULT_MAX_TOKENS))
        temperature = float(payload.get("temperature", DEFAULT_TEMPERATURE))
        top_p = float(payload.get("top_p", DEFAULT_TOP_P))
        repeat_penalty = float(payload.get("repeat_penalty", DEFAULT_REPEAT_PENALTY))

        result = model.create_chat_completion(
            messages=messages,
            max_tokens=max_tokens,
            temperature=temperature,
            top_p=top_p,
            repeat_penalty=repeat_penalty,
        )

        text = (
            result.get("choices", [{}])[0]
            .get("message", {})
            .get("content", "")
            .strip()
        )
        return {
            "text": text,
            "model_repo": MODEL_REPO,
            "model_file": MODEL_FILE,
            "usage": result.get("usage", {}),
        }
    except Exception as exc:  # noqa: BLE001
        return {"error": str(exc)}


if __name__ == "__main__":
    runpod.serverless.start({"handler": handler})
