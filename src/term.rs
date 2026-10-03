use std::mem;
use std::os::raw::c_void;

const TCSANOW: i32 = 0;
const VMIN: usize = 6;

// ioctl request for the terminal window size.
#[cfg(target_os = "macos")]
pub const TIOCGWINSZ: i64 = 0x4008_7468;
#[cfg(target_os = "linux")]
pub const TIOCGWINSZ: i64 = 0x5413;

// Number of control characters (NCCS): Darwin 20, glibc/Linux 32.
#[cfg(target_os = "macos")]
const NCCS: usize = 20;
#[cfg(target_os = "linux")]
const NCCS: usize = 32;

// `poll`'s nfds_t: Darwin is `unsigned int`, Linux is `unsigned long`.
#[cfg(target_os = "macos")]
type Nfds = u32;
#[cfg(target_os = "linux")]
type Nfds = u64;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Termios {
    pub c_iflag: u32,
    pub c_oflag: u32,
    pub c_cflag: u32,
    pub c_lflag: u32,
    pub c_line: u8,
    pub c_cc: [u8; NCCS],
    pub c_ispeed: u32,
    pub c_ospeed: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Winsize {
    pub ws_row: u16,
    pub ws_col: u16,
    pub ws_xpixel: u16,
    pub ws_ypixel: u16,
}

#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

const POLLIN: i16 = 0x0001;

unsafe extern "C" {
    fn tcgetattr(fd: i32, termios: *mut Termios) -> i32;
    fn tcsetattr(fd: i32, actions: i32, termios: *const Termios) -> i32;
    fn cfmakeraw(termios: *mut Termios);
    // Variadic, as in C: on Apple Silicon variadic arguments are passed on the
    // stack, so declaring it non-variadic makes the call read garbage.
    fn ioctl(fd: i32, request: i64, ...) -> i32;
    fn poll(fds: *mut PollFd, nfds: Nfds, timeout: i32) -> i32;
    fn read(fd: i32, buf: *mut c_void, count: usize) -> isize;
}

pub fn set_raw_mode(fd: i32) -> Option<Termios> {
    let mut orig: Termios = unsafe { mem::zeroed() };
    if unsafe { tcgetattr(fd, &mut orig) } != 0 {
        return None;
    }
    let mut raw = orig;
    unsafe { cfmakeraw(&mut raw) };
    raw.c_cc[VMIN] = 1;
    if unsafe { tcsetattr(fd, TCSANOW, &raw) } != 0 {
        return None;
    }
    Some(orig)
}

pub fn restore_termios(fd: i32, t: &Termios) {
    let _ = unsafe { tcsetattr(fd, TCSANOW, t) };
}

pub fn terminal_size() -> (u16, u16) {
    let mut ws: Winsize = unsafe { mem::zeroed() };
    let ret = unsafe { ioctl(0, TIOCGWINSZ, &mut ws as *mut Winsize) };
    if ret == 0 && ws.ws_row > 0 && ws.ws_col > 0 {
        return (ws.ws_row, ws.ws_col);
    }
    // Fallback to the shell-provided values, then to a sane default.
    let rows = std::env::var("LINES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(24);
    let cols = std::env::var("COLUMNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(80);
    (rows, cols)
}

/// Whether stdin (fd 0) has data available within `timeout_ms`.
pub fn stdin_readable(timeout_ms: i32) -> bool {
    let mut pfd = PollFd {
        fd: 0,
        events: POLLIN,
        revents: 0,
    };
    let r = unsafe { poll(&mut pfd, 1, timeout_ms) };
    r > 0
}

/// Must be called AFTER set_raw_mode.
/// Returns how many physical cells one ASCII character occupies (1 or 2).
pub fn detect_cell_width() -> u32 {
    use std::io::Write;
    let (_, cols) = terminal_size();
    if cols == 0 {
        return 1;
    }
    let mut out = std::io::stdout();
    let _ = out.write_all(b"\x1b[2J\x1b[H");
    let _ = out.flush();
    for _ in 0..(cols as usize) {
        let _ = out.write_all(b" ");
    }
    let _ = out.write_all(b"\x1b[6n");
    let _ = out.flush();
    let mut buf = [0u8; 32];
    let n = if stdin_readable(150) {
        let r = unsafe { read(0, buf.as_mut_ptr() as *mut c_void, buf.len()) };
        if r > 0 { r as usize } else { 0 }
    } else {
        0
    };
    let _ = out.write_all(b"\x1b[2J\x1b[H");
    let _ = out.flush();
    if n >= 4 {
        let s = String::from_utf8_lossy(&buf[..n.min(32)]);
        if let Some(body) = s.strip_prefix("\x1b[") {
            if let Some(rp) = body.find('R') {
                let row: u16 = body[..rp]
                    .split(';')
                    .next()
                    .and_then(|x| x.parse().ok())
                    .unwrap_or(1);
                if row >= 3 {
                    return 2;
                }
            }
        }
    }
    1
}
