// v2p — push-to-talk dictation.
// Hold Right Ctrl, speak, release → transcribed and pasted into the active app.

use clap::{Parser, Subcommand};
use cpal::traits::HostTrait;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

mod daemon;
mod listener;
mod perms;

#[derive(Parser)]
#[command(
    name = "v2p",
    version,
    about = "Push-to-talk dictation: hold Right Ctrl, speak, release — text is transcribed and pasted into the active application"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run everything (daemon + keyboard listener) — the normal way to start
    Run {
        #[arg(short, long, default_value = "en")]
        language: String,
    },
    /// Run the daemon only: audio, transcription, clipboard, tray
    Daemon {
        #[arg(short, long, default_value = "en")]
        language: String,
    },
    /// Run the keyboard listener only (needs access to /dev/input and /dev/uinput)
    Listen,
    /// Check permissions and report what's missing
    Doctor,
}

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Run { language } => run(language),
        Cmd::Daemon { language } => {
            let shutdown = Arc::new(AtomicBool::new(false));
            daemon::run(&language, shutdown)
        }
        Cmd::Listen => listener::run(),
        Cmd::Doctor => {
            doctor();
            Ok(())
        }
    }
}

fn run(language: String) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("cannot resolve own binary path: {e}"))?;

    let shutdown = Arc::new(AtomicBool::new(false));
    ctrlc::set_handler({
        let shutdown = shutdown.clone();
        move || {
            shutdown.store(true, Ordering::SeqCst);
        }
    })
    .map_err(|e| format!("ctrlc handler: {e}"))?;

    // Daemon runs on a worker thread; main thread supervises the listener.
    let daemon_done = Arc::new(AtomicBool::new(false));
    let daemon_handle = {
        let shutdown = shutdown.clone();
        let done = daemon_done.clone();
        let lang = language.clone();
        std::thread::spawn(move || {
            let r = daemon::run(&lang, shutdown);
            done.store(true, Ordering::SeqCst);
            r
        })
    };

    let result = if perms::ok() {
        eprintln!("Keyboard access OK — listener runs in-process");
        let _listener_thread = std::thread::spawn(|| {
            if let Err(e) = listener::run() {
                eprintln!("listener error: {e}");
            }
        });
        daemon_handle
            .join()
            .map_err(|_| "daemon thread panicked")?
    } else {
        eprintln!(
            "No keyboard access (evdev/uinput) — starting listener with sudo.\n\
             Run ./install.sh once to grant access and drop the need for sudo."
        );
        let mut child = Command::new("sudo")
            .arg(&exe)
            .arg("listen")
            .spawn()
            .map_err(|e| format!("failed to start 'sudo {exe:?} listen': {e}"))?;

        // Give the sudo listener a grace period; if it exits early
        // (bad password, no tty, permission denied), stop the daemon too
        // rather than running silently without a listener.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if shutdown.load(Ordering::SeqCst)
                || daemon_done.load(Ordering::SeqCst)
                || std::time::Instant::now() >= deadline
            {
                break;
            }
            if let Some(status) = child.try_wait()? {
                eprintln!(
                    "listener exited early ({status}) - daemon stopped. Run ./install.sh once, or start it manually."
                );
                shutdown.store(true, Ordering::SeqCst);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }

        let result: Result<(), Box<dyn std::error::Error + Send + Sync>> =
            daemon_handle.join().map_err(|_| "daemon thread panicked")?;
        let _ = child.kill();
        let _ = child.wait();
        result
    };

    result
}

fn doctor() {
    println!("v2p {}", env!("CARGO_PKG_VERSION"));
    println!();

    // Keyboard devices
    let kbs = listener::find_keyboards();
    println!(
        "{} keyboard devices found: {}",
        if kbs.is_empty() { "[!!]" } else { "[ok]" },
        if kbs.is_empty() {
            "none".to_string()
        } else {
            kbs.join(", ")
        }
    );

    // evdev read access
    let evdev_ok = perms::evdev_readable();
    println!(
        "{} /dev/input read access (Right Ctrl detection)",
        if evdev_ok { "[ok]" } else { "[!!]" }
    );

    // uinput write access
    let uinput_ok = perms::uinput_writable();
    println!(
        "{} /dev/uinput write access (Ctrl+V injection)",
        if uinput_ok { "[ok]" } else { "[!!]" }
    );

    // Audio
    let audio_ok = cpal::default_host()
        .default_input_device()
        .is_some();
    println!(
        "{} audio input device",
        if audio_ok { "[ok]" } else { "[!!]" }
    );

    // Clipboard tools
    let wl = which("wl-copy");
    let xclip = which("xclip");
    println!(
        "{} wl-copy (Wayland clipboard): {}",
        if wl.is_some() { "[ok]" } else { "[--]" },
        wl.as_deref().unwrap_or("not installed")
    );
    println!(
        "{} xclip (X11 fallback): {}",
        if xclip.is_some() { "[ok]" } else { "[--]" },
        xclip.as_deref().unwrap_or("not installed")
    );

    // Display server (tray)
    let display = std::env::var("DISPLAY").ok();
    let wayland = std::env::var("WAYLAND_DISPLAY").ok();
    println!(
        "{} display server for tray (DISPLAY={}, WAYLAND_DISPLAY={})",
        if display.is_some() || wayland.is_some() { "[ok]" } else { "[--]" },
        display.as_deref().unwrap_or("none"),
        wayland.as_deref().unwrap_or("none")
    );

    println!();
    if !evdev_ok || !uinput_ok {
        println!("Run ./install.sh once, then log out and back in, to fix device access.");
    }
    if !audio_ok {
        println!("No input device found — check your microphone / ALSA setup.");
    }
    if !evdev_ok || !uinput_ok || !audio_ok {
        std::process::exit(1);
    }
    println!("All good — ready to run. Start with: v2p run");
}

fn which(cmd: &str) -> Option<String> {
    Command::new("which").arg(cmd).output().ok().map(|o| {
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }).filter(|s| !s.is_empty())
}
