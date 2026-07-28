# voice2prompt (Rust)

Push-to-talk dictation: hold Right Ctrl, speak, release → transcribed & pasted.

Same architecture as the Python original, but compiled to native code:

- **v2p-daemon** — audio capture / Whisper STT / clipboard / tray icon (user)
- **v2p-listener** — Right Ctrl monitoring / Ctrl+V simulation (root)

## Quick start

```bash
./start.sh
```

Select a language, authenticate sudo once, and hold Right Ctrl to dictate.

## Manual build & run

```bash
# Build
cargo build --release

# Run daemon (terminal 1)
./target/release/v2p-daemon --language en

# Run listener as root (terminal 2)
sudo ./target/release/v2p-listener
```

## Requirements

- Linux with ALSA (input device)
- `wl-clipboard` or `xclip` (for clipboard paste fallback — currently unused, Rust uses `arboard`)
- GTK3 + libappindicator (for system tray — optional, app works without a display server)

## Build dependencies

```bash
sudo apt install build-essential pkg-config cmake \
  libgtk-3-dev libayatana-appindicator3-dev libxdo-dev
```

Then `cargo build --release`.

## Files

| File | Purpose |
|---|---|
| `daemon/src/main.rs` | User process: audio, STT, clipboard, tray, UDP command receiver |
| `listener/src/main.rs` | Root process: evdev keyboard reader, Ctrl+V injector, UDP command sender |
| `Cargo.toml` | Workspace definition |
| `start.sh` | Convenience launcher |
| `models/` | Downloaded Whisper GGML files (auto-downloaded to `~/.local/share/voice2prompt/models/`) |
