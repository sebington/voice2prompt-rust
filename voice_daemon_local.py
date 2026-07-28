#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.10"
# dependencies = [
#     "numpy",
#     "pywhispercpp",
#     "pystray",
#     "pillow",
#     "pyperclip",
#     "sounddevice",
# ]
# ///

"""
Voice Prompt Daemon (User Component)
Listens for audio commands via UDP (from key_listener), records, and transcribes.
"""

import socket
import select
import queue
import subprocess
import os
import sys
import tempfile
import threading
import time
import io
import wave
import argparse
from pathlib import Path
from pywhispercpp.model import Model
import pystray
import pyperclip
import sounddevice as sd
import numpy as np
from PIL import Image, ImageDraw

# Configuration
UDP_IP = "127.0.0.1"
UDP_PORT = 5005
CMD_PORT = 5006  # Port to send PASTE commands to key_listener
CHANNELS = 1
RATE = 16000  # whisper works best with 16kHz
FORMAT = "S16_LE"  # 16-bit little endian
ARECORD_CMD = ["arecord"]
WHISPER_PATH = Path(__file__).parent / "whisper.cpp"

# Language-to-model mapping
LANGUAGE_CONFIG = {
    'en': {
        'model_name': 'tiny.en',
        'model_file': 'ggml-tiny.en.bin',
        'whisper_lang': 'en'
    },
    'fr': {
        'model_name': 'tiny',
        'model_file': 'ggml-tiny.bin',
        'whisper_lang': 'fr'
    }
}

# These will be set based on command-line argument
WHISPER_MODEL_NAME = None
WHISPER_MODEL = None
WHISPER_LANGUAGE = None

audio_q = queue.Queue()
listening = False
recording_data = []
recording_thread = None
whisper_model = None

# System tray variables
tray_icon = None
icon_thread = None

def create_image(color):
    """Generate an image with a colored square for system tray"""
    width = 64
    height = 64

    image = Image.new('RGBA', (width, height), (0, 0, 0, 0))
    
    # If color is None, return just the transparent background
    if color is None:
        return image
        
    dc = ImageDraw.Draw(image)
    
    # Draw a square (same size/location for all colors)
    dc.rectangle((0, 0, 64, 64), fill=color)

    return image

def update_tray_icon(color):
    """Update the system tray icon color"""
    global tray_icon
    if tray_icon:
        tray_icon.icon = create_image(color)

def run_tray_icon():
    """Run the system tray icon in a separate thread"""
    global tray_icon
    tray_icon = pystray.Icon("voice_daemon", create_image("#2ECC71"), "Voice Daemon")
    tray_icon.run()

def simulate_ctrl_v():
    """Send PASTE command to the root key_listener"""
    try:
        sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        sock.sendto(b"PASTE", (UDP_IP, CMD_PORT))
        print("Sent PASTE command to key_listener")
    except Exception as e:
        print(f"Failed to send PASTE command: {e}")

def on_start_command():
    """Start recording command received"""
    global listening, recording_data
    if not listening:
        listening = True
        recording_data = [] # Reset recording data
        print("Recording...")
        update_tray_icon("#FF0000")  # Red

def on_stop_command():
    """Stop recording command received"""
    global listening
    if listening:
        listening = False
        print("Processing...")
        update_tray_icon("#2ECC71")  # Green

def audio_callback(indata, frames, time, status):
    """Callback for sounddevice input stream"""
    if status:
        print(f"Audio status: {status}")
    
    if listening:
        recording_data.append(indata.copy())
        if len(recording_data) % 10 == 0:
            print(".", end="", flush=True)

def normalize_prompt(text):
    """Format text as a prompt"""
    return text.strip()

def numpy_to_wav_bytes(audio_data, sample_rate):
    """Convert numpy array to WAV format in memory"""
    byte_io = io.BytesIO()
    with wave.open(byte_io, 'wb') as wav_file:
        wav_file.setnchannels(CHANNELS)
        wav_file.setsampwidth(2)  # 16-bit
        wav_file.setframerate(sample_rate)
        wav_file.writeframes(audio_data.tobytes())
    byte_io.seek(0)
    return byte_io

def transcribe_audio(audio_data, sample_rate):
    """Transcribe audio using pywhispercpp (in-memory)"""
    try:
        # Check if audio data is sufficient
        if len(audio_data) < RATE // 10:  # Less than 0.1 seconds
            return None
        
        # Create temporary file for pywhispercpp (it requires a file path)
        # But we'll use a fast in-memory approach with a minimal temp file
        with tempfile.NamedTemporaryFile(suffix='.wav', delete=False) as tmp_file:
            tmp_path = tmp_file.name
            # Write WAV header and data
            with wave.open(tmp_file, 'wb') as wav_file:
                wav_file.setnchannels(CHANNELS)
                wav_file.setsampwidth(2)  # 16-bit
                wav_file.setframerate(sample_rate)
                wav_file.writeframes(audio_data.tobytes())
        
        try:
            # Use pywhispercpp to transcribe
            segments = whisper_model.transcribe(tmp_path, language=WHISPER_LANGUAGE)
            
            # Extract text from all segments
            if segments and len(segments) > 0:
                transcription_parts = []
                for segment in segments:
                    if hasattr(segment, 'text') and segment.text.strip():
                        transcription_parts.append(segment.text.strip())
                
                transcription = ' '.join(transcription_parts) if transcription_parts else None
                
                return transcription
            else:
                return None
        finally:
            # Clean up temp file immediately
            try:
                os.unlink(tmp_path)
            except:
                pass
            
    except Exception as e:
        print(f"Error during transcription: {e}")
        return None

def main():
    global listening, recording_data, whisper_model, icon_thread
    global WHISPER_MODEL_NAME, WHISPER_MODEL, WHISPER_LANGUAGE
    
    # Parse command-line arguments
    parser = argparse.ArgumentParser(description='Voice to Prompt Daemon')
    parser.add_argument('--language', '-l', 
                        choices=['en', 'fr'],
                        default='fr',
                        help='Language for transcription (en=English/tiny.en, fr=French/tiny)')
    args = parser.parse_args()
    
    # Configure model based on language
    lang_config = LANGUAGE_CONFIG[args.language]
    WHISPER_MODEL_NAME = lang_config['model_name']
    WHISPER_MODEL = WHISPER_PATH / "models" / lang_config['model_file']
    WHISPER_LANGUAGE = lang_config['whisper_lang']
    
    print(f"Language: {args.language.upper()} | Model: {WHISPER_MODEL_NAME}")
    
    # Check if running as root - WE SHOULD NOT BE ROOT
    if os.geteuid() == 0:
        print("Warning: Run as regular user, not root")

    # Start the system tray icon in a separate thread
    icon_thread = threading.Thread(target=run_tray_icon, daemon=True)
    icon_thread.start()
    time.sleep(0.5)

    # Initialize pywhispercpp
    try:
        if WHISPER_MODEL.exists():
            # Redirect stderr to suppress whisper.cpp initialization logs
            with open(os.devnull, 'w') as devnull:
                old_stderr = sys.stderr
                sys.stderr = devnull
                try:
                    whisper_model = Model(str(WHISPER_MODEL))
                finally:
                    sys.stderr = old_stderr
            print("Model loaded")
        else:
            # Auto-download model if not found locally
            print("Downloading model...")
            whisper_model = Model(WHISPER_MODEL_NAME)
            print("Model loaded")
        
    except Exception as e:
        print(f"Error: {e}")
        print("Check internet connection or model path")
        sys.exit(1)

    # Setup UDP listener
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.bind((UDP_IP, UDP_PORT))
    sock.setblocking(False)
    
    print("Voice Prompt Daemon Ready")
    print("   Hold Right Ctrl to record, release to transcribe & paste")

    # Open audio stream
    # Try to use default device, blocksize=1024 for stability
    try:
        with sd.InputStream(samplerate=RATE, channels=CHANNELS, dtype='int16', 
                            blocksize=1024, callback=audio_callback):
            print("Audio ready")
            
            while True:
                try:
                    # Check for UDP commands
                    ready = select.select([sock], [], [], 0.05)
                    if ready[0]:
                        data, addr = sock.recvfrom(1024)
                        cmd = data.decode().strip()
                        if cmd == "START":
                            on_start_command()
                        elif cmd == "STOP":
                            on_stop_command()

                    # Process recording if stopped
                    if not listening and len(recording_data) > 0:
                        # Concatenate all recorded chunks
                        my_recording = np.concatenate(recording_data, axis=0)
                        
                        # Reset buffer
                        recording_data = []
                        
                        # Transcribe directly from memory (no file I/O)
                        transcription = transcribe_audio(my_recording, RATE)
                        
                        if transcription:
                            prompt = normalize_prompt(transcription)
                            print(f"Transcribed: {transcription}")
                            if prompt:
                                # Try to copy to clipboard
                                clipboard_ok = False
                                try:
                                    prompt_with_space = prompt + " "
                                    pyperclip.copy(prompt_with_space)
                                    clipboard_ok = True
                                except Exception as e:
                                    print(f"pyperclip failed: {e}")
                                    # Try wl-copy for Wayland
                                    try:
                                        subprocess.run(["wl-copy"], input=(prompt + " ").encode("utf-8"), check=True)
                                        clipboard_ok = True
                                    except FileNotFoundError:
                                        print("wl-copy not found. Install: sudo apt install wl-clipboard")
                                    except Exception as e2:
                                        print(f"wl-copy failed: {e2}")
                                        # Try xclip for X11
                                        try:
                                            subprocess.run(["xclip", "-selection", "clipboard"], 
                                                         input=(prompt + " ").encode("utf-8"), check=True)
                                            clipboard_ok = True
                                        except FileNotFoundError:
                                            print("xclip not found. Install: sudo apt install xclip")
                                        except Exception as e3:
                                            print(f"xclip failed: {e3}")
                                
                                if clipboard_ok:
                                    time.sleep(0.1)
                                    simulate_ctrl_v()
                                else:
                                    print("ERROR: No working clipboard tool found!")
                                    print("Install one of: wl-clipboard (Wayland) or xclip (X11)")
                            
                    time.sleep(0.01)
                    
                except KeyboardInterrupt:
                    print("\nShutting down...")
                    break
                except Exception as e:
                    print(f"Error: {e}")
                    time.sleep(1)
    except Exception as e:
         print(f"Audio error: {e}")
         print("Check microphone settings")
    
    # Stop the system tray icon
    if tray_icon:
        tray_icon.stop()
    
    # Wait for the icon thread to finish
    if icon_thread and icon_thread.is_alive():
        icon_thread.join(timeout=1.0)

if __name__ == "__main__":
    main()
