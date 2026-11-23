//! The few things the companion needs from the system that std does not
//! give: SIGINT / SIGTERM / SIGHUP noticed (a flag the main loop reads),
//! a signal to a process group, random bytes, the terminal's key-at-a-time
//! mode (through `stty`), and the local clock. No crate: two libc calls
//! on unix, one on Windows.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

/// Set by SIGINT, SIGTERM or SIGHUP: the companion stops.
pub static STOP: AtomicBool = AtomicBool::new(false);

pub const SIGTERM: i32 = 15;
pub const SIGKILL: i32 = 9;

#[cfg(unix)]
mod unix {
    use super::{Ordering, STOP};

    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
        fn signal(sig: i32, handler: usize) -> usize;
    }

    extern "C" fn on_signal(_: i32) {
        STOP.store(true, Ordering::SeqCst);
    }

    pub fn trap() {
        let handler = on_signal as extern "C" fn(i32) as usize;
        // SIGHUP (the terminal closed), SIGINT, SIGTERM: the same on macOS and Linux.
        for sig in [1, 2, 15] {
            // SAFETY: the handler only stores to an atomic, which is async-signal-safe.
            unsafe { signal(sig, handler) };
        }
    }

    pub fn kill_group(pid: u32, sig: i32) {
        if pid > 1 {
            // SAFETY: a plain syscall; a negative pid names the process group.
            unsafe { kill(-(pid as i32), sig) };
        }
    }
}

/// Notice SIGINT, SIGTERM and SIGHUP (`STOP`) instead of dying at once,
/// so every command and runner started is stopped first.
pub fn trap() {
    #[cfg(unix)]
    unix::trap();
}

/// A signal to the process group `pid` leads (each command starts its own).
pub fn kill_group(pid: u32, sig: i32) {
    #[cfg(unix)]
    unix::kill_group(pid, sig);
    #[cfg(windows)]
    {
        let _ = sig;
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

/// A signal's name (`SIGTERM`), as the Node companion reports it.
pub fn signal_name(n: i32) -> String {
    match n {
        1 => "SIGHUP".into(),
        2 => "SIGINT".into(),
        3 => "SIGQUIT".into(),
        6 => "SIGABRT".into(),
        9 => "SIGKILL".into(),
        13 => "SIGPIPE".into(),
        14 => "SIGALRM".into(),
        15 => "SIGTERM".into(),
        n => format!("SIG{n}"),
    }
}

/// `n` random bytes from the system.
pub fn random(n: usize) -> Vec<u8> {
    let mut buf = vec![0u8; n];
    #[cfg(unix)]
    {
        use std::io::Read;
        let ok = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut buf)).is_ok();
        assert!(ok, "no /dev/urandom: the companion cannot make a pairing code");
    }
    #[cfg(windows)]
    {
        #[link(name = "bcrypt")]
        unsafe extern "system" {
            fn BCryptGenRandom(alg: *mut std::ffi::c_void, buf: *mut u8, len: u32, flags: u32) -> i32;
        }
        // SAFETY: the buffer is ours and its length is passed; 2 = BCRYPT_USE_SYSTEM_PREFERRED_RNG.
        let status = unsafe { BCryptGenRandom(std::ptr::null_mut(), buf.as_mut_ptr(), buf.len() as u32, 2) };
        assert!(status == 0, "BCryptGenRandom failed: the companion cannot make a pairing code");
    }
    buf
}

/// Unix seconds now.
pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// The local clock's offset from UTC, seconds (asked of `date` once).
fn offset() -> i64 {
    static OFFSET: OnceLock<i64> = OnceLock::new();
    *OFFSET.get_or_init(|| {
        let out = std::process::Command::new("date").arg("+%z").output().ok();
        let z = out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
        let (sign, digits) = match z.as_bytes().first() {
            Some(b'-') => (-1, &z[1..]),
            Some(b'+') => (1, &z[1..]),
            _ => return 0,
        };
        let h: i64 = digits.get(..2).and_then(|s| s.parse().ok()).unwrap_or(0);
        let m: i64 = digits.get(2..4).and_then(|s| s.parse().ok()).unwrap_or(0);
        sign * (h * 3600 + m * 60)
    })
}

/// `HH:MM:SS` of unix seconds `t`, local time.
pub fn clock(t: i64) -> String {
    let s = (t + offset()).rem_euclid(86_400);
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

/// The terminal in key-at-a-time mode while it lives (no line buffering,
/// no echo, a read returns after 0.1 s with nothing): so Esc is one key.
/// Put back as it was when dropped. Unix only (`stty`); elsewhere a no-op.
pub struct Keys {
    saved: Option<String>,
}

impl Keys {
    pub fn start() -> Keys {
        #[cfg(unix)]
        {
            use std::process::{Command, Stdio};
            let saved = Command::new("stty")
                .arg("-g")
                .stdin(Stdio::inherit())
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
            if saved.is_some() {
                let _ = Command::new("stty").args(["-icanon", "-echo", "min", "0", "time", "1"]).stdin(Stdio::inherit()).status();
            }
            Keys { saved }
        }
        #[cfg(not(unix))]
        Keys { saved: None }
    }

    /// Whether keys come one at a time (else lines, Esc then Enter).
    pub fn raw(&self) -> bool {
        self.saved.is_some()
    }

    pub fn restore(&mut self) {
        #[cfg(unix)]
        if let Some(s) = self.saved.take() {
            let _ = std::process::Command::new("stty").arg(s).stdin(std::process::Stdio::inherit()).status();
        }
    }
}

impl Drop for Keys {
    fn drop(&mut self) {
        self.restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_bytes_and_the_clock() {
        assert_eq!(random(32).len(), 32);
        assert_ne!(random(16), random(16));
        assert_eq!(clock(0).len(), 8);
        assert_eq!(signal_name(15), "SIGTERM");
    }
}
