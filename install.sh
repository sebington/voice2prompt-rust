#!/bin/bash
# voice2prompt — one-time install:
#   1. Builds the binary (if needed)
#   2. Installs a udev rule granting the `input` group read access to
#      /dev/input/event* and write access to /dev/uinput
#   3. Adds the invoking user to the `input` group
#   4. Installs `v2p` to /usr/local/bin
#
# After this, log out and back in (or run: newgrp input) for the group
# membership to take effect. Then just run `v2p` — no sudo needed.

set -e
cd "$(dirname "$0")"

# Build first, as the invoking user (sudo would have root's HOME)
if [ ! -x target/release/v2p ]; then
    echo "Building…"
    . "$HOME/.cargo/env" 2>/dev/null || true
    cargo build --release
fi

if [ "$(id -u)" -ne 0 ]; then
    echo "Re-running with sudo…"
    exec sudo "$0" "$@"
fi

RULES=/etc/udev/rules.d/99-voice2prompt.rules

install -m 644 packaging/99-voice2prompt.rules "$RULES"
echo "Installed $RULES"

udevadm control --reload-rules
udevadm trigger
echo "Udev rules reloaded"

USER_="${SUDO_USER:-}"
if [ -n "$USER_" ] && ! id -nG "$USER_" | grep -qw input; then
    usermod -a -G input "$USER_"
    echo "Added '$USER_' to the 'input' group."
fi

install -m 755 target/release/v2p /usr/local/bin/v2p
echo "Installed /usr/local/bin/v2p"

echo ""
echo "Done. Log out and back in (or run: newgrp input) to activate."
echo "Then verify with: v2p doctor"
