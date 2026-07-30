# voice2prompt

Push-to-talk dictation for Linux: hold **Right Ctrl**, speak, release — speech is
transcribed locally with Whisper and pasted into the active application.
System-wide, works in any app.

Two native binaries communicate over UDP on localhost:

- **v2p-daemon** (user) — audio capture, Whisper STT, clipboard, tray icon
- **v2p-listener** (root) — Right Ctrl monitoring via evdev, Ctrl+V injection via uinput

## Quick start

```bash
./start.sh
```

Builds if needed, asks for sudo once (listener needs `/dev/input` access),
kills stale instances from previous runs, then starts both processes.
Hold Right Ctrl to dictate. Ctrl+C stops everything.

## Features

- **Local transcription** — Whisper.cpp (`whisper-rs`), no network, no API key
- **System tray indicator** — green = ready, red = recording, yellow = transcribing;
  hover for state tooltip, click for a Quit menu
- **Terminal feedback** — `Recording…` → `Transcribed: …` → `Pasted.`
- **Languages** — English (`--language en`, default) and French (`--language fr`)
- **Model auto-download** — Whisper `tiny` model fetched on first run with progress
  to `~/.local/share/voice2prompt/models/`
- **Clipboard fallback chain** — `wl-copy` (Wayland) → `arboard` (X11) → `xclip`

## Manual build & run

```bash
cargo build --release

# terminal 1
./target/release/v2p-daemon --language en

# terminal 2
sudo ./target/release/v2p-listener
```

## Requirements

Runtime:

- Linux (X11 or Wayland) with ALSA
- sudo / root access for the listener (evdev + uinput)
- `wl-clipboard` (Wayland) or `xclip` (X11) recommended for reliable pasting
- GTK3 + libayatana-appindicator3 for the tray icon
  (optional — app works without it; on GNOME the AppIndicator extension must be enabled)

Build:

```bash
sudo apt install build-essential pkg-config cmake \
  libgtk-3-dev libayatana-appindicator3-dev libxdo-dev
cargo build --release
```

## How it works

```
v2p-listener (root)                    v2p-daemon (user)
/dev/input/event* ── Right Ctrl ──▶ UDP :5005 START/STOP ──▶ record / transcribe
uinput Ctrl+V   ◀── UDP :5006 PASTE ◀── clipboard set ◀── whisper-rs
```

## Files

| File | Purpose |
|---|---|
| `daemon/src/main.rs` | User process: audio, STT, clipboard, tray, UDP command receiver |
| `listener/src/main.rs` | Root process: evdev keyboard reader, Ctrl+V injector |
| `start.sh` | Launcher: build check, stale-instance cleanup, sudo handling |
| `Cargo.toml` | Workspace definition |
