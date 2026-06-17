#!/bin/bash
# MobileCLI Uninstaller (Linux & macOS)
# Usage: curl -fsSL https://raw.githubusercontent.com/MobileCLI/mobilecli/main/uninstall.sh | bash
#
# If the `mobilecli` binary is on your PATH this simply delegates to the built-in
# `mobilecli uninstall`, which is the authoritative cleanup path. If the binary is
# missing or broken, this script falls back to removing everything the installer
# and setup wizard create.

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

BINARY_NAME="mobilecli"
CONFIG_DIR="${HOME}/.mobilecli"

info() { echo -e "${CYAN}$1${NC}"; }
success() { echo -e "${GREEN}✓ $1${NC}"; }
warn() { echo -e "${YELLOW}⚠ $1${NC}"; }

info "╔══════════════════════════════════════════════════════════════╗"
info "║              📱 MobileCLI Uninstaller                        ║"
info "╚══════════════════════════════════════════════════════════════╝"
echo

# Preferred path: let the binary uninstall itself (handles autostart, shell hook,
# config, and the binary in one consistent place).
if command -v "$BINARY_NAME" >/dev/null 2>&1; then
    info "Found ${BINARY_NAME} on PATH — running its built-in uninstaller..."
    if "$BINARY_NAME" uninstall --yes; then
        exit 0
    fi
    warn "Built-in uninstaller did not finish cleanly; running manual cleanup..."
fi

# Fallback: manual cleanup when the binary is missing or failed.
warn "Performing manual cleanup."

# Stop any running daemon by removing its socket/pid is handled by the binary; without
# it we just leave the process to exit. Best-effort kill via the recorded PID file.
if [ -f "${CONFIG_DIR}/daemon.pid" ]; then
    pid="$(cat "${CONFIG_DIR}/daemon.pid" 2>/dev/null || true)"
    if [ -n "${pid}" ] && kill -0 "${pid}" 2>/dev/null; then
        kill "${pid}" 2>/dev/null && success "Stopped daemon (PID: ${pid})"
    fi
fi

# Remove daemon autostart (Linux systemd user unit).
if command -v systemctl >/dev/null 2>&1; then
    systemctl --user disable --now mobilecli.service >/dev/null 2>&1 || true
fi
SYSTEMD_UNIT="${HOME}/.config/systemd/user/mobilecli.service"
if [ -f "${SYSTEMD_UNIT}" ]; then
    rm -f "${SYSTEMD_UNIT}"
    success "Removed systemd unit: ${SYSTEMD_UNIT}"
    systemctl --user daemon-reload >/dev/null 2>&1 || true
fi

# Remove daemon autostart (macOS launchd agent).
LAUNCHD_PLIST="${HOME}/Library/LaunchAgents/com.mobilecli.daemon.plist"
if [ -f "${LAUNCHD_PLIST}" ]; then
    launchctl unload -w "${LAUNCHD_PLIST}" >/dev/null 2>&1 || true
    rm -f "${LAUNCHD_PLIST}"
    success "Removed launchd agent: ${LAUNCHD_PLIST}"
fi

# Remove the shell auto-launch hook from rc files (sentinel-delimited block).
BEGIN_MARKER="# >>> mobilecli auto-launch >>>"
END_MARKER="# <<< mobilecli auto-launch <<<"
for rc in "${HOME}/.bashrc" "${HOME}/.bash_profile" "${HOME}/.profile" "${HOME}/.zshrc" "${HOME}/.config/fish/config.fish"; do
    if [ -f "${rc}" ] && grep -qF "${BEGIN_MARKER}" "${rc}"; then
        tmp="$(mktemp)"
        # Delete everything between (and including) the sentinel markers.
        awk -v b="${BEGIN_MARKER}" -v e="${END_MARKER}" '
            $0 == b {skip=1; next}
            skip && $0 == e {skip=0; next}
            !skip {print}
        ' "${rc}" > "${tmp}"
        mv "${tmp}" "${rc}"
        success "Removed shell hook from ${rc}"
    fi
done

# Remove the config directory (paired credentials, sessions, logs).
if [ -d "${CONFIG_DIR}" ]; then
    rm -rf "${CONFIG_DIR}"
    success "Removed config directory: ${CONFIG_DIR}"
fi

# Remove the binary from common install locations.
for dir in "${HOME}/.local/bin" "${HOME}/bin" "/usr/local/bin"; do
    target="${dir}/${BINARY_NAME}"
    if [ -f "${target}" ]; then
        if rm -f "${target}" 2>/dev/null; then
            success "Removed binary: ${target}"
        elif command -v sudo >/dev/null 2>&1; then
            sudo rm -f "${target}" && success "Removed binary: ${target}"
        else
            warn "Could not remove ${target} (insufficient permissions)"
        fi
    fi
done

echo
success "MobileCLI has been uninstalled."
