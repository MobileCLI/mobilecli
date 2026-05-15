#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# shellcheck source=../install.sh
source "${ROOT_DIR}/install.sh"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "${tmp_dir}"' EXIT

archive_name="mobilecli-v0.0.0-x86_64-unknown-linux-gnu.tar.gz"
archive_path="${tmp_dir}/${archive_name}"
checksum_path="${tmp_dir}/SHA256SUMS.txt"

printf 'mobilecli-test-archive' >"${archive_path}"
valid_digest="$(sha256_digest "${archive_path}")"

expect_success() {
    local label="$1"
    if ! verify_archive_checksum "${archive_path}" "${archive_name}" "${checksum_path}" >/dev/null 2>&1; then
        printf 'FAIL: expected success for %s\n' "${label}" >&2
        exit 1
    fi
}

expect_failure() {
    local label="$1"
    if (verify_archive_checksum "${archive_path}" "${archive_name}" "${checksum_path}") >/dev/null 2>&1; then
        printf 'FAIL: expected failure for %s\n' "${label}" >&2
        exit 1
    fi
}

printf '%s  %s\n' "${valid_digest}" "${archive_name}" >"${checksum_path}"
expect_success "valid checksum"

printf '%s  *%s\n' "${valid_digest}" "${archive_name}" >"${checksum_path}"
expect_success "coreutils star-prefixed checksum"

printf '%s  ./archive-other.tar.gz\n' "${valid_digest}" >"${checksum_path}"
expect_failure "missing archive entry"

printf 'not-a-sha  %s\n' "${archive_name}" >"${checksum_path}"
expect_failure "invalid digest shape"

printf '%064d  %s\n' 0 "${archive_name}" >"${checksum_path}"
expect_failure "wrong digest"

: >"${checksum_path}"
expect_failure "empty checksum manifest"

printf 'installer checksum tests: ok\n'
