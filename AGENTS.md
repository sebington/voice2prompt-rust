# Project Context for AI Coding Agents

> This file is for stable, slow-changing information: architecture, conventions, and how to work in this repo.
> For what's happening *right now* (in-progress work, next steps), see `STATE.md` instead.
> Read this file first, every session, before touching code.

## 1. What this project is

Push-to-talk dictation: hold Right Ctrl, speak, release — text is transcribed
via Whisper and pasted into the active application. System-wide, works in any app.

Single native Rust binary (`v2p`) with subcommands. Originally two separate
binaries (daemon + listener); merged into one so it can be shipped as a single
artifact.

## 2. Tech stack

- Language: Rust (edition 2021)
- Framework(s): none — plain cargo binary crate
- Key dependencies:
  - `whisper-rs` (0.13) — Whisper.cpp bindings for STT
  - `cpal` (0.15) — audio capture via ALSA
  - `evdev` (0.13, `raw_stream`) — Linux input event reading
  - `tray-icon` (0.19) + `gtk` (0.18) — system tray indicator (Linux: GTK + libappindicator)
  - `arboard` / `wl-copy` / `xclip` — clipboard fallback chain
  - `clap` (4, derive) — CLI subcommands
  - `ureq` (2) — HTTPS model download from huggingface.co
  - `nix` (0.29, fs) — `O_NONBLOCK` on evdev fds
  - `ctrlc` (3) — graceful shutdown
- Build: cargo; `.deb` packaging via `cargo-deb` ([package.metadata.deb] in
  Cargo.toml + `scripts/postinst`)

## 3. Architecture overview

One binary, four subcommands:

```
v2p run      ← normal entry point: daemon + listener, supervises both
v2p daemon   ← audio / Whisper STT / clipboard / tray (user space)
v2p listen   ← evdev Right-Ctrl detection + uinput Ctrl+V injection
v2p doctor   ← permission & environment diagnostics
```

Privilege handling in `v2p run`:

- If the user has device access (see §6), the listener runs **in-process as a
  thread** — single process, no root.
- Otherwise `v2p run` re-execs `sudo v2p listen` as a child process and monitors
  it: if the listener dies within a 10s grace period, the daemon shuts down too
  (no silently dead app).

The two halves still communicate over UDP on localhost (ports 5005/5006):

```
┌────────────────────────── v2p run ──────────────────────────┐
│  daemon (main/thread)           listener (thread or child)  │
│                                                             │
│  Audio: cpal (16 kHz mono i16)  evdev: /dev/input/event*    │
│  STT:   whisper-rs (tiny.en/tiny)│ detects KEY_RIGHTCTRL     │
│  Tray:  tray-icon (green/red/   │ → START/STOP → daemon      │
│         yellow, tooltip, menu)  │                            │
│  Clip:  wl-copy → arboard →     │ uinput: injects Ctrl+V     │
│         xclip                    │ ← PASTE ← daemon           │
└─────────────────────────────────────────────────────────────┘
        UDP 5005 (START/STOP) and 5006 (PASTE) on 127.0.0.1
```

**Key entry points:**
- App starts at: `start.sh` (builds, kills stale instances, `exec v2p run`)
- CLI: `src/main.rs` (subcommand dispatch, `run` orchestration, `doctor`)
- Daemon logic: `src/daemon.rs`
- Listener logic: `src/listener.rs`
- Device permissions: `src/perms.rs`, `packaging/99-voice2prompt.rules`

## 4. Conventions

- No tests yet. Manual testing via `./start.sh` or `v2p doctor`.
- Error handling: `Result<(), Box<dyn std::error::Error + Send + Sync>>` in main,
  `Result<(), String>` for helpers.
- Clipboard: prefer `wl-copy` first (Wayland), then `arboard` (X11), then `xclip`.
- Tray state is pushed over an mpsc channel as `(color, tooltip)` pairs;
  quitting from the tray menu sets the shared `shutdown` flag (never
  `std::process::exit` — the `run` supervisor must clean up the listener child).

## 5. How to build, run, and test

```bash
# build
cargo build --release

# one-time install (udev rule + input group + binary to /usr/local/bin;
# no sudo needed after)
./install.sh          # then log out & back in

# build a .deb package (installs same pieces via dpkg + scripts/postinst)
cargo install cargo-deb && cargo deb

# run (builds if needed)
./start.sh            # or: target/release/v2p run --language en

# manual / debugging
./target/release/v2p daemon --language en   # audio/STT/tray only
./target/release/v2p listen                 # privileged part only
./target/release/v2p doctor                 # permission diagnostics
```

System deps (build):
```bash
sudo apt install build-essential pkg-config cmake \
  libgtk-3-dev libayatana-appindicator3-dev libxdo-dev
```

## 6. Things that are NOT obvious from the code

- **Device access without root:** `install.sh` installs
  `packaging/99-voice2prompt.rules` (grants the `input` group read on
  `/dev/input/event*` and write on `/dev/uinput`, plus `uaccess` tag for
  systemd logind), adds the user to the `input` group, and installs the
  binary to `/usr/local/bin`. The user must re-login for group membership.
  The `.deb` (`cargo deb`) does the same via `scripts/postinst`.
  `v2p run` falls back to `sudo v2p listen` when access is missing.
- **`v2p run` supervision:** the daemon runs on a worker thread; the main thread
  supervises the listener child (grace-period startup check, kill on exit).
  Ctrl+C is caught via `ctrlc` → sets `shutdown` → clean teardown.
- **sync_stream swallows events.** The `evdev::Device` (from `sync_stream`) can
  miss key-release events in non-blocking polls. Always use `raw_stream::RawDevice`
  with manual `O_NONBLOCK` via `nix::fcntl` for this use-case.
- **arboard hangs on Wayland.** `arboard::Clipboard::new()` tries X11 first and
  blocks for several seconds if no X server is running. The code checks
  `$WAYLAND_DISPLAY` implicitly by trying `wl-copy` first.
- **Tray icon needs GTK thread.** On Linux, `tray-icon` requires `gtk::init()` +
  a polling loop calling `gtk::main_iteration_do(false)` in a dedicated thread.
  If GTK init fails (no display), the thread exits silently — no tray icon but
  the app still works. Only that thread touches GTK.
- **whisper.cpp builds from source.** `whisper-rs` invokes cmake during `cargo build`
  to compile whisper.cpp. First build takes ~5 min. Requires cmake + C++ compiler.
- **Mono 16 kHz audio.** `cpal` with ALSA on Linux supports 16 kHz mono i16
  natively. No resampling needed.
- **Terminal output.** `Transcribed: …` → the injected paste may land in the
  same terminal (it's the active app) → `Pasted.` → `Ready`. The middle copy is
  the real paste, not a log line.

## 7. Where to find more detail

- CLI: `src/main.rs`
- Daemon: `src/daemon.rs`
- Listener: `src/listener.rs`
- Packaging: `install.sh`, `packaging/99-voice2prompt.rules`,
  `scripts/postinst` + `[package.metadata.deb]` in Cargo.toml (`cargo deb`)
- README: basic usage instructions
- Model files auto-download to `~/.local/share/voice2prompt/models/`

---
*Update this file only when something structural or conventional actually changes — not every session. For session-to-session progress, use STATE.md.*
