# Review: watcher.rs
Model: Strand-Rust-Coder-14B (RunPod) | 2026-02-20T17:57:17-08:00

## watcher.rs (110 lines)

**Finding 1:** The `watch` method clones the entire path string for every watcher entry and also calls `to_string()` on it when inserting into `DashMap`. This creates multiple heap allocations per watch operation.

**Fix**: Use a single owned `String` (`let key = path.to_string();`) once, avoiding repeated cloning. Also remove unnecessary `.clone()` inside the closure where possible.

---

REVIEW_COMPLETE

---
Complete: 2026-02-20T17:57:19-08:00
