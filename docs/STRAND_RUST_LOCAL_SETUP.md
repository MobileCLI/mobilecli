# Strand Rust Local Setup (Ollama)

This repo includes a local setup for `Strand-Rust-Coder-14B-v1` so Claude can use it for Rust review/suggestion passes.

## 1) Create the local model alias

Default (recommended on this machine):

```bash
cd /home/bigphoot/Desktop/Projects/MobileCLI
chmod +x scripts/setup-strand-rust.sh
scripts/setup-strand-rust.sh
```

This creates:
- alias: `strand-rust-coder:14b-q4`
- quant: `Q4_K_M`

Optional higher quality (slower/heavier):

```bash
scripts/setup-strand-rust.sh strand-rust-coder:14b-q5 Q5_K_M
```

## 2) Run a Rust review pass

Review one file:

```bash
cd /home/bigphoot/Desktop/Projects/MobileCLI
chmod +x scripts/strand-rust-review.sh
scripts/strand-rust-review.sh --file cli/src/autostart.rs
```

Review working-tree diff:

```bash
scripts/strand-rust-review.sh --diff
```

Select backend explicitly:

```bash
scripts/strand-rust-review.sh --backend ollama --file cli/src/autostart.rs
```

Use a specific model alias:

```bash
scripts/strand-rust-review.sh --file cli/src/daemon.rs --model strand-rust-coder:14b-q5
```

Tune runtime for large files (CPU-heavy on this machine):

```bash
NUM_PREDICT=400 REQUEST_TIMEOUT=1200 \
  scripts/strand-rust-review.sh --file cli/src/daemon.rs --model strand-rust-coder:14b-q4
```

## 3) Claude workflow recommendation

1. Claude asks Strand for findings on a specific file or diff.
2. Claude implements patches directly in repo.
3. Claude validates:
   - `cargo fmt --check`
   - `cargo clippy --all-targets --all-features`
   - `cargo test`
4. Claude summarizes risk + test impact before commit.

## Notes

- Prompt template used by review script:
  - `prompts/strand_rust_review_prompt.md`
- Modelfile reference:
  - `tooling/llm/Modelfile.strand-rust-q4`
- Current local processing path:
  - CPU inference (model works, but large reviews can take minutes)
- For RunPod deployment and endpoint usage:
  - `docs/STRAND_RUST_RUNPOD_SETUP.md`
