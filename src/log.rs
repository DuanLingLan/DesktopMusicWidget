//! A tiny file logger and a panic hook.
//!
//! A GUI-subsystem binary has nowhere to print, and `panic = "abort"` in the
//! release profile would otherwise make a crash completely silent — the window
//! simply disappears. Everything goes to `<config dir>\widget.log` (rotated at
//! 1 MB), and the panic hook additionally shows a message box pointing at it so
//! a user can attach the log to a bug report.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use windows::{
    core::PCWSTR,
    Win32::{
        System::{Console::GetConsoleWindow, SystemInformation::GetLocalTime},
        UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK},
    },
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Error,
}

static SINK: OnceLock<Option<Mutex<File>>> = OnceLock::new();
static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
static VERBOSE: AtomicBool = AtomicBool::new(false);

const MAX_BYTES: u64 = 1 << 20;

/// Opens the log file inside `dir`, rotating a previous oversized one.
/// Returns the path in use.
pub fn init(dir: &Path, verbose: bool) -> PathBuf {
    VERBOSE.store(verbose, Ordering::Relaxed);
    let path = dir.join("widget.log");
    let _ = LOG_PATH.set(path.clone());
    let _ = std::fs::create_dir_all(dir);

    let oversized = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > MAX_BYTES;
    if oversized {
        let _ = std::fs::rename(&path, dir.join("widget.log.1"));
    }

    let sink = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()
        .map(Mutex::new);
    let _ = SINK.set(sink);

    log_line(
        Level::Info,
        &format!(
            "=== DesktopMusicWidget {} start (os={}, arch={}) ===",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
    );
    path
}

pub fn path() -> Option<&'static Path> {
    LOG_PATH.get().map(PathBuf::as_path)
}

fn stamp() -> String {
    let t = unsafe { GetLocalTime() };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond, t.wMilliseconds
    )
}

/// Writes one line. Never panics and never blocks on a poisoned lock, because
/// logging must not be able to take the app down.
pub fn log_line(level: Level, msg: &str) {
    let tag = match level {
        Level::Info => "INFO ",
        Level::Warn => "WARN ",
        Level::Error => "ERROR",
    };
    let line = format!("{} {tag} {msg}\n", stamp());

    if let Some(Some(sink)) = SINK.get() {
        if let Ok(mut f) = sink.lock() {
            let _ = f.write_all(line.as_bytes());
            let _ = f.flush();
        }
    }

    // Echo to stderr when a console is attached (or when asked), so running the
    // exe from a terminal is still useful for debugging.
    let has_console = unsafe { !GetConsoleWindow().0.is_null() };
    if has_console || VERBOSE.load(Ordering::Relaxed) {
        let _ = std::io::stderr().write_all(line.as_bytes());
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Logs a panic (including the payload and location) and tells the user where
/// the log is before the process aborts.
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("panic: {info}");
        log_line(Level::Error, &msg);

        let text = match path() {
            Some(p) => format!("{msg}\n\nLog file:\n{}", p.display()),
            None => msg,
        };
        let title = wide("DesktopMusicWidget");
        let body = wide(&text);
        unsafe {
            MessageBoxW(
                None,
                PCWSTR(body.as_ptr()),
                PCWSTR(title.as_ptr()),
                MB_OK | MB_ICONERROR,
            );
        }
    }));
}

#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => { $crate::log::log_line($crate::log::Level::Info, &format!($($arg)*)) };
}

#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => { $crate::log::log_line($crate::log::Level::Warn, &format!($($arg)*)) };
}

#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => { $crate::log::log_line($crate::log::Level::Error, &format!($($arg)*)) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_is_idempotent_and_returns_a_path() {
        let dir = std::env::temp_dir().join("dmw-log-test");
        let p = init(&dir, false);
        assert!(p.ends_with("widget.log"));
        // A second call must not panic even though the OnceLock is taken.
        let p2 = init(&dir, false);
        assert_eq!(p, p2);
        assert!(path().is_some());
    }

    #[test]
    fn wide_strings_are_null_terminated() {
        let w = wide("hi");
        assert_eq!(w, vec![b'h' as u16, b'i' as u16, 0]);
    }
}
