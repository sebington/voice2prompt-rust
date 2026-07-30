// v2p-listener — runs as root.
// Reads /dev/input/event* keyboards, monitors Right Ctrl,
// sends START/STOP to daemon, and simulates Ctrl+V on PASTE.

use clap::Parser;
use evdev::{
    raw_stream::RawDevice,
    uinput::VirtualDevice,
    AttributeSet, InputEvent, KeyCode, KeyEvent, SynchronizationCode,
    SynchronizationEvent,
};
use std::net::UdpSocket;
use std::os::fd::AsRawFd;
use std::time::Duration;

const UDP_CMD_PORT: u16 = 5005;
const UDP_PASTE_PORT: u16 = 5006;

#[derive(Parser)]
#[command(name = "v2p-listener", about = "Voice2Prompt keyboard listener (root)")]
struct Args;

fn find_keyboards() -> Vec<String> {
    let mut keyboards = Vec::new();
    let dir = match std::fs::read_dir("/dev/input") {
        Ok(d) => d,
        Err(_) => return keyboards,
    };
    for entry in dir.flatten() {
        let name = entry.file_name();
        let s = name.to_string_lossy();
        if !s.starts_with("event") {
            continue;
        }
        let path = entry.path();
        let dev = match RawDevice::open(&path) {
            Ok(d) => d,
            _ => continue,
        };
        let has_keys = dev.supported_keys().map(|keys| {
            keys.contains(KeyCode::KEY_A)
                || keys.contains(KeyCode::KEY_ENTER)
                || keys.contains(KeyCode::KEY_SPACE)
        }).unwrap_or(false);
        if has_keys {
            keyboards.push(path.to_string_lossy().to_string());
        }
    }
    keyboards
}

fn create_uinput_keyboard() -> Option<VirtualDevice> {
    let keys: AttributeSet<KeyCode> =
        [KeyCode::KEY_LEFTCTRL, KeyCode::KEY_V].into_iter().collect();
    let builder = VirtualDevice::builder().ok()?;
    builder
        .name("v2p-ctrl-v")
        .with_keys(&keys)
        .ok()?
        .build()
        .ok()
}

fn send_ctrl_v(uinput: &mut VirtualDevice) {
    let down = |code| KeyEvent::new(KeyCode(code), 1);
    let up = |code| KeyEvent::new(KeyCode(code), 0);
    let sync = || SynchronizationEvent::new(SynchronizationCode::SYN_REPORT, 0).into();
    let events: Vec<InputEvent> = vec![
        down(KeyCode::KEY_LEFTCTRL.0).into(),
        down(KeyCode::KEY_V.0).into(),
        sync(),
        up(KeyCode::KEY_V.0).into(),
        up(KeyCode::KEY_LEFTCTRL.0).into(),
        sync(),
    ];
    let _ = uinput.emit(&events);
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _args = Args::parse();

    // Find keyboard devices
    let kb_paths = find_keyboards();
    eprintln!("Found {} keyboard device(s)", kb_paths.len());
    for p in &kb_paths {
        eprintln!("  {p}");
    }

    // Open all keyboard devices (non-blocking, raw stream)
    let mut kb_devices: Vec<RawDevice> = Vec::new();
    for p in &kb_paths {
        match RawDevice::open(p) {
            Ok(d) => {
                let fd = d.as_raw_fd();
                let flags = nix::fcntl::fcntl(fd, nix::fcntl::F_GETFL)?;
                let oflags = nix::fcntl::OFlag::from_bits_retain(flags)
                    | nix::fcntl::OFlag::O_NONBLOCK;
                nix::fcntl::fcntl(fd, nix::fcntl::F_SETFL(oflags))?;
                kb_devices.push(d);
            }
            Err(e) => eprintln!("Cannot open {p}: {e}"),
        }
    }

    // Virtual uinput keyboard for Ctrl+V injection
    let mut uinput = create_uinput_keyboard()
        .expect("Cannot create uinput — are you root?");

    // UDP sockets
    let cmd_sock = UdpSocket::bind(format!("127.0.0.1:{UDP_PASTE_PORT}"))
        .map_err(|e| format!("cannot bind UDP :{UDP_PASTE_PORT} ({e}) - is another v2p-listener already running?"))?;
    cmd_sock.set_read_timeout(Some(Duration::from_millis(50)))?;
    let out_sock = UdpSocket::bind("127.0.0.1:0")?;

    let mut pressed = false;
    eprintln!("Listening for Right Ctrl …");
    let mut buf = [0u8; 64];

    loop {
        // Read keyboard events from all devices
        for dev in &mut kb_devices {
            match dev.fetch_events() {
                Ok(mut events) => {
                    while let Some(ev) = events.next() {
                        if ev.event_type().0 != 0x01 { continue; } // EV_KEY only
                        if ev.code() != 97 { continue; }           // KEY_RIGHTCTRL
                        let val = ev.value();
                        if val == 2 { continue; }                   // skip auto-repeat
                        if val == 1 && !pressed {
                            pressed = true;
                            let _ = out_sock
                                .send_to(b"START", ("127.0.0.1", UDP_CMD_PORT));
                        } else if val == 0 && pressed {
                            pressed = false;
                            let _ = out_sock
                                .send_to(b"STOP", ("127.0.0.1", UDP_CMD_PORT));
                        }
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => {
                    if e.kind() != std::io::ErrorKind::Interrupted {
                        eprintln!("evdev error: {e}");
                    }
                }
            }
        }

        // Check for PASTE command from daemon
        loop {
            match cmd_sock.recv_from(&mut buf) {
                Ok((len, _)) => {
                    if String::from_utf8_lossy(&buf[..len]).trim() == "PASTE" {
                        send_ctrl_v(&mut uinput);
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => {
                    eprintln!("UDP error: {e}");
                    break;
                }
            }
        }

        std::thread::sleep(Duration::from_millis(5));
    }
}
