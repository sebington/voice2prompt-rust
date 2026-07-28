#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.10"
# dependencies = [
#     "keyboard",
# ]
# ///

"""
Global Keyboard Listener (Root Component)
Listens for Right Ctrl key (Scan Code 97) and sends signals to the main daemon via UDP.
Also listens for "PASTE" commands from the daemon to execute as root.
"""

import socket
import keyboard
import time
import sys
import os
import threading
import select

# Configuration
UDP_IP = "127.0.0.1"
UDP_PORT = 5005   # Send START/STOP to this port
CMD_PORT = 5006   # Listen for PASTE on this port
RIGHT_CTRL_SCANCODE = 97

def main():
    if os.geteuid() != 0:
        print("Error: key_listener must run as sudo")
        sys.exit(1)

    print("Key Listener listening for Right Ctrl...")

    # Socket for sending START/STOP
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)

    def send_cmd(cmd):
        try:
            sock.sendto(cmd.encode(), (UDP_IP, UDP_PORT))
        except Exception as e:
            print(f"Error: {e}")

    # State to handle auto-repeat
    is_pressed = False

    def on_press(e):
        nonlocal is_pressed
        if not is_pressed:
            is_pressed = True
            send_cmd("START")

    def on_release(e):
        nonlocal is_pressed
        if is_pressed:
            is_pressed = False
            send_cmd("STOP")

    # Listen specifically for scan code 97 (Right Ctrl) to avoid ambiguity
    keyboard.on_press_key(RIGHT_CTRL_SCANCODE, on_press)
    keyboard.on_release_key(RIGHT_CTRL_SCANCODE, on_release)

    # Socket for receiving PASTE commands
    cmd_sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    cmd_sock.bind((UDP_IP, CMD_PORT))
    
    def command_listener():
        while True:
            try:
                ready = select.select([cmd_sock], [], [], 0.1)
                if ready[0]:
                    data, _ = cmd_sock.recvfrom(1024)
                    cmd = data.decode().strip()
                    if cmd == "PASTE":
                        keyboard.send('ctrl+v')
            except Exception as e:
                print(f"Error: {e}")
                time.sleep(1)

    # Start listener thread
    t = threading.Thread(target=command_listener, daemon=True)
    t.start()

    try:
        while True:
            time.sleep(1)
    except KeyboardInterrupt:
        pass

if __name__ == "__main__":
    main()
