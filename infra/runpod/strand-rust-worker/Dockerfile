FROM nvidia/cuda:12.1.1-cudnn8-devel-ubuntu22.04

ENV DEBIAN_FRONTEND=noninteractive \
    PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    PIP_NO_CACHE_DIR=1 \
    HF_HOME=/workspace/.cache/huggingface \
    MODEL_CACHE_DIR=/workspace/models

RUN apt-get update && apt-get install -y --no-install-recommends \
    python3 python3-pip python3-venv python3-dev \
    build-essential cmake ninja-build pkg-config git curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace

COPY requirements.txt /workspace/requirements.txt

RUN python3 -m pip install --upgrade pip setuptools wheel \
    && python3 -m pip install -r /workspace/requirements.txt \
    && python3 -m pip install \
      --extra-index-url https://abetlen.github.io/llama-cpp-python/whl/cu121 \
      --only-binary=:all: \
      "llama-cpp-python==0.2.90"

COPY handler.py /workspace/handler.py

CMD ["python3", "-u", "/workspace/handler.py"]
