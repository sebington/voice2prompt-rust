// Permission checks for evdev (/dev/input) and uinput (/dev/uinput).
// With the shipped udev rule, membership in the `input` group grants access.

use std::fs::OpenOptions;

/// Can we open at least one /dev/input/event* device for reading?
pub fn evdev_readable() -> bool {
    let dir = match std::fs::read_dir("/dev/input") {
        Ok(d) => d,
        Err(_) => return false,
    };
    for entry in dir.flatten() {
        let name = entry.file_name();
        let s = name.to_string_lossy();
        if !s.starts_with("event") {
            continue;
        }
        if OpenOptions::new().read(true).open(entry.path()).is_ok() {
            return true;
        }
    }
    false
}

/// Can we open /dev/uinput for writing (needed for the virtual Ctrl+V device)?
pub fn uinput_writable() -> bool {
    OpenOptions::new()
        .write(true)
        .open("/dev/uinput")
        .is_ok()
}

/// Full access needed to run the listener unprivileged.
pub fn ok() -> bool {
    evdev_readable() && uinput_writable()
}
