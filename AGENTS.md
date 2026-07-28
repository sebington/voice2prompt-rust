# Project Context for AI Coding Agents

> This file is for stable, slow-changing information: architecture, conventions, and how to work in this repo.
> For what's happening *right now* (in-progress work, next steps), see `STATE.md` instead.
> Read this file first, every session, before touching code.

## 1. What this project is

Push-to-talk dictation: hold Right Ctrl, speak, release — text is transcribed
via Whisper and pasted into the active application. System-wide, works in any app.

Originally a Python/uv script, ported to Rust as two native binaries.

## 2. Tech stack

- Language(s): Rust (edition 2021)
- Framework(s): none — bare cargo workspace with two binaries
- Database: none
- Key dependencies:
  - `whisper-rs` (0.13) — Whisper.cpp bindings for STT
  - `cpal` (0.15) — audio capture via ALSA
  - `evdev` (0.13, `raw_stream`) — Linux input event reading
  - `tray-icon` (0.19) — system tray indicator (Linux: GTK + libappindicator)
  - `arboard` / `wl-copy` — clipboard
- Build: cargo

## 3. Architecture overview

Split-privilege design. Two processes communicate over UDP on localhost:

```
┌─────────────────────────────────────────────────────────────┐
│ v2p-daemon  (user)                                         │
│                                                             │
│  UDP :5005 ← START/STOP ← listener                         │
│  UDP :5006 → PASTE    → listener                           │
│                                                             │
│  Audio: cpal (16 kHz mono i16)                              │
│  STT:   whisper-rs (tiny.en / tiny model)                   │
│  Tray:  tray-icon (green=ready, red=recording)              │
│  Clip:  wl-copy → arboard → xclip                           │
└─────────────────────────────────────────────────────────────┘
        ▲ UDP ports 5005/5006 on 127.0.0.1
        ▼
┌─────────────────────────────────────────────────────────────┐
│ v2p-listener  (root)                                       │
│                                                             │
│  evdev raw_stream: reads /dev/input/event* keyboards       │
│    → detects KEY_RIGHTCTRL press/release                   │
│    → sends START / STOP to daemon                           │
│                                                             │
│  uinput virtual keyboard: injects Ctrl+V on PASTE           │
└─────────────────────────────────────────────────────────────┘
```

**Key entry points:**
- App starts at: `start.sh` (builds + launches both)
- Daemon: `daemon/src/main.rs`
- Listener: `listener/src/main.rs`

## 4. Conventions

- No tests yet. Manual testing via `./start.sh`.
- Error handling: `Box<dyn std::error::Error>` in main, `Result<(), String>` for helpers.
- Clipboard: prefer `wl-copy` first (Wayland), then `arboard` (X11), then `xclip`.

## 5. How to build, run, and test

```bash
# build
cargo build --release

# run (builds if needed, handles sudo)
./start.sh

# run binaries separately (e.g. for debugging)
./target/release/v2p-daemon --language en   # terminal 1
sudo ./target/release/v2p-listener           # terminal 2
```

System deps:
```bash
sudo apt install build-essential pkg-config cmake \
  libgtk-3-dev libayatana-appindicator3-dev libxdo-dev
```

## 6. Things that are NOT obvious from the code

- **sync_stream swallows events.** The `evdev::Device` (from `sync_stream`) can
  miss key-release events in non-blocking polls. Always use `raw_stream::RawDevice`
  with manual `O_NONBLOCK` via `nix::fcntl` for this use-case.
- **arboard hangs on Wayland.** `arboard::Clipboard::new()` tries X11 first and
  blocks for several seconds if no X server is running. The code checks
  `$WAYLAND_DISPLAY` implicitly by trying `wl-copy` first.
- **Tray icon needs GTK thread.** On Linux, `tray-icon` requires `gtk::init()` +
  a polling loop calling `gtk::main_iteration_do(false)` in a dedicated thread.
  If GTK init fails (no display), the thread exits silently — no tray icon but
  the app still works.
- **whisper.cpp builds from source.** `whisper-rs` invokes cmake during `cargo build`
  to compile whisper.cpp. First build takes ~5 min. Requires cmake + C++ compiler.
- **Mono 16 kHz audio.** `cpal` with ALSA on Linux supports 16 kHz mono i16
  natively. No resampling needed.

## 7. Where to find more detail

- Rust implementation: `daemon/src/main.rs`, `listener/src/main.rs`
- Python original (reference): `voice_daemon_local.py`, `key_listener.py`
- README: basic usage instructions
- Model files auto-download to `~/.local/share/voice2prompt/models/`

---
*Update this file only when something structural or conventional actually changes — not every session. For session-to-session progress, use STATE.md.*
