# Review: protocol.rs
Model: Strand-Rust-Coder-14B (RunPod) | 2026-02-20T17:56:50-08:00

## protocol.rs (545 lines, 2 chunks)

### Chunk 1/2 (lines 1-500)

**Finding 1:** Command injection via session names or arguments in `SpawnSession` message.

*Description:* The `command` and `args` fields are directly used to spawn a process without sanitization. An attacker could inject malicious commands by crafting specially crafted strings for these fields, leading to arbitrary command execution on the server.

**Fix:** Validate that both `command` and each element of `args` contain only alphanumeric characters or common punctuation (e.g., hyphens, underscores) before spawning the process. Reject any input containing shell metacharacters like semicolons, pipes, redirection operators, etc.

---

REVIEW_COMPLETE

### Chunk 2/2 (lines 501-545)

**Finding 1:** The `to_compact_qr` method builds a URL string without validating the input parameters (device_id and device_name). If these contain characters that are not properly encoded or if they exceed length limits for QR codes, it could lead to malformed URLs or truncated data when scanned by mobile clients. This can cause pairing failures.

**Fix:** Add validation checks on `device_id` and `device_name` before encoding them into the URL string. Ensure they only contain allowed characters (alphanumeric plus a few special chars) and are within reasonable length limits for QR codes, then encode using proper percent-encoding to prevent injection attacks or truncation issues.

**Finding 2:** The method does not handle cases where `device_id` or `device_name` might be empty strings. While technically valid URLs can contain these parameters with empty values, it may lead to unexpected behavior on the mobile side that expects non-empty identifiers for device pairing.

**Fix:** Add explicit checks for empty string inputs and either reject them (return an error) or encode them as empty percent-encoded strings (`""` becomes `%20`) before including in the URL. This ensures consistent handling of edge cases without silently dropping important information.

REVIEW_COMPLETE

---
Complete: 2026-02-20T17:56:59-08:00
