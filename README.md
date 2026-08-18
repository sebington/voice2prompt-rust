# voice2prompt

Push-to-talk dictation for Linux: hold **Right Ctrl**, speak, release — speech is
transcribed locally with Whisper and pasted into the active application.
System-wide, works in any app.

One binary (`v2p`) with subcommands:

| Command | Purpose |
|---|---|
| `v2p run` | Normal entry point: daemon + keyboard listener together |
| `v2p daemon` | Audio, transcription, clipboard, tray (user space) |
| `v2p listen` | Right Ctrl detection (evdev) + Ctrl+V injection (uinput) |
| `v2p doctor` | Permission & environment diagnostics |

## Quick start

```bash
# one-time setup: installs v2p to /usr/local/bin and grants device
# access (udev rule + input group) so no sudo is needed later
./install.sh        # then log out and back in

# run
./start.sh
```

Hold Right Ctrl to dictate, release to transcribe and paste. Ctrl+C stops
everything. The tray icon shows green = ready, red = recording, yellow =
transcribing.

## Features

- **Local transcription** — Whisper.cpp (`whisper-rs`), no network, no API key
- **No root at runtime** — udev rule + `input` group (installed once by
  `install.sh`); falls back to `sudo v2p listen` if access is missing
- **System tray indicator** — green/red/yellow states, hover tooltip, Quit menu
- **Terminal feedback** — `Recording…` → `Transcribed: …` → `Pasted.`
- **Languages** — English (`--language en`, default) and French (`--language fr`)
- **Model auto-download** — Whisper model fetched on first run with progress to
  `~/.local/share/voice2prompt/models/` (`tiny.en` for English, multilingual
  `tiny` for French)
- **Clipboard fallback chain** — `wl-copy` (Wayland) → `arboard` (X11) → `xclip`

## Manual run

```bash
cargo build --release
./target/release/v2p run --language en
./target/release/v2p doctor    # check setup
```

## Requirements

Runtime:

- Linux (X11 or Wayland) with ALSA
- Device access for the listener: after `./install.sh` + re-login this is a
  non-root `input`-group member (evdev + uinput); otherwise the listener needs root
- `wl-clipboard` (Wayland) or `xclip` (X11) recommended for reliable pasting
- GTK3 + libayatana-appindicator3 for the tray icon
  (optional — app works without it; on GNOME the AppIndicator extension must be enabled)

Build:

```bash
sudo apt install build-essential pkg-config cmake \
  libgtk-3-dev libayatana-appindicator3-dev libxdo-dev
cargo build --release
```

## Debian package

```bash
cargo install cargo-deb
cargo deb            # builds target/debian/v2p_<version>_amd64.deb
```

The `.deb` installs the binary to `/usr/local/bin`, the udev rule to
`/etc/udev/rules.d/`, and a postinst script reloads udev and adds the
installing user to the `input` group (re-login still required).

## How it works

```
v2p run
├─ daemon (in-process)      ── UDP :5005 START/STOP ──►  listener
│   audio (cpal) ─► whisper-rs ─► clipboard ─► paste cmd ──►  uinput Ctrl+V
└─ listener (thread, or sudo child if no device access)
```

The daemon captures 16 kHz mono audio while Right Ctrl is held, transcribes
with the Whisper tiny model, copies the result, and asks the listener to inject
Ctrl+V into the focused application.

## Files

| File | Purpose |
|---|---|
| `src/main.rs` | CLI, `run` supervisor, `doctor` |
| `src/daemon.rs` | Audio, STT, clipboard, tray |
| `src/listener.rs` | evdev keyboard reader, Ctrl+V injector |
| `src/perms.rs` | Device permission checks |
| `packaging/99-voice2prompt.rules` | udev rule for non-root device access |
| `scripts/postinst` | .deb postinstall (udev reload, `input` group) |
| `install.sh` | One-time setup (udev rule + `input` group + install to `/usr/local/bin`) |
| `start.sh` | Launcher (build check, stale-instance cleanup) |
