#!/bin/bash
# MobileCLI Installer (Linux & macOS)
# Usage: curl -fsSL https://raw.githubusercontent.com/MobileCLI/mobilecli/main/install.sh | bash
#
# Windows users: install via cargo or download binaries from GitHub Releases.
#   cargo install --git https://github.com/MobileCLI/mobilecli --path cli

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

REPO="MobileCLI/mobilecli"
BINARY_NAME="mobilecli"

# Print styled messages
info() { echo -e "${CYAN}$1${NC}"; }
success() { echo -e "${GREEN}✓ $1${NC}"; }
warn() { echo -e "${YELLOW}⚠ $1${NC}"; }
error() { echo -e "${RED}✗ $1${NC}" >&2; exit 1; }

# Map OS/arch to Rust target triple (matches release workflow archives)
detect_target() {
    local os arch

    os="$(uname -s)"
    arch="$(uname -m)"

    case "${os}-${arch}" in
        Linux-x86_64)    echo "x86_64-unknown-linux-gnu" ;;
        Linux-aarch64)   echo "aarch64-unknown-linux-gnu" ;;
        Darwin-x86_64)   echo "x86_64-apple-darwin" ;;
        Darwin-arm64)    echo "aarch64-apple-darwin" ;;
        *) error "Unsupported platform: ${os} ${arch}" ;;
    esac
}

# Get the latest release version
get_latest_version() {
    local version
    version=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/')

    if [ -z "$version" ]; then
        error "Failed to fetch latest version. Check your internet connection."
    fi

    echo "$version"
}

# Get install directory
get_install_dir() {
    # Check for common user-local bin directories
    if [ -d "$HOME/.local/bin" ] && echo "$PATH" | grep -q "$HOME/.local/bin"; then
        echo "$HOME/.local/bin"
    elif [ -d "$HOME/bin" ] && echo "$PATH" | grep -q "$HOME/bin"; then
        echo "$HOME/bin"
    elif [ -w "/usr/local/bin" ]; then
        echo "/usr/local/bin"
    else
        # Create ~/.local/bin if it doesn't exist
        mkdir -p "$HOME/.local/bin"
        echo "$HOME/.local/bin"
    fi
}

# Download and install
install() {
    local platform version install_dir archive_name download_url tmp_dir

    info "╔══════════════════════════════════════════════════════════════╗"
    info "║              📱 MobileCLI Installer                          ║"
    info "╚══════════════════════════════════════════════════════════════╝"
    echo

    # Detect target
    platform=$(detect_target)
    info "Detected target: $platform"

    # Get latest version
    info "Fetching latest version..."
    version=$(get_latest_version)
    success "Latest version: $version"

    # Determine install directory
    install_dir=$(get_install_dir)
    info "Install directory: $install_dir"

    # Construct download URL
    archive_name="${BINARY_NAME}-${version}-${platform}.tar.gz"
    download_url="https://github.com/${REPO}/releases/download/${version}/${archive_name}"

    # Create temp directory
    tmp_dir=$(mktemp -d)
    trap "rm -rf $tmp_dir" EXIT

    # Download archive
    info "Downloading ${archive_name}..."
    if ! curl -fsSL "$download_url" -o "${tmp_dir}/${archive_name}"; then
        error "Failed to download from $download_url"
    fi
    success "Downloaded successfully"

    # Extract archive
    info "Extracting..."
    tar -xzf "${tmp_dir}/${archive_name}" -C "$tmp_dir"

    # Install binary
    info "Installing to ${install_dir}..."
    if [ -w "$install_dir" ]; then
        mv "${tmp_dir}/${BINARY_NAME}" "${install_dir}/${BINARY_NAME}"
        chmod +x "${install_dir}/${BINARY_NAME}"
    else
        warn "Need sudo to install to ${install_dir}"
        sudo mv "${tmp_dir}/${BINARY_NAME}" "${install_dir}/${BINARY_NAME}"
        sudo chmod +x "${install_dir}/${BINARY_NAME}"
    fi
    success "Installed to ${install_dir}/${BINARY_NAME}"

    # Check if install_dir is in PATH
    if ! echo "$PATH" | grep -q "$install_dir"; then
        echo
        warn "Note: $install_dir is not in your PATH"
        echo "Add it to your shell config:"
        echo "  export PATH=\"\$PATH:$install_dir\""
        echo
    fi

    # Verify installation
    echo
    success "╔══════════════════════════════════════════════════════════════╗"
    success "║              ✓ Installation Complete!                        ║"
    success "╚══════════════════════════════════════════════════════════════╝"
    echo
    info "Run 'mobilecli setup' to get started!"
    echo

    # Show version
    if command -v mobilecli &> /dev/null; then
        mobilecli --version
    fi
}

# Run installation
install
