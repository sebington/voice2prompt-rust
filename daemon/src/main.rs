use clap::Parser;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::io::{Read, Write};
use std::net::UdpSocket;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const UDP_CMD_PORT: u16 = 5005;
const UDP_PASTE_PORT: u16 = 5006;
const SAMPLE_RATE: u32 = 16000;
const CHANNELS: u16 = 1;
const MIN_SAMPLES: usize = 1600;
const MODEL_BASE_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

#[derive(Parser)]
#[command(name = "v2p-daemon")]
struct Args {
    #[arg(short, long, default_value = "en")]
    language: String,
}

struct LangCfg {
    model_file: &'static str,
    whisper_lang: &'static str,
}

fn lang_cfg(lang: &str) -> Option<LangCfg> {
    match lang {
        "en" => Some(LangCfg { model_file: "ggml-tiny.en.bin", whisper_lang: "en" }),
        "fr" => Some(LangCfg { model_file: "ggml-tiny.bin", whisper_lang: "fr" }),
        _ => None,
    }
}

// ── Model ──────────────────────────────────────────────────────────────────

fn model_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".local/share/voice2prompt/models")
}

fn download_model(model_file: &str, dest: &std::path::Path) -> Result<(), String> {
    let url = format!("{MODEL_BASE_URL}/{model_file}");
    eprintln!("Downloading {model_file}");

    let resp = ureq::get(&url).call().map_err(|e| format!("HTTP: {e}"))?;
    let total: u64 = resp
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    let mut reader = resp.into_reader();
    let mut file = std::fs::File::create(dest).map_err(|e| format!("create: {e}"))?;

    if total > 0 {
        let mut done: u64 = 0;
        let mut buf = [0u8; 8192];
        loop {
            let n = reader.read(&mut buf).map_err(|e| format!("read: {e}"))?;
            if n == 0 { break; }
            file.write_all(&buf[..n]).map_err(|e| format!("write: {e}"))?;
            done += n as u64;
            eprint!("\r  {}%", done * 100 / total);
        }
        eprintln!();
    } else {
        std::io::copy(&mut reader, &mut file).map_err(|e| format!("copy: {e}"))?;
    }
    Ok(())
}

// ── Tray icon ──────────────────────────────────────────────────────────────

fn make_icon(r: u8, g: u8, b: u8) -> tray_icon::Icon {
    let sz = 64usize;
    let mut px = Vec::with_capacity(sz * sz * 4);
    for _ in 0..sz * sz {
        px.extend([r, g, b, 255]);
    }
    tray_icon::Icon::from_rgba(px, sz as u32, sz as u32).expect("icon")
}

fn spawn_tray_thread() -> std::sync::mpsc::Sender<[u8; 3]> {
    let (tx, rx) = std::sync::mpsc::channel::<[u8; 3]>();

    std::thread::spawn(move || {
        if gtk::init().is_err() {
            return; // no display, skip tray
        }
        let tray = tray_icon::TrayIconBuilder::new()
            .with_tooltip("Voice2Prompt")
            .with_icon(make_icon(0x2e, 0xcc, 0x71))
            .build()
            .expect("tray icon");

        loop {
            while let Ok(rgb) = rx.try_recv() {
                let _ = tray.set_icon(Some(make_icon(rgb[0], rgb[1], rgb[2])));
            }
            gtk::main_iteration_do(false);
            std::thread::sleep(Duration::from_millis(50));
        }
    });
    tx
}

// ── Audio ──────────────────────────────────────────────────────────────────

fn open_audio_stream(
    recording: Arc<AtomicBool>,
    buffer: Arc<Mutex<Vec<i16>>>,
) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| "no input device".to_string())?;
    let config = cpal::StreamConfig {
        channels: CHANNELS,
        sample_rate: cpal::SampleRate(SAMPLE_RATE),
        buffer_size: cpal::BufferSize::Default,
    };
    let stream = device
        .build_input_stream::<i16, _, _>(
            &config,
            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                if recording.load(Ordering::Relaxed) {
                    if let Ok(mut buf) = buffer.lock() {
                        buf.extend_from_slice(data);
                    }
                }
            },
            move |err| eprintln!("Audio error: {err}"),
            None,
        )
        .map_err(|e| format!("audio stream: {e}"))?;
    stream.play().map_err(|e| format!("play: {e}"))?;
    Ok(stream)
}

// ── Clipboard ──────────────────────────────────────────────────────────────

fn copy_clip(text: &str) -> bool {
    // wl-copy first (Wayland)
    if let Ok(mut child) = std::process::Command::new("wl-copy")
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        let _ = child.stdin.take().unwrap().write_all(text.as_bytes());
        let _ = child.wait();
        return true;
    }
    // arboard (X11 / native)
    let ok = std::panic::catch_unwind(|| {
        arboard::Clipboard::new()
            .and_then(|mut cb| cb.set_text(text))
            .is_ok()
    });
    if let Ok(true) = ok {
        return true;
    }
    // xclip fallback
    if let Ok(mut child) = std::process::Command::new("xclip")
        .args(["-selection", "clipboard"])
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        let _ = child.stdin.take().unwrap().write_all(text.as_bytes());
        let _ = child.wait();
        return true;
    }
    eprintln!("Clipboard: all backends failed - install wl-clipboard");
    false
}

// ── PASTE command ──────────────────────────────────────────────────────────

fn send_paste() {
    if let Ok(sock) = UdpSocket::bind("127.0.0.1:0") {
        let _ = sock.send_to(b"PASTE", ("127.0.0.1", UDP_PASTE_PORT));
    }
}

// ── Main ───────────────────────────────────────────────────────────────────

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let cfg = lang_cfg(&args.language)
        .ok_or_else(|| format!("unsupported lang '{}'", args.language))?;

    // Model
    let model_dir = model_dir();
    std::fs::create_dir_all(&model_dir)?;
    let model_path = model_dir.join(cfg.model_file);
    if !model_path.exists() {
        download_model(cfg.model_file, &model_path)?;
    }

    eprintln!("Loading model ...");
    let whisper_ctx = {
        use whisper_rs::{WhisperContext, WhisperContextParameters};
        let ctx = WhisperContext::new_with_params(
            model_path.to_str().unwrap(),
            WhisperContextParameters::default(),
        )?;
        eprintln!("Model loaded");
        Arc::new(ctx)
    };

    // Tray icon (background thread, silently skips if no display)
    let tray_tx = spawn_tray_thread();

    // Shared state
    let recording = Arc::new(AtomicBool::new(false));
    let audio_buf: Arc<Mutex<Vec<i16>>> = Arc::new(Mutex::new(Vec::new()));

    // Audio stream
    let _stream = open_audio_stream(recording.clone(), audio_buf.clone())?;

    // UDP listener
    let sock = UdpSocket::bind(format!("127.0.0.1:{UDP_CMD_PORT}"))?;
    sock.set_read_timeout(Some(Duration::from_millis(50)))?;

    eprintln!("Ready - hold Right Ctrl to record");
    let _ = tray_tx.send([0x2e, 0xcc, 0x71]);

    let mut udp_buf = [0u8; 64];

    loop {
        match sock.recv_from(&mut udp_buf) {
            Ok((len, _)) => {
                let cmd = String::from_utf8_lossy(&udp_buf[..len]);
                match cmd.trim() {
                    "START" => {
                        recording.store(true, Ordering::SeqCst);
                        audio_buf.lock().unwrap().clear();
                        eprint!("\rRecording ... ");
                        let _ = tray_tx.send([0xff, 0x00, 0x00]);
                    }
                    "STOP" => {
                        recording.store(false, Ordering::SeqCst);
                        let _ = tray_tx.send([0xf1, 0xc4, 0x0f]);

                        let samples: Vec<i16> = {
                            let mut buf = audio_buf.lock().unwrap();
                            std::mem::take(&mut *buf)
                        };

                        if samples.len() < MIN_SAMPLES {
                            eprintln!("\rToo short, skipped                ");
                            let _ = tray_tx.send([0x2e, 0xcc, 0x71]);
                            continue;
                        }

                        eprint!("\rTranscribing ...                     ");

                        let f32s: Vec<f32> =
                            samples.iter().map(|&s| s as f32 / 32768.0).collect();

                        let text = {
                            use whisper_rs::{FullParams, SamplingStrategy};
                            let mut params =
                                FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
                            params.set_language(Some(cfg.whisper_lang));
                            params.set_n_threads(4);
                            params.set_print_progress(false);
                            params.set_print_realtime(false);
                            params.set_print_timestamps(false);
                            params.set_no_context(true);
                            params.set_suppress_blank(true);

                            let mut state = whisper_ctx.create_state()?;
                            state.full(params, &f32s)?;

                            let n = state.full_n_segments()?;
                            let parts: Vec<String> = (0..n)
                                .filter_map(|i| {
                                    let s = state.full_get_segment_text(i).ok()?;
                                    let t = s.trim().to_string();
                                    if t.is_empty() { None } else { Some(t) }
                                })
                                .collect();
                            if parts.is_empty() { None } else { Some(parts.join(" ")) }
                        };

                        if let Some(ref t) = text {
                            eprintln!("\rTranscribed: {t}");
                            let paste = format!("{} ", t.trim());
                            if copy_clip(&paste) {
                                std::thread::sleep(Duration::from_millis(150));
                                send_paste();
                            }
                        } else {
                            eprintln!("\rNo speech detected");
                        }

                        let _ = tray_tx.send([0x2e, 0xcc, 0x71]);
                        eprint!("\rReady - hold Right Ctrl to record");
                    }
                    _ => {}
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => {
                eprintln!("UDP error: {e}");
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    }
}
