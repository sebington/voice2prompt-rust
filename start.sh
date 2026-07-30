#!/bin/bash
# voice2prompt — Rust launcher
set -e

cd "$(dirname "$0")"

BIN_DIR="target/release"
DAEMON="$BIN_DIR/v2p-daemon"
LISTENER="$BIN_DIR/v2p-listener"

# Build if needed
if [ ! -x "$DAEMON" ] || [ ! -x "$LISTENER" ]; then
    echo "Building…"
    . "$HOME/.cargo/env" 2>/dev/null || true
    cargo build --release
fi

cleanup() {
    echo ""
    echo "Shutting down…"
    [ -n "$DAEMON_PID" ] && kill "$DAEMON_PID" 2>/dev/null
    [ -n "$ROOT_PID" ]   && sudo kill "$ROOT_PID" 2>/dev/null
    exit 0
}
trap cleanup SIGINT SIGTERM

# Sudo upfront
echo "Requesting sudo for keyboard listener…"
sudo -v || { echo "sudo required"; exit 1; }

# Kill stale instances from previous runs
pkill -f v2p-daemon 2>/dev/null || true
sudo pkill -f v2p-listener 2>/dev/null || true
sleep 1

# Start daemon (user process)
echo "Starting audio service…"
"$DAEMON" --language en &
DAEMON_PID=$!
sleep 2

# Start listener (root process)
echo "Starting keyboard listener…"
sudo "$LISTENER" &
ROOT_PID=$!

echo "Hold Right Ctrl to record, release to transcribe & paste"
echo "Press Ctrl+C to stop"

wait "$ROOT_PID" "$DAEMON_PID" 2>/dev/null
cleanup
