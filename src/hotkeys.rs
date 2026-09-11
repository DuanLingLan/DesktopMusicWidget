//! Global hotkey registration.
//!
//! `Ctrl+Alt+<arrow>` is claimed by the Intel graphics driver's screen-rotation
//! shortcut on a lot of machines, so each combination falls back to adding Shift
//! rather than silently doing nothing. Failures are reported so the settings
//! window can tell the user instead of leaving them guessing.

use std::sync::atomic::{AtomicBool, Ordering};
use windows::Win32::{
    Foundation::HWND,
    UI::Input::KeyboardAndMouse::{
        RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
        MOD_SHIFT,
    },
};

pub const HK_PLAYPAUSE: i32 = 1;
pub const HK_NEXT: i32 = 2;
pub const HK_PREV: i32 = 3;
pub const HK_VOLUP: i32 = 4;
pub const HK_VOLDOWN: i32 = 5;

// Virtual-key codes inlined so the table below can be a `const`.
const VK_SPACE: u32 = 0x20;
const VK_LEFT: u32 = 0x25;
const VK_UP: u32 = 0x26;
const VK_RIGHT: u32 = 0x27;
const VK_DOWN: u32 = 0x28;

/// `id`, virtual key, and a readable name used in the failure message.
const COMBOS: [(i32, u32, &str); 5] = [
    (HK_PLAYPAUSE, VK_SPACE, "Space"),
    (HK_NEXT, VK_RIGHT, "Right"),
    (HK_PREV, VK_LEFT, "Left"),
    (HK_VOLUP, VK_UP, "Up"),
    (HK_VOLDOWN, VK_DOWN, "Down"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    CtrlAlt,
    CtrlShiftAlt,
}

impl Preset {
    pub fn from_config(value: &str) -> Self {
        if value == "ctrl-shift-alt" {
            Self::CtrlShiftAlt
        } else {
            Self::CtrlAlt
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::CtrlAlt => "ctrl-alt",
            Self::CtrlShiftAlt => "ctrl-shift-alt",
        }
    }

    fn modifiers(self) -> HOT_KEY_MODIFIERS {
        match self {
            // Never MOD_WIN: those combinations are reserved by the shell.
            Self::CtrlAlt => MOD_CONTROL | MOD_ALT,
            Self::CtrlShiftAlt => MOD_CONTROL | MOD_SHIFT | MOD_ALT,
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Registration {
    pub registered: usize,
    /// Human names of the combinations that could not be claimed.
    pub failed: Vec<&'static str>,
}

static ACTIVE: AtomicBool = AtomicBool::new(false);

/// (Re)registers every hotkey. Any previous registration for this window is
/// released first, so this is safe to call on a preset change.
pub fn register(hwnd: HWND, preset: Preset) -> Registration {
    unregister(hwnd);
    let base = preset.modifiers() | MOD_NOREPEAT;
    let mut out = Registration::default();
    unsafe {
        for (id, vk, name) in COMBOS {
            let plain = RegisterHotKey(Some(hwnd), id, base, vk).is_ok();
            let shifted = plain || RegisterHotKey(Some(hwnd), id, base | MOD_SHIFT, vk).is_ok();
            if shifted {
                out.registered += 1;
            } else {
                out.failed.push(name);
            }
        }
    }
    ACTIVE.store(out.registered > 0, Ordering::Relaxed);
    out
}

pub fn unregister(hwnd: HWND) {
    unsafe {
        for (id, _, _) in COMBOS {
            let _ = UnregisterHotKey(Some(hwnd), id);
        }
    }
    ACTIVE.store(false, Ordering::Relaxed);
}

/// Whether any hotkey is currently held, so the app knows not to bother.
pub fn any_active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// Which action a `WM_HOTKEY` id maps to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    TogglePause,
    Next,
    Prev,
    VolumeUp,
    VolumeDown,
}

pub fn action_for(id: i32) -> Option<HotkeyAction> {
    match id {
        HK_PLAYPAUSE => Some(HotkeyAction::TogglePause),
        HK_NEXT => Some(HotkeyAction::Next),
        HK_PREV => Some(HotkeyAction::Prev),
        HK_VOLUP => Some(HotkeyAction::VolumeUp),
        HK_VOLDOWN => Some(HotkeyAction::VolumeDown),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_round_trip_through_config_strings() {
        for preset in [Preset::CtrlAlt, Preset::CtrlShiftAlt] {
            assert_eq!(Preset::from_config(preset.as_str()), preset);
        }
        // Unknown values fall back to the historical default.
        assert_eq!(Preset::from_config("nonsense"), Preset::CtrlAlt);
    }

    #[test]
    fn never_uses_the_windows_key() {
        // MOD_WIN combinations are reserved by the shell and are silently
        // dropped, which would look like a broken hotkey.
        for preset in [Preset::CtrlAlt, Preset::CtrlShiftAlt] {
            let m = preset.modifiers().0;
            assert_eq!(m & 0x0008, 0, "MOD_WIN must not be used");
            assert!(m & 0x0002 != 0, "every preset must include Ctrl");
            assert!(m & 0x0001 != 0, "every preset must include Alt");
        }
    }

    #[test]
    fn shifted_preset_is_a_superset() {
        let base = Preset::CtrlAlt.modifiers().0;
        let shifted = Preset::CtrlShiftAlt.modifiers().0;
        assert_eq!(
            shifted & base,
            base,
            "the fallback must add to, not replace"
        );
        assert!(shifted & 0x0004 != 0, "the fallback adds Shift");
    }

    #[test]
    fn combo_ids_are_unique() {
        let mut ids: Vec<i32> = COMBOS.iter().map(|(id, _, _)| *id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), COMBOS.len());
    }

    #[test]
    fn every_combo_id_resolves_to_an_action() {
        for (id, _, _) in COMBOS {
            assert!(
                action_for(id).is_some(),
                "hotkey id {id} has no handler registered"
            );
        }
        assert!(action_for(999).is_none());
    }
}
