use std::io::Error;
use std::os::raw::c_void;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCode {
    Char(char),
    Up,
    Down,
    Left,
    Right,
    Enter,
    Backspace,
    Escape,
    Unknown,
}

pub enum KeyEvent {
    Key(KeyCode),
    Timeout,
    Eof,
}

const EINTR: i32 = 4;

unsafe extern "C" {
    fn read(fd: i32, buf: *mut c_void, count: usize) -> isize;
}

/// Non-blocking read with a timeout — used so the UI can animate while waiting.
pub fn read_key_timeout(timeout_ms: i32) -> KeyEvent {
    if !crate::term::stdin_readable(timeout_ms) {
        return KeyEvent::Timeout;
    }
    match read_key() {
        Some(k) => KeyEvent::Key(k),
        None => KeyEvent::Eof,
    }
}

pub fn read_key() -> Option<KeyCode> {
    let b1 = read_one()?;
    match b1 {
        0x1b => read_escape(),
        b'\r' | b'\n' => Some(KeyCode::Enter),
        0x7f | 0x08 => Some(KeyCode::Backspace),
        c if c >= 0x20 && c < 0x7f => Some(KeyCode::Char(c as char)),
        _ => Some(KeyCode::Unknown),
    }
}

/// Distinguish a bare Esc from an escape sequence. Arrow keys arrive either as
/// CSI (`ESC [ A/B/C/D`) or — in "application cursor keys" mode — as SS3
/// (`ESC O A/B/C/D`); a bare Esc has nothing after it.
fn read_escape() -> Option<KeyCode> {
    if !crate::term::stdin_readable(100) {
        return Some(KeyCode::Escape);
    }
    let b2 = read_one()?;
    match b2 {
        b'[' | b'O' => {
            let b3 = read_one()?;
            Some(match b3 {
                b'A' => KeyCode::Up,
                b'B' => KeyCode::Down,
                b'C' => KeyCode::Right,
                b'D' => KeyCode::Left,
                _ => KeyCode::Unknown,
            })
        }
        _ => Some(KeyCode::Unknown),
    }
}

fn read_one() -> Option<u8> {
    // Unbuffered read straight from fd 0. Reading through std::io::stdin()
    // would buffer the whole `ESC [ B` sequence and hide the remaining bytes
    // from `poll`, making single arrow presses look like a bare Esc.
    let mut b = [0u8; 1];
    loop {
        let n = unsafe { read(0, b.as_mut_ptr() as *mut c_void, 1) };
        if n == 1 {
            return Some(b[0]);
        }
        if n == 0 {
            return None;
        }
        if Error::last_os_error().raw_os_error() == Some(EINTR) {
            continue;
        }
        return None;
    }
}
