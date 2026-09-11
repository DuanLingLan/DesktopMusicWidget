//! Autostart registration under HKCU (no UAC prompt, per-user).
//!
//! Writing to the Run key is a system-level change, so it never happens
//! silently: it is driven by the `autostart` config value, which the tray menu
//! and the settings window both write.

use std::path::Path;
use windows::{
    core::*,
    Win32::{
        Foundation::WIN32_ERROR,
        System::Registry::{
            RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
            HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_SZ,
        },
    },
};

const RUN_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const VALUE_NAME: PCWSTR = w!("DesktopMusicWidget");
/// The pre-rename build's entry. Detected and offered for removal, but never
/// deleted unless the user asks — silently touching it would break an existing
/// install of the older version.
const LEGACY_VALUE_NAME: PCWSTR = w!("HorizMusicWidget");

/// `ERROR_FILE_NOT_FOUND`, which for our purposes means "already gone".
const ERROR_FILE_NOT_FOUND: u32 = 2;

/// windows-rs returns WIN32_ERROR for these calls, so convert manually.
fn check(r: WIN32_ERROR) -> Result<()> {
    if r.0 == 0 {
        Ok(())
    } else {
        Err(Error::from_hresult(windows::core::HRESULT(r.0 as i32)))
    }
}

fn open(write: bool) -> Result<HKEY> {
    let rights = if write { KEY_SET_VALUE } else { KEY_READ };
    unsafe {
        let mut hkey = HKEY::default();
        check(RegOpenKeyExW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            None,
            rights,
            &mut hkey,
        ))?;
        Ok(hkey)
    }
}

/// The command line stored in the Run value.
///
/// Quoted on purpose: an unquoted `C:\Program Files\...\app.exe` is split at the
/// first space by the shell, so the entry would silently fail to launch — which
/// is exactly what happens for most users installing to Program Files.
fn run_value_for(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

fn write_value(name: PCWSTR, text: &str) -> Result<()> {
    let mut cmd: Vec<u16> = text.encode_utf16().collect();
    cmd.push(0);
    // SAFETY: `bytes` aliases `cmd`, which outlives the call.
    let bytes: &[u8] =
        unsafe { std::slice::from_raw_parts(cmd.as_ptr() as *const u8, cmd.len() * 2) };
    unsafe {
        let hkey = open(true)?;
        let r = check(RegSetValueExW(hkey, name, None, REG_SZ, Some(bytes)));
        let _ = RegCloseKey(hkey);
        r
    }
}

/// Reads a REG_SZ value. Returns `None` when absent or unreadable.
fn read_value(name: PCWSTR) -> Option<String> {
    let hkey = open(false).ok()?;
    unsafe {
        let mut kind = REG_SZ;
        let mut size = 0u32;
        let probe = RegQueryValueExW(hkey, name, None, Some(&mut kind), None, Some(&mut size));
        if probe.0 != 0 || size == 0 {
            let _ = RegCloseKey(hkey);
            return None;
        }

        let mut buf = vec![0u8; size as usize];
        let mut read = size;
        let got = RegQueryValueExW(
            hkey,
            name,
            None,
            Some(&mut kind),
            Some(buf.as_mut_ptr()),
            Some(&mut read),
        );
        let _ = RegCloseKey(hkey);
        if got.0 != 0 {
            return None;
        }

        let words: Vec<u16> = buf
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        Some(
            String::from_utf16_lossy(&words)
                .trim_end_matches('\0')
                .to_string(),
        )
    }
}

fn delete_value(name: PCWSTR) -> Result<()> {
    unsafe {
        let hkey = open(true)?;
        let r = RegDeleteValueW(hkey, name);
        let _ = RegCloseKey(hkey);
        // Removing something that is not there counts as success.
        if r.0 == 0 || r.0 == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            check(r)
        }
    }
}

/// Registers the current executable to run at logon.
pub fn install() -> Result<()> {
    let exe = std::env::current_exe()?;
    write_value(VALUE_NAME, &run_value_for(&exe))
}

/// Removes the logon entry. Succeeds if it was never installed.
pub fn remove() -> Result<()> {
    delete_value(VALUE_NAME)
}

pub fn is_installed() -> bool {
    read_value(VALUE_NAME).is_some()
}

/// The command line currently registered, for diagnostics and the About box.
pub fn installed_command() -> Option<String> {
    read_value(VALUE_NAME)
}

/// True when the pre-rename build is still registered to start at logon.
pub fn legacy_installed() -> bool {
    read_value(LEGACY_VALUE_NAME).is_some()
}

pub fn remove_legacy() -> Result<()> {
    delete_value(LEGACY_VALUE_NAME)
}

/// Reconciles the registry with the config value on startup.
///
/// Only ever *adds* or repairs: when the config says autostart is on but the
/// entry is missing or still points at an old location (the user moved the
/// folder), it is rewritten. It deliberately does not remove entries the config
/// knows nothing about.
///
/// Returns `true` when the registry was changed.
pub fn ensure(enabled: bool) -> Result<bool> {
    if !enabled {
        return Ok(false);
    }
    let exe = std::env::current_exe()?;
    let want = run_value_for(&exe);
    if read_value(VALUE_NAME).as_deref() == Some(want.as_str()) {
        return Ok(false);
    }
    write_value(VALUE_NAME, &want)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_value_is_quoted_so_spaces_survive() {
        let v = run_value_for(Path::new(r"C:\Program Files\DesktopMusicWidget\app.exe"));
        assert_eq!(
            v, r#""C:\Program Files\DesktopMusicWidget\app.exe""#,
            "an unquoted path with spaces would not launch"
        );
    }

    #[test]
    fn value_names_are_the_public_and_legacy_ones() {
        // `PCWSTR::to_string` is unsafe because the pointer must be NUL
        // terminated; these are compile-time literals, so it is.
        unsafe {
            assert_eq!(VALUE_NAME.to_string().unwrap(), "DesktopMusicWidget");
            assert_eq!(LEGACY_VALUE_NAME.to_string().unwrap(), "HorizMusicWidget");
        }
    }

    #[test]
    fn error_code_zero_is_success() {
        assert!(check(WIN32_ERROR(0)).is_ok());
        assert!(check(WIN32_ERROR(5)).is_err());
    }

    #[test]
    fn disabled_autostart_never_touches_the_registry() {
        // Must not read or write anything when off; observable as Ok(false).
        assert!(!ensure(false).unwrap());
    }
}
