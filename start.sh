#!/bin/bash
# voice2prompt — launcher
set -e

cd "$(dirname "$0")"

BIN="target/release/v2p"

# Build if needed
if [ ! -x "$BIN" ]; then
    echo "Building…"
    . "$HOME/.cargo/env" 2>/dev/null || true
    cargo build --release
fi

# Kill stale instances from previous runs (pattern must not match this script)
pkill -f 'target/release/v2p run'    2>/dev/null || true
pkill -f 'target/release/v2p listen' 2>/dev/null || true
pkill -f 'target/release/v2p daemon' 2>/dev/null || true
sleep 1

exec "$BIN" run "$@"
