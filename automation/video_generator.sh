#!/bin/bash
# MobileCLI Automated Demo Video Generator
#
# Generates fresh demo videos for social media using Android emulator on Linux.
# Requires: Android SDK, emulator, adb, maestro, ffmpeg
#
# Pipeline:
#   1. Boot Android emulator (headless)
#   2. Install MobileCLI app (Expo/React Native build)
#   3. Run Maestro flow (scripted demo interactions)
#   4. Record screen via adb
#   5. Post-process with ffmpeg (device frame, captions, trim)
#   6. Output social-ready video files
#
# Setup:
#   ./video_generator.sh setup    # Install dependencies
#   ./video_generator.sh record   # Record a demo
#   ./video_generator.sh process  # Post-process existing recording

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
OUTPUT_DIR="$SCRIPT_DIR/video_output"
MAESTRO_DIR="$SCRIPT_DIR/maestro_flows"
RECORDING_FILE="/sdcard/mobilecli-demo.mp4"
LOCAL_RECORDING="$OUTPUT_DIR/raw_recording.mp4"

mkdir -p "$OUTPUT_DIR" "$MAESTRO_DIR"

# ─── Colors ──────────────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

log() { echo -e "${GREEN}[+]${NC} $1"; }
warn() { echo -e "${YELLOW}[!]${NC} $1"; }
err() { echo -e "${RED}[✗]${NC} $1"; }

# ─── Setup ───────────────────────────────────────────────────────────
cmd_setup() {
    log "Setting up video generation dependencies..."

    # Check for Android SDK
    if ! command -v adb &>/dev/null; then
        warn "Android SDK not found. Install it:"
        echo "  sudo apt install android-sdk"
        echo "  # Or download from: https://developer.android.com/studio#command-tools"
        echo "  # Then: sdkmanager 'emulator' 'platform-tools' 'system-images;android-34;google_apis;x86_64'"
        echo "  # Then: avdmanager create avd -n mobilecli_demo -k 'system-images;android-34;google_apis;x86_64'"
    else
        log "adb found: $(which adb)"
    fi

    # Check for emulator
    if ! command -v emulator &>/dev/null; then
        warn "Android emulator not found. Add \$ANDROID_HOME/emulator to PATH"
    else
        log "emulator found: $(which emulator)"
        log "Available AVDs:"
        emulator -list-avds 2>/dev/null || true
    fi

    # Check for Maestro
    if ! command -v maestro &>/dev/null; then
        warn "Maestro not found. Install it:"
        echo "  curl -fsSL https://get.maestro.mobile.dev | bash"
    else
        log "maestro found: $(which maestro)"
    fi

    # Check for ffmpeg
    if ! command -v ffmpeg &>/dev/null; then
        warn "ffmpeg not found. Install it:"
        echo "  sudo apt install ffmpeg"
    else
        log "ffmpeg found: $(which ffmpeg)"
    fi

    # Check for scrcpy (optional, better recording)
    if ! command -v scrcpy &>/dev/null; then
        warn "scrcpy not found (optional, better recordings):"
        echo "  sudo apt install scrcpy"
    else
        log "scrcpy found: $(which scrcpy)"
    fi

    # Create default Maestro flow if not exists
    if [ ! -f "$MAESTRO_DIR/demo_flow.yaml" ]; then
        log "Creating default Maestro demo flow..."
        cat > "$MAESTRO_DIR/demo_flow.yaml" << 'FLOW'
# MobileCLI Demo Flow
# Customize this to match your app's UI
appId: com.mobilecli.app
---
# Launch the app
- launchApp

# Wait for app to load
- waitForAnimationToEnd

# Tap on a session (customize selector)
- tapOn: "Claude Code"

# Wait for terminal to render
- waitForAnimationToEnd
- scroll:
    direction: DOWN

# Show the approval flow
- waitForAnimationToEnd

# Navigate to file browser
- tapOn: "Files"
- waitForAnimationToEnd

# Go back to sessions
- tapOn: "Sessions"
- waitForAnimationToEnd
FLOW
        log "Default flow created at $MAESTRO_DIR/demo_flow.yaml"
        warn "Edit this flow to match your actual app UI!"
    fi

    log "Setup complete. Edit $MAESTRO_DIR/demo_flow.yaml, then run: $0 record"
}

# ─── Record ──────────────────────────────────────────────────────────
cmd_record() {
    log "Starting demo recording..."

    # Check emulator is running
    if ! adb devices | grep -q "emulator"; then
        log "Starting emulator..."
        AVD=$(emulator -list-avds 2>/dev/null | head -1)
        if [ -z "$AVD" ]; then
            err "No AVD found. Run: $0 setup"
            exit 1
        fi
        emulator -avd "$AVD" -no-audio -no-boot-anim &
        adb wait-for-device
        sleep 10
        log "Emulator booted: $AVD"
    fi

    # Clean status bar for professional recording
    adb shell settings put global sysui_demo_allowed 1
    adb shell am broadcast -a com.android.systemui.demo -e command clock -e hhmm 0941
    adb shell am broadcast -a com.android.systemui.demo -e command battery -e level 100 -e plugged false
    adb shell am broadcast -a com.android.systemui.demo -e command network -e wifi show -e level 4
    adb shell am broadcast -a com.android.systemui.demo -e command notifications -e visible false

    # Start recording
    log "Recording screen (max 180s)..."
    adb shell screenrecord --size 1080x1920 --bit-rate 8000000 "$RECORDING_FILE" &
    RECORD_PID=$!

    # Run Maestro flow
    if command -v maestro &>/dev/null && [ -f "$MAESTRO_DIR/demo_flow.yaml" ]; then
        log "Running Maestro flow..."
        maestro test "$MAESTRO_DIR/demo_flow.yaml" || warn "Maestro flow had issues"
        sleep 2
    else
        warn "No Maestro flow. Recording for 30 seconds..."
        sleep 30
    fi

    # Stop recording
    kill $RECORD_PID 2>/dev/null || true
    sleep 2

    # Pull recording
    adb pull "$RECORDING_FILE" "$LOCAL_RECORDING"
    adb shell rm "$RECORDING_FILE"

    # Reset demo mode
    adb shell am broadcast -a com.android.systemui.demo -e command exit

    log "Raw recording saved: $LOCAL_RECORDING"
    log "Run '$0 process' to create social-ready videos"
}

# ─── Process ─────────────────────────────────────────────────────────
cmd_process() {
    if [ ! -f "$LOCAL_RECORDING" ]; then
        err "No recording found at $LOCAL_RECORDING. Run '$0 record' first."
        exit 1
    fi

    log "Processing recording into social-ready formats..."

    # Square crop for Twitter/Instagram (1080x1080)
    ffmpeg -y -i "$LOCAL_RECORDING" \
        -vf "crop=1080:1080:0:420,scale=1080:1080" \
        -c:v libx264 -preset medium -crf 23 \
        -an \
        "$OUTPUT_DIR/demo-square.mp4" 2>/dev/null
    log "Square (1080x1080): $OUTPUT_DIR/demo-square.mp4"

    # Portrait for TikTok/Stories (1080x1920)
    ffmpeg -y -i "$LOCAL_RECORDING" \
        -vf "scale=1080:1920" \
        -c:v libx264 -preset medium -crf 23 \
        -an \
        "$OUTPUT_DIR/demo-portrait.mp4" 2>/dev/null
    log "Portrait (1080x1920): $OUTPUT_DIR/demo-portrait.mp4"

    # Wide for YouTube/LinkedIn (1920x1080) with phone frame
    ffmpeg -y -i "$LOCAL_RECORDING" \
        -vf "scale=608:1080,pad=1920:1080:(1920-608)/2:0:black" \
        -c:v libx264 -preset medium -crf 23 \
        -an \
        "$OUTPUT_DIR/demo-wide.mp4" 2>/dev/null
    log "Wide (1920x1080): $OUTPUT_DIR/demo-wide.mp4"

    # GIF for GitHub README / tweets (480px wide, 15 fps, 15 sec max)
    ffmpeg -y -i "$LOCAL_RECORDING" \
        -t 15 \
        -vf "fps=15,scale=480:-1:flags=lanczos,crop=480:480:0:210" \
        -loop 0 \
        "$OUTPUT_DIR/demo.gif" 2>/dev/null
    log "GIF (480x480): $OUTPUT_DIR/demo.gif"

    log "All formats generated in $OUTPUT_DIR/"
    ls -lh "$OUTPUT_DIR/"
}

# ─── Main ────────────────────────────────────────────────────────────
case "${1:-help}" in
    setup)   cmd_setup ;;
    record)  cmd_record ;;
    process) cmd_process ;;
    *)
        echo "MobileCLI Demo Video Generator"
        echo ""
        echo "Usage:"
        echo "  $0 setup    - Install dependencies and create Maestro flow"
        echo "  $0 record   - Boot emulator, run flow, record screen"
        echo "  $0 process  - Post-process raw recording into social formats"
        echo ""
        echo "Output formats: square (Twitter), portrait (TikTok), wide (YouTube), GIF (GitHub)"
        ;;
esac
