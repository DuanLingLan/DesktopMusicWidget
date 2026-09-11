//! DesktopMusicWidget — a lightweight always-on-desktop music widget for Windows.
//!
//! The binary is a thin shell around these modules: `main.rs` parses arguments,
//! takes the single-instance mutex and runs the message loop; everything else
//! lives here so it can be unit-tested.

pub mod app;
pub mod art;
pub mod audio;
pub mod autostart;
pub mod cli;
pub mod config;
pub mod dialogs;
pub mod hotkeys;
pub mod i18n;
pub mod library;
pub mod log;
pub mod menu;
pub mod render;
pub mod settings;
pub mod theme;

use std::sync::OnceLock;
use windows::core::{w, PCWSTR};

/// Window class of the card.
pub const CLASS_NAME: PCWSTR = w!("DesktopMusicWidget_v1");
/// Window class of the settings window.
pub const SETTINGS_CLASS_NAME: PCWSTR = w!("DesktopMusicWidget_Settings_v1");
/// Single-instance mutex.
pub const MUTEX_NAME: PCWSTR = w!("DesktopMusicWidget_SingleInstance_v1");
/// Window title, also used as the tray tooltip fallback.
pub const WINDOW_TITLE: PCWSTR = w!("DesktopMusicWidget");

/// Kept alive for the process lifetime — dropping the handle releases the mutex.
/// Stored as the raw handle value because HANDLE is not Sync.
pub static INSTANCE_MUTEX: OnceLock<isize> = OnceLock::new();

/// The shell's `TaskbarCreated` message, so the tray icon can be re-added after
/// Explorer restarts.
pub static MSG_TASKBAR_CREATED: OnceLock<u32> = OnceLock::new();

/// Where the app's source lives, shown in the About box.
pub const HOMEPAGE: &str = "github.com/DuanLingLan/DesktopMusicWidget";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_names_are_distinct_and_versioned() {
        // `PCWSTR::to_string` is unsafe; these are compile-time literals.
        let (card, settings) = unsafe {
            (
                CLASS_NAME.to_string().unwrap(),
                SETTINGS_CLASS_NAME.to_string().unwrap(),
            )
        };
        assert_ne!(card, settings, "the two windows need separate classes");
        assert!(card.contains("DesktopMusicWidget"));
        assert!(settings.contains("DesktopMusicWidget"));
    }

    #[test]
    fn the_single_instance_mutex_is_not_the_legacy_name() {
        // Keeping these distinct lets the old build keep running side by side
        // with a new one, which matters while the author migrates.
        let name = unsafe { MUTEX_NAME.to_string().unwrap() };
        assert!(name.starts_with("DesktopMusicWidget"));
        assert!(!name.contains("Horiz"));
    }

    #[test]
    fn homepage_matches_the_package_repository() {
        assert!(env!("CARGO_PKG_REPOSITORY").contains(HOMEPAGE));
    }
}
