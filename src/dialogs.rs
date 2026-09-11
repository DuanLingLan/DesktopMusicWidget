//! Native shell dialogs: the folder picker, opening things in Explorer, and
//! message boxes.
//!
//! The folder picker deliberately gets **no owner window**: the card is a
//! `WS_EX_NOACTIVATE` window parked at the bottom of the Z order, and using it as
//! the owner makes the dialog fail to come to the foreground. Passing `None`
//! gives a normal, focused dialog. Only the settings window is passed as an
//! owner, and only when it exists.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use windows::{
    core::{w, HSTRING, PCWSTR, PWSTR},
    Win32::{
        Foundation::HWND,
        System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER},
        UI::{
            Shell::{
                FOLDERID_Music, FileOpenDialog, IFileOpenDialog, IShellItem,
                SHCreateItemFromParsingName, SHGetKnownFolderPath, ShellExecuteW,
                FOS_ALLOWMULTISELECT, FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR, FOS_PATHMUSTEXIST,
                FOS_PICKFOLDERS, KF_FLAG_DEFAULT, SIGDN_FILESYSPATH,
            },
            WindowsAndMessaging::{
                MessageBoxW, IDYES, MB_ICONERROR, MB_ICONINFORMATION, MB_ICONQUESTION,
                MB_ICONWARNING, MB_OK, MB_OKCANCEL, MB_YESNO, MESSAGEBOX_STYLE, SW_SHOWNORMAL,
            },
        },
    },
};

/// NUL-terminated UTF-16, for the Win32 APIs that want a `PCWSTR`.
pub fn wide_os(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

pub fn wide(s: &str) -> Vec<u16> {
    wide_os(OsStr::new(s))
}

fn free(p: PWSTR) {
    unsafe { CoTaskMemFree(Some(p.0 as *const core::ffi::c_void)) };
}

/// Shows the folder picker. Returns an empty vector when the user cancels, which
/// callers must treat as "no change" rather than "remove everything".
pub fn pick_folders(
    owner: Option<HWND>,
    title: &str,
    start: Option<&Path>,
    multi: bool,
) -> Vec<PathBuf> {
    let mut out = Vec::new();
    unsafe {
        let dialog: IFileOpenDialog =
            match CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER) {
                Ok(d) => d,
                Err(e) => {
                    crate::log_error!("cannot create the folder picker: {e}");
                    return out;
                }
            };

        let mut options = match dialog.GetOptions() {
            Ok(o) => o,
            Err(e) => {
                crate::log_error!("folder picker GetOptions failed: {e}");
                return out;
            }
        };
        options |= FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_NOCHANGEDIR;
        if multi {
            options |= FOS_ALLOWMULTISELECT;
        }
        if let Err(e) = dialog.SetOptions(options) {
            crate::log_error!("folder picker SetOptions failed: {e}");
            return out;
        }

        let title_h = HSTRING::from(title);
        let _ = dialog.SetTitle(&title_h);

        // Start where the user already keeps music, when we know.
        if let Some(start) = start {
            let start_h = HSTRING::from(start.as_os_str());
            if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&start_h, None) {
                let _ = dialog.SetFolder(&item);
            }
        }

        // A user cancel is reported as an error (HRESULT_FROM_WIN32(ERROR_CANCELLED))
        // and is not worth logging as a failure.
        if let Err(e) = dialog.Show(owner) {
            if e.code().0 as u32 != 0x8007_04C7 {
                crate::log_warn!("folder picker closed with {e}");
            }
            return out;
        }

        let results = match dialog.GetResults() {
            Ok(r) => r,
            Err(e) => {
                crate::log_error!("folder picker GetResults failed: {e}");
                return out;
            }
        };
        let count = results.GetCount().unwrap_or(0);
        for i in 0..count {
            let Ok(item) = results.GetItemAt(i) else {
                continue;
            };
            let Ok(path) = item.GetDisplayName(SIGDN_FILESYSPATH) else {
                continue;
            };
            let text = path.to_string();
            free(path);
            if let Ok(text) = text {
                out.push(PathBuf::from(text));
            }
        }
    }
    out
}

/// `%USERPROFILE%\Music` (or wherever the user redirected it to).
pub fn default_music_dir() -> Option<PathBuf> {
    unsafe {
        let p = SHGetKnownFolderPath(&FOLDERID_Music, KF_FLAG_DEFAULT, None).ok()?;
        let text = p.to_string();
        free(p);
        text.ok().map(PathBuf::from).filter(|p| p.is_dir())
    }
}

fn shell_exec(file: &Path, params: Option<&str>, verb: PCWSTR) -> bool {
    let file_w = wide_os(file.as_os_str());
    let params_w = params.map(wide);
    let params_ptr = match &params_w {
        Some(v) => PCWSTR(v.as_ptr()),
        None => PCWSTR(std::ptr::null()),
    };
    unsafe {
        let result = ShellExecuteW(
            None,
            verb,
            PCWSTR(file_w.as_ptr()),
            params_ptr,
            PCWSTR(std::ptr::null()),
            SW_SHOWNORMAL,
        );
        // ShellExecuteW returns a fake HINSTANCE; anything above 32 means success.
        result.0 as usize > 32
    }
}

/// Opens a folder, or reveals a file inside one.
pub fn open_in_explorer(path: &Path) {
    if !shell_exec(path, None, w!("open")) {
        crate::log_warn!("cannot open {}", path.display());
    }
}

/// Selects a file in Explorer rather than opening it with its default handler.
pub fn reveal_in_explorer(path: &Path) {
    if path.is_dir() {
        open_in_explorer(path);
        return;
    }
    if !shell_exec(
        Path::new("explorer.exe"),
        Some(&format!("/select,\"{}\"", path.display())),
        w!("open"),
    ) {
        open_in_explorer(path.parent().unwrap_or(path));
    }
}

/// Opens a file with its default handler, falling back to Notepad — `.toml` has
/// no association on a stock Windows install, which is exactly the file we care
/// about here.
pub fn open_path(path: &Path) {
    if shell_exec(path, None, w!("open")) {
        return;
    }
    crate::log_info!("no handler for {}, trying notepad", path.display());
    if !shell_exec(
        Path::new("notepad.exe"),
        Some(&format!("\"{}\"", path.display())),
        w!("open"),
    ) {
        let text = crate::i18n::tr_fmt(
            crate::i18n::Key::DlgOpenConfigFailedText,
            &[&path.display().to_string()],
        );
        error(
            None,
            crate::i18n::tr(crate::i18n::Key::DlgOpenConfigFailedTitle),
            &text,
        );
    }
}

fn boxed(owner: Option<HWND>, title: &str, text: &str, flags: MESSAGEBOX_STYLE) -> i32 {
    let title_w = wide(title);
    let text_w = wide(text);
    unsafe {
        MessageBoxW(
            owner,
            PCWSTR(text_w.as_ptr()),
            PCWSTR(title_w.as_ptr()),
            flags,
        )
        .0
    }
}

pub fn info(owner: Option<HWND>, title: &str, text: &str) {
    boxed(owner, title, text, MB_OK | MB_ICONINFORMATION);
}

pub fn warn(owner: Option<HWND>, title: &str, text: &str) {
    boxed(owner, title, text, MB_OK | MB_ICONWARNING);
}

pub fn error(owner: Option<HWND>, title: &str, text: &str) {
    boxed(owner, title, text, MB_OK | MB_ICONERROR);
}

pub fn confirm(owner: Option<HWND>, title: &str, text: &str) -> bool {
    boxed(owner, title, text, MB_YESNO | MB_ICONQUESTION) == IDYES.0
}

/// `true` when the user pressed OK, `false` on cancel.
pub fn ok_cancel(owner: Option<HWND>, title: &str, text: &str) -> bool {
    boxed(owner, title, text, MB_OKCANCEL | MB_ICONINFORMATION) == 1 // IDOK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_strings_are_nul_terminated() {
        assert_eq!(wide("a"), vec![b'a' as u16, 0]);
        assert_eq!(wide(""), vec![0]);
        let p = wide_os(OsStr::new("C:\\x"));
        assert_eq!(*p.last().unwrap(), 0);
        assert_eq!(p.len(), 5);
    }
}
