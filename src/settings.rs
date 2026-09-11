//! The settings window.
//!
//! Built from the standard Win32 controls on purpose. BUTTON / STATIC / EDIT /
//! COMBOBOX / LISTBOX and the trackbar draw and hit-test themselves, so this
//! module is layout plus a command handler rather than an entire widget toolkit.
//!
//! The window holds a **draft** copy of the config: nothing is written to disk or
//! applied to the running widget until Apply / OK is pressed, so Cancel really does
//! discard. Switching the language is the one immediate action, so the user can
//! see what they picked.

use crate::app::App;
use crate::config::{self, Config};
use crate::dialogs;
use crate::i18n::{self, tr, tr_num, Key};
use crate::library::{self, PreviewState, ScanOptions};
use crate::log_warn;
use crate::SETTINGS_CLASS_NAME;

use std::ffi::c_void;
use std::sync::Arc;
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Graphics::Gdi::{CreateFontIndirectW, DeleteObject, COLOR_WINDOW, HBRUSH, HFONT, HGDIOBJ},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::{InitCommonControlsEx, ICC_BAR_CLASSES, INITCOMMONCONTROLSEX},
            HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow},
            WindowsAndMessaging::*,
        },
    },
};

/// `WM_USER + 0`. windows-rs exposes the neighbouring trackbar messages but not
/// this one, so it is spelled out.
const TBM_GETPOS: u32 = 1024;
const TBM_SETPOS: u32 = 1029;
const TBM_SETRANGE: u32 = 1030;

// Button and combo messages, spelled out for the same reason.
const BM_GETCHECK: u32 = 0x00F0;
const BM_SETCHECK: u32 = 0x00F1;
const BST_CHECKED: isize = 1;
const CB_ADDSTRING: u32 = 0x0143;
const CB_GETCURSEL: u32 = 0x0147;
const CB_RESETCONTENT: u32 = 0x014B;
const CB_SETCURSEL: u32 = 0x014E;
const LB_ADDSTRING: u32 = 0x0180;
const LB_GETCURSEL: u32 = 0x0188;
const LB_RESETCONTENT: u32 = 0x0184;

const BN_CLICKED: u32 = 0;
const CBN_SELCHANGE: u32 = 1;

const TIMER_PREVIEW: usize = 2;

const ID_FOLDERS: usize = 2001;
const ID_ADD: usize = 2002;
const ID_REMOVE: usize = 2003;
const ID_OPEN: usize = 2004;
const ID_EXCLUDES: usize = 2010;
const ID_EXCL_ADD: usize = 2011;
const ID_EXCL_REMOVE: usize = 2012;
const ID_RECURSIVE: usize = 2020;
const ID_FOUND: usize = 2021;
const ID_AUTOSTART: usize = 2030;
const ID_AUTOPLAY: usize = 2031;
const ID_PLAY_MODE: usize = 2032;
const ID_VOLUME: usize = 2033;
const ID_VOLUME_VALUE: usize = 2034;
const ID_WIDTH: usize = 2040;
const ID_HEIGHT: usize = 2041;
const ID_CORNERS: usize = 2042;
const ID_OPACITY: usize = 2043;
const ID_OPACITY_VALUE: usize = 2044;
const ID_SHOW_CONTROLS: usize = 2045;
const ID_ZORDER: usize = 2046;
const ID_MONITOR: usize = 2047;
const ID_FONT: usize = 2048;
const ID_LANGUAGE: usize = 2050;
const ID_HOTKEYS: usize = 2051;
const ID_HOTKEY_PRESET: usize = 2052;
const ID_PATH_INFO: usize = 2060;
const ID_NOTE: usize = 2061;
const ID_DEFAULTS: usize = 2070;
const ID_OPEN_CONFIG: usize = 2071;
const ID_LEGACY: usize = 2072;
const ID_CANCEL: usize = 2080;
const ID_APPLY: usize = 2081;
const ID_OK: usize = 2082;

// Logical layout, in 96-dpi units; every rect is scaled on creation.
const CLIENT_W: i32 = 640;
const CLIENT_H: i32 = 624;

/// Handles of the interactive controls. Every labelled control (buttons,
/// checkboxes, group boxes, captions) is also registered in `titled` so
/// `relabel` can rewrite all of them from the catalog in one pass.
#[derive(Default)]
struct Controls {
    folders: HWND,
    excludes: HWND,
    recursive: HWND,
    found: HWND,
    autostart: HWND,
    autoplay: HWND,
    play_mode: HWND,
    volume: HWND,
    volume_value: HWND,
    width: HWND,
    height: HWND,
    corners: HWND,
    opacity: HWND,
    opacity_value: HWND,
    show_controls: HWND,
    zorder: HWND,
    monitor: HWND,
    font: HWND,
    language: HWND,
    hotkeys: HWND,
    hotkey_preset: HWND,
    path_info: HWND,
    note: HWND,
    cancel: HWND,
    titled: Vec<(HWND, Key)>,
}

impl Controls {
    fn add_titled(&mut self, hwnd: HWND, key: Key) {
        if !hwnd.0.is_null() {
            self.titled.push((hwnd, key));
        }
    }
}

struct State {
    app: *mut App,
    hwnd: HWND,
    instance: HINSTANCE,
    scale: f32,
    font: HFONT,
    draft: Config,
    ctl: Controls,
    preview: Arc<PreviewState>,
    preview_key: ScanOptions,
}

// ---------------------------------------------------------------------------
// Control helpers
// ---------------------------------------------------------------------------

fn set_text(hwnd: HWND, text: &str) {
    if hwnd.0.is_null() {
        return;
    }
    let h = HSTRING::from(text);
    unsafe {
        let _ = SetWindowTextW(hwnd, &h);
    }
}

fn get_text(hwnd: HWND) -> String {
    if hwnd.0.is_null() {
        return String::new();
    }
    let len = unsafe { GetWindowTextLengthW(hwnd) };
    let mut buf = vec![0u16; len as usize + 1];
    let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n as usize])
}

fn read_int(hwnd: HWND) -> Option<i32> {
    get_text(hwnd).trim().parse::<i32>().ok()
}

/// `SendMessageW` takes `Option<WPARAM>` / `Option<LPARAM>` in windows-rs; this
/// wrapper keeps the call sites readable.
fn send_msg(hwnd: HWND, msg: u32, wparam: usize, lparam: isize) -> LRESULT {
    unsafe { SendMessageW(hwnd, msg, Some(WPARAM(wparam)), Some(LPARAM(lparam))) }
}

fn is_checked(hwnd: HWND) -> bool {
    send_msg(hwnd, BM_GETCHECK, 0, 0).0 == BST_CHECKED
}

fn set_checked(hwnd: HWND, on: bool) {
    send_msg(
        hwnd,
        BM_SETCHECK,
        if on { BST_CHECKED as usize } else { 0 },
        0,
    );
}

/// Sends a string message with a locally owned, NUL-terminated buffer. `HSTRING`
/// would work too, but the raw control messages are typed as a bare pointer and
/// owning the buffer here keeps the lifetime obvious.
fn send_text(hwnd: HWND, msg: u32, text: &str) {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    send_msg(hwnd, msg, 0, wide.as_ptr() as isize);
}

fn fill_combo(hwnd: HWND, items: &[String], selected: usize) {
    send_msg(hwnd, CB_RESETCONTENT, 0, 0);
    for item in items {
        send_text(hwnd, CB_ADDSTRING, item);
    }
    let index = if items.is_empty() {
        0
    } else {
        selected.min(items.len() - 1)
    };
    send_msg(hwnd, CB_SETCURSEL, index, 0);
}

fn combo_index(hwnd: HWND) -> usize {
    let r = send_msg(hwnd, CB_GETCURSEL, 0, 0);
    if r.0 < 0 {
        0
    } else {
        r.0 as usize
    }
}

fn fill_list(hwnd: HWND, items: &[String]) {
    send_msg(hwnd, LB_RESETCONTENT, 0, 0);
    for item in items {
        send_text(hwnd, LB_ADDSTRING, item);
    }
}

fn list_selection(hwnd: HWND) -> Option<usize> {
    let r = send_msg(hwnd, LB_GETCURSEL, 0, 0);
    if r.0 < 0 {
        None
    } else {
        Some(r.0 as usize)
    }
}

/// Sets the 0-100 range and the position, both with a repaint.
fn set_slider(hwnd: HWND, value: i32) {
    send_msg(hwnd, TBM_SETRANGE, 1, (100 << 16) as isize);
    send_msg(hwnd, TBM_SETPOS, 1, value as isize);
}

fn slider_pos(hwnd: HWND) -> i32 {
    send_msg(hwnd, TBM_GETPOS, 0, 0).0 as i32
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

struct Ui {
    parent: HWND,
    instance: HINSTANCE,
    font: HFONT,
    scale: f32,
}

impl Ui {
    fn px(&self, v: i32) -> i32 {
        (v as f32 * self.scale).round() as i32
    }

    #[allow(clippy::too_many_arguments)]
    fn mk(
        &self,
        class: PCWSTR,
        text: &str,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        id: usize,
    ) -> HWND {
        let htext = HSTRING::from(text);
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                &htext,
                WINDOW_STYLE(style),
                self.px(x),
                self.px(y),
                self.px(w),
                self.px(h),
                Some(self.parent),
                Some(HMENU(id as *mut c_void)),
                Some(self.instance),
                None,
            )
        }
        .unwrap_or_default();
        if !hwnd.0.is_null() {
            send_msg(hwnd, WM_SETFONT, self.font.0 as usize, 1);
        }
        hwnd
    }
}

/// Style bits, composed once so the layout below stays readable.
struct Styles {
    group: u32,
    button: u32,
    check: u32,
    label: u32,
    edit: u32,
    list: u32,
    combo: u32,
    slider: u32,
}

impl Styles {
    fn new() -> Self {
        let child = WS_CHILD.0 | WS_VISIBLE.0;
        let tab = WS_TABSTOP.0;
        Self {
            group: child | BS_GROUPBOX as u32,
            button: child | tab | BS_PUSHBUTTON as u32,
            check: child | tab | BS_AUTOCHECKBOX as u32,
            label: child,
            edit: child | tab | WS_BORDER.0 | ES_AUTOHSCROLL as u32,
            list: child | tab | WS_BORDER.0 | WS_VSCROLL.0 | LBS_NOTIFY,
            combo: child | tab | WS_VSCROLL.0 | CBS_DROPDOWNLIST,
            slider: child | tab,
        }
    }
}

const LBS_NOTIFY: u32 = 0x0001;
const CBS_DROPDOWNLIST: u32 = 0x0003;

fn build_controls(ui: &Ui) -> Controls {
    let s = Styles::new();
    let mut c = Controls::default();

    // --- music folders (left, top) -----------------------------------------
    let group = ui.mk(
        w!("BUTTON"),
        tr(Key::SetGroupFolders),
        s.group,
        12,
        10,
        300,
        150,
        0,
    );
    c.add_titled(group, Key::SetGroupFolders);
    c.folders = ui.mk(w!("LISTBOX"), "", s.list, 26, 34, 272, 86, ID_FOLDERS);
    let add = ui.mk(
        w!("BUTTON"),
        tr(Key::SetAdd),
        s.button,
        26,
        126,
        86,
        24,
        ID_ADD,
    );
    c.add_titled(add, Key::SetAdd);
    let remove = ui.mk(
        w!("BUTTON"),
        tr(Key::SetRemove),
        s.button,
        118,
        126,
        86,
        24,
        ID_REMOVE,
    );
    c.add_titled(remove, Key::SetRemove);
    let open = ui.mk(
        w!("BUTTON"),
        tr(Key::SetOpen),
        s.button,
        210,
        126,
        88,
        24,
        ID_OPEN,
    );
    c.add_titled(open, Key::SetOpen);

    // --- scan scope --------------------------------------------------------
    let group = ui.mk(
        w!("BUTTON"),
        tr(Key::SetGroupScan),
        s.group,
        12,
        170,
        300,
        190,
        0,
    );
    c.add_titled(group, Key::SetGroupScan);
    c.recursive = ui.mk(
        w!("BUTTON"),
        tr(Key::SetRecursive),
        s.check,
        26,
        196,
        272,
        20,
        ID_RECURSIVE,
    );
    c.add_titled(c.recursive, Key::SetRecursive);
    let caption = ui.mk(
        w!("STATIC"),
        tr(Key::SetExcludes),
        s.label,
        26,
        220,
        272,
        16,
        0,
    );
    c.add_titled(caption, Key::SetExcludes);
    c.excludes = ui.mk(w!("LISTBOX"), "", s.list, 26, 240, 272, 70, ID_EXCLUDES);
    let excl_add = ui.mk(
        w!("BUTTON"),
        tr(Key::SetAdd),
        s.button,
        26,
        316,
        132,
        24,
        ID_EXCL_ADD,
    );
    c.add_titled(excl_add, Key::SetAdd);
    let excl_remove = ui.mk(
        w!("BUTTON"),
        tr(Key::SetRemove),
        s.button,
        164,
        316,
        134,
        24,
        ID_EXCL_REMOVE,
    );
    c.add_titled(excl_remove, Key::SetRemove);
    c.found = ui.mk(w!("STATIC"), "", s.label, 26, 346, 272, 18, ID_FOUND);

    // --- general -----------------------------------------------------------
    let group = ui.mk(
        w!("BUTTON"),
        tr(Key::SetGroupGeneral),
        s.group,
        12,
        370,
        300,
        150,
        0,
    );
    c.add_titled(group, Key::SetGroupGeneral);
    let caption = ui.mk(
        w!("STATIC"),
        tr(Key::SetLanguage),
        s.label,
        26,
        394,
        90,
        16,
        0,
    );
    c.add_titled(caption, Key::SetLanguage);
    c.language = ui.mk(w!("COMBOBOX"), "", s.combo, 120, 392, 178, 200, ID_LANGUAGE);
    c.hotkeys = ui.mk(
        w!("BUTTON"),
        tr(Key::SetHotkeys),
        s.check,
        26,
        420,
        272,
        20,
        ID_HOTKEYS,
    );
    c.add_titled(c.hotkeys, Key::SetHotkeys);
    let caption = ui.mk(
        w!("STATIC"),
        tr(Key::SetHotkeyPreset),
        s.label,
        26,
        448,
        90,
        16,
        0,
    );
    c.add_titled(caption, Key::SetHotkeyPreset);
    c.hotkey_preset = ui.mk(
        w!("COMBOBOX"),
        "",
        s.combo,
        120,
        446,
        178,
        200,
        ID_HOTKEY_PRESET,
    );
    let legacy = ui.mk(
        w!("BUTTON"),
        tr(Key::SetRemoveLegacy),
        s.button,
        26,
        478,
        200,
        26,
        ID_LEGACY,
    );
    c.add_titled(legacy, Key::SetRemoveLegacy);

    // --- startup (right, top) ----------------------------------------------
    let group = ui.mk(
        w!("BUTTON"),
        tr(Key::SetGroupStartup),
        s.group,
        324,
        10,
        304,
        106,
        0,
    );
    c.add_titled(group, Key::SetGroupStartup);
    c.autostart = ui.mk(
        w!("BUTTON"),
        tr(Key::MenuAutostart),
        s.check,
        338,
        34,
        280,
        20,
        ID_AUTOSTART,
    );
    c.add_titled(c.autostart, Key::MenuAutostart);
    c.autoplay = ui.mk(
        w!("BUTTON"),
        tr(Key::MenuAutoplay),
        s.check,
        338,
        58,
        280,
        20,
        ID_AUTOPLAY,
    );
    c.add_titled(c.autoplay, Key::MenuAutoplay);
    let caption = ui.mk(
        w!("STATIC"),
        tr(Key::SetPlayMode),
        s.label,
        338,
        84,
        80,
        16,
        0,
    );
    c.add_titled(caption, Key::SetPlayMode);
    c.play_mode = ui.mk(w!("COMBOBOX"), "", s.combo, 422, 82, 182, 200, ID_PLAY_MODE);

    // --- volume ------------------------------------------------------------
    let group = ui.mk(w!("BUTTON"), "", s.group, 324, 126, 304, 70, 0);
    let _ = group; // untitled separator box
    let caption = ui.mk(
        w!("STATIC"),
        tr(Key::SetVolume),
        s.label,
        338,
        150,
        56,
        16,
        0,
    );
    c.add_titled(caption, Key::SetVolume);
    c.volume = ui.mk(
        w!("msctls_trackbar32"),
        "",
        s.slider,
        398,
        148,
        150,
        24,
        ID_VOLUME,
    );
    c.volume_value = ui.mk(w!("STATIC"), "", s.label, 552, 150, 60, 16, ID_VOLUME_VALUE);

    // --- appearance --------------------------------------------------------
    let group = ui.mk(
        w!("BUTTON"),
        tr(Key::SetGroupAppearance),
        s.group,
        324,
        206,
        304,
        196,
        0,
    );
    c.add_titled(group, Key::SetGroupAppearance);
    let caption = ui.mk(
        w!("STATIC"),
        tr(Key::SetWidth),
        s.label,
        338,
        230,
        50,
        16,
        0,
    );
    c.add_titled(caption, Key::SetWidth);
    c.width = ui.mk(w!("EDIT"), "", s.edit, 390, 228, 56, 22, ID_WIDTH);
    let caption = ui.mk(
        w!("STATIC"),
        tr(Key::SetHeight),
        s.label,
        456,
        230,
        50,
        16,
        0,
    );
    c.add_titled(caption, Key::SetHeight);
    c.height = ui.mk(w!("EDIT"), "", s.edit, 508, 228, 56, 22, ID_HEIGHT);
    let caption = ui.mk(
        w!("STATIC"),
        tr(Key::SetCorners),
        s.label,
        338,
        260,
        50,
        16,
        0,
    );
    c.add_titled(caption, Key::SetCorners);
    c.corners = ui.mk(w!("EDIT"), "", s.edit, 390, 258, 56, 22, ID_CORNERS);
    let caption = ui.mk(
        w!("STATIC"),
        tr(Key::SetOpacity),
        s.label,
        338,
        290,
        56,
        16,
        0,
    );
    c.add_titled(caption, Key::SetOpacity);
    c.opacity = ui.mk(
        w!("msctls_trackbar32"),
        "",
        s.slider,
        398,
        288,
        148,
        24,
        ID_OPACITY,
    );
    c.opacity_value = ui.mk(
        w!("STATIC"),
        "",
        s.label,
        552,
        290,
        60,
        16,
        ID_OPACITY_VALUE,
    );
    let caption = ui.mk(
        w!("STATIC"),
        tr(Key::SetShowControls),
        s.label,
        338,
        320,
        88,
        16,
        0,
    );
    c.add_titled(caption, Key::SetShowControls);
    c.show_controls = ui.mk(
        w!("COMBOBOX"),
        "",
        s.combo,
        430,
        318,
        174,
        200,
        ID_SHOW_CONTROLS,
    );
    let caption = ui.mk(
        w!("STATIC"),
        tr(Key::SetZOrder),
        s.label,
        338,
        348,
        88,
        16,
        0,
    );
    c.add_titled(caption, Key::SetZOrder);
    c.zorder = ui.mk(w!("COMBOBOX"), "", s.combo, 430, 346, 174, 200, ID_ZORDER);
    let caption = ui.mk(
        w!("STATIC"),
        tr(Key::SetMonitor),
        s.label,
        338,
        376,
        88,
        16,
        0,
    );
    c.add_titled(caption, Key::SetMonitor);
    c.monitor = ui.mk(w!("COMBOBOX"), "", s.combo, 430, 374, 174, 200, ID_MONITOR);

    // --- font --------------------------------------------------------------
    let group = ui.mk(w!("BUTTON"), "", s.group, 324, 412, 304, 68, 0);
    let _ = group;
    let caption = ui.mk(w!("STATIC"), tr(Key::SetFont), s.label, 338, 436, 50, 16, 0);
    c.add_titled(caption, Key::SetFont);
    c.font = ui.mk(w!("EDIT"), "", s.edit, 390, 434, 216, 22, ID_FONT);

    // --- footer ------------------------------------------------------------
    c.path_info = ui.mk(w!("STATIC"), "", s.label, 12, 536, 616, 18, ID_PATH_INFO);
    c.note = ui.mk(w!("STATIC"), "", s.label, 12, 556, 616, 18, ID_NOTE);
    let defaults = ui.mk(
        w!("BUTTON"),
        tr(Key::SetRestoreDefaults),
        s.button,
        12,
        582,
        120,
        30,
        ID_DEFAULTS,
    );
    c.add_titled(defaults, Key::SetRestoreDefaults);
    let open_config = ui.mk(
        w!("BUTTON"),
        tr(Key::MenuOpenConfig),
        s.button,
        140,
        582,
        130,
        30,
        ID_OPEN_CONFIG,
    );
    c.add_titled(open_config, Key::MenuOpenConfig);
    c.cancel = ui.mk(
        w!("BUTTON"),
        tr(Key::SetCancel),
        s.button,
        446,
        582,
        54,
        30,
        ID_CANCEL,
    );
    c.add_titled(c.cancel, Key::SetCancel);
    let apply = ui.mk(
        w!("BUTTON"),
        tr(Key::SetApply),
        s.button,
        508,
        582,
        54,
        30,
        ID_APPLY,
    );
    c.add_titled(apply, Key::SetApply);
    let ok = ui.mk(
        w!("BUTTON"),
        tr(Key::SetOk),
        s.button | BS_DEFPUSHBUTTON as u32,
        570,
        582,
        58,
        30,
        ID_OK,
    );
    c.add_titled(ok, Key::SetOk);

    c
}

/// Rewrites every caption from the catalog so a language change shows up at once.
fn relabel(state: &mut State) {
    let pairs: Vec<(HWND, Key)> = state.ctl.titled.clone();
    for (hwnd, key) in pairs {
        set_text(hwnd, tr(key));
    }
    // Anything whose text is composed rather than a plain catalog entry.
    set_text(
        state.ctl.path_info,
        &i18n::tr_fmt(
            Key::SetPathInfo,
            &[&config::config_path().display().to_string()],
        ),
    );
    update_slider_labels(state);
}

fn index_of(values: &[&str], needle: &str) -> usize {
    values.iter().position(|v| *v == needle).unwrap_or(0)
}

fn update_slider_labels(state: &State) {
    set_text(
        state.ctl.volume_value,
        &slider_pos(state.ctl.volume).to_string(),
    );
    set_text(
        state.ctl.opacity_value,
        &format!("{}%", slider_pos(state.ctl.opacity)),
    );
}

/// Pushes the draft into every control.
fn load_controls(state: &mut State) {
    let d = state.draft.clone();
    fill_list(state.ctl.folders, &d.library);
    fill_list(state.ctl.excludes, &d.exclude_dirs);

    set_checked(state.ctl.recursive, d.scan_recursive);
    set_checked(state.ctl.autostart, d.autostart);
    set_checked(state.ctl.autoplay, d.autoplay);
    set_checked(state.ctl.hotkeys, d.hotkeys_enabled);

    fill_combo(
        state.ctl.play_mode,
        &[
            tr(Key::PlayModeSequential).to_string(),
            tr(Key::PlayModeShuffle).to_string(),
            tr(Key::PlayModeRepeatOne).to_string(),
        ],
        index_of(config::PLAY_MODES, d.play_mode.as_str()),
    );
    fill_combo(
        state.ctl.show_controls,
        &[
            tr(Key::ShowControlsHover).to_string(),
            tr(Key::ShowControlsAlways).to_string(),
        ],
        index_of(config::SHOW_CONTROLS, d.show_controls.as_str()),
    );
    fill_combo(
        state.ctl.zorder,
        &[
            tr(Key::ModeBottom).to_string(),
            tr(Key::ModeTopmost).to_string(),
        ],
        index_of(config::MODES, d.mode.as_str()),
    );
    fill_combo(
        state.ctl.language,
        &[
            tr(Key::LangAuto).to_string(),
            tr(Key::LangZh).to_string(),
            tr(Key::LangEn).to_string(),
        ],
        index_of(config::LANGUAGES, d.language.as_str()),
    );
    fill_combo(
        state.ctl.hotkey_preset,
        &[
            tr(Key::HotkeyPresetCtrlAlt).to_string(),
            tr(Key::HotkeyPresetCtrlShiftAlt).to_string(),
        ],
        index_of(config::HOTKEY_PRESETS, d.hotkey_preset.as_str()),
    );

    let all = crate::app::monitors();
    let items: Vec<String> = (0..all.len())
        .map(|i| tr_num(Key::MonitorItem, i + 1))
        .collect();
    let selected = if d.monitor < 0 {
        all.iter()
            .position(|r| r.left == 0 && r.top == 0)
            .unwrap_or(0)
    } else {
        (d.monitor as usize).min(items.len().saturating_sub(1))
    };
    fill_combo(state.ctl.monitor, &items, selected);

    set_text(state.ctl.width, &d.width.to_string());
    set_text(state.ctl.height, &d.height.to_string());
    set_text(
        state.ctl.corners,
        &(d.corner_radius.round() as i32).to_string(),
    );
    set_text(state.ctl.font, &d.font_family);
    set_slider(state.ctl.volume, d.volume as i32);
    set_slider(state.ctl.opacity, (d.card_opacity * 100.0).round() as i32);
    update_slider_labels(state);
    set_text(
        state.ctl.path_info,
        &i18n::tr_fmt(
            Key::SetPathInfo,
            &[&config::config_path().display().to_string()],
        ),
    );
}

fn restart_preview(state: &mut State) {
    let opts = ScanOptions::from_config(&state.draft);
    if opts == state.preview_key && state.preview.running() {
        return;
    }
    state.preview_key = opts.clone();
    state.preview = library::spawn_preview(opts);
}

fn tick_preview(state: &mut State) {
    if state.preview.running() {
        set_text(state.ctl.found, tr(Key::SetScanning));
        return;
    }
    let text = if state.draft.library.is_empty() {
        tr(Key::SetNoLibrary).to_string()
    } else {
        tr_num(Key::SetFound, state.preview.count())
    };
    set_text(state.ctl.found, &text);
}

/// Reads the free-text number fields into the draft. Returns the message to show
/// when a field cannot be parsed.
fn read_numbers(state: &mut State) -> std::result::Result<(), String> {
    let (wlo, whi) = config::WIDTH_RANGE;
    let (hlo, hhi) = config::HEIGHT_RANGE;
    let width = read_int(state.ctl.width).ok_or_else(|| invalid_range(wlo, whi))?;
    let height = read_int(state.ctl.height).ok_or_else(|| invalid_range(hlo, hhi))?;
    let corners = read_int(state.ctl.corners).ok_or_else(|| invalid_range(0, hhi / 2))?;
    state.draft.width = width;
    state.draft.height = height;
    state.draft.corner_radius = corners as f32;
    Ok(())
}

fn invalid_range(low: i32, high: i32) -> String {
    i18n::tr_fmt(Key::SetInvalidNumber, &[&format!("{low}..{high}")])
}

/// Validates the draft and, if it holds together, pushes it onto the running
/// app. Returns false when the user needs to fix something first.
fn commit(state: &mut State) -> bool {
    if let Err(message) = read_numbers(state) {
        set_text(state.ctl.note, &message);
        return false;
    }

    let warnings = config::validate(&mut state.draft);
    for w in &warnings {
        log_warn!("settings: {w}");
    }

    if state.app.is_null() {
        return false;
    }
    let app = unsafe { &mut *state.app };
    let old = app.cfg.clone();
    app.cfg = state.draft.clone();
    app.apply_config(&old);

    // Read back whatever validation clamped, so the window never shows a value
    // that is not in effect.
    state.draft = app.cfg.clone();
    load_controls(state);
    let note = if warnings.is_empty() {
        tr(Key::DlgConfigSavedText).to_string()
    } else {
        warnings.join("; ")
    };
    set_text(state.ctl.note, &note);
    true
}

// ---------------------------------------------------------------------------
// Window procedure
// ---------------------------------------------------------------------------

extern "system" fn settings_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_CREATE {
        unsafe {
            let cs = &*(lparam.0 as *const CREATESTRUCTW);
            let raw = cs.lpCreateParams as *mut State;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, raw as isize);
            let state = &mut *raw;
            state.hwnd = hwnd;
            if !state.app.is_null() {
                (*state.app).settings_hwnd = hwnd;
            }
            let ui = Ui {
                parent: hwnd,
                instance: state.instance,
                font: state.font,
                scale: state.scale,
            };
            state.ctl = build_controls(&ui);
            load_controls(state);
            restart_preview(state);
            let _ = SetTimer(Some(hwnd), TIMER_PREVIEW, 250, None);
        }
        return LRESULT(0);
    }

    let p = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State };
    if p.is_null() {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    let state = unsafe { &mut *p };

    match msg {
        WM_TIMER => {
            tick_preview(state);
            LRESULT(0)
        }
        WM_HSCROLL => {
            update_slider_labels(state);
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = wparam.0 & 0xffff;
            let code = ((wparam.0 >> 16) & 0xffff) as u32;
            handle_command(state, id, code);
            LRESULT(0)
        }
        WM_CLOSE => {
            // Close means cancel: nothing is committed without Apply / OK.
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe {
                let state = Box::from_raw(p);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                // Child windows are destroyed with their parent; only the GDI
                // object we created needs releasing.
                let _ = DeleteObject(HGDIOBJ::from(state.font));
                if !state.app.is_null() {
                    (*state.app).settings_hwnd = HWND::default();
                }
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn handle_command(state: &mut State, id: usize, code: u32) {
    // Combos and list boxes only act on a selection change; buttons only on a
    // click. Ignoring the other notifications keeps focus changes from applying
    // settings by accident.
    match id {
        ID_ADD if code == BN_CLICKED => {
            let start = state.draft.library_paths().into_iter().next();
            let picked = dialogs::pick_folders(
                Some(state.hwnd),
                tr(Key::DlgPickFolderTitle),
                start.as_deref(),
                true,
            );
            for p in picked {
                let text = p.to_string_lossy().to_string();
                if !state.draft.library.contains(&text) {
                    state.draft.library.push(text);
                }
            }
            fill_list(state.ctl.folders, &state.draft.library);
            restart_preview(state);
        }
        ID_REMOVE if code == BN_CLICKED => {
            if let Some(i) = list_selection(state.ctl.folders) {
                if i < state.draft.library.len() {
                    state.draft.library.remove(i);
                    fill_list(state.ctl.folders, &state.draft.library);
                    restart_preview(state);
                }
            }
        }
        ID_OPEN if code == BN_CLICKED => {
            let index = list_selection(state.ctl.folders).unwrap_or(0);
            if let Some(path) = state.draft.library_paths().get(index) {
                if path.is_dir() {
                    dialogs::open_in_explorer(path);
                } else {
                    let text =
                        i18n::tr_fmt(Key::DlgFolderMissingText, &[&path.display().to_string()]);
                    dialogs::warn(Some(state.hwnd), tr(Key::DlgFolderMissingTitle), &text);
                }
            }
        }
        ID_EXCL_ADD if code == BN_CLICKED => {
            let start = state.draft.library_paths().into_iter().next();
            let picked = dialogs::pick_folders(
                Some(state.hwnd),
                tr(Key::DlgPickExcludeTitle),
                start.as_deref(),
                true,
            );
            for p in picked {
                let text = p.to_string_lossy().to_string();
                if !state.draft.exclude_dirs.contains(&text) {
                    state.draft.exclude_dirs.push(text);
                }
            }
            fill_list(state.ctl.excludes, &state.draft.exclude_dirs);
            restart_preview(state);
        }
        ID_EXCL_REMOVE if code == BN_CLICKED => {
            if let Some(i) = list_selection(state.ctl.excludes) {
                if i < state.draft.exclude_dirs.len() {
                    state.draft.exclude_dirs.remove(i);
                    fill_list(state.ctl.excludes, &state.draft.exclude_dirs);
                    restart_preview(state);
                }
            }
        }
        ID_RECURSIVE if code == BN_CLICKED => {
            state.draft.scan_recursive = is_checked(state.ctl.recursive);
            restart_preview(state);
        }
        ID_AUTOSTART if code == BN_CLICKED => {
            state.draft.autostart = is_checked(state.ctl.autostart);
        }
        ID_AUTOPLAY if code == BN_CLICKED => {
            state.draft.autoplay = is_checked(state.ctl.autoplay);
        }
        ID_HOTKEYS if code == BN_CLICKED => {
            state.draft.hotkeys_enabled = is_checked(state.ctl.hotkeys);
        }
        ID_PLAY_MODE if code == CBN_SELCHANGE => {
            let options = config::PLAY_MODES;
            state.draft.play_mode =
                options[combo_index(state.ctl.play_mode).min(options.len() - 1)].to_string();
        }
        ID_SHOW_CONTROLS if code == CBN_SELCHANGE => {
            let options = config::SHOW_CONTROLS;
            state.draft.show_controls =
                options[combo_index(state.ctl.show_controls).min(options.len() - 1)].to_string();
        }
        ID_ZORDER if code == CBN_SELCHANGE => {
            let options = config::MODES;
            state.draft.mode =
                options[combo_index(state.ctl.zorder).min(options.len() - 1)].to_string();
        }
        ID_LANGUAGE if code == CBN_SELCHANGE => {
            let options = config::LANGUAGES;
            let pick = options[combo_index(state.ctl.language).min(options.len() - 1)];
            state.draft.language = pick.to_string();
            // Applied immediately so the user sees the choice take effect; every
            // other field still waits for Apply.
            i18n::init(pick);
            relabel(state);
            load_controls(state);
        }
        ID_HOTKEY_PRESET if code == CBN_SELCHANGE => {
            let options = config::HOTKEY_PRESETS;
            state.draft.hotkey_preset =
                options[combo_index(state.ctl.hotkey_preset).min(options.len() - 1)].to_string();
        }
        ID_DEFAULTS if code == BN_CLICKED => {
            // Restoring defaults keeps the folders the user picked: silently
            // dropping their library behind a button would be a nasty surprise.
            let library = state.draft.library.clone();
            let excludes = state.draft.exclude_dirs.clone();
            state.draft = Config::default();
            state.draft.library = library;
            state.draft.exclude_dirs = excludes;
            load_controls(state);
            restart_preview(state);
        }
        ID_OPEN_CONFIG if code == BN_CLICKED => {
            dialogs::open_path(&config::config_path());
        }
        ID_LEGACY if code == BN_CLICKED => {
            if dialogs::confirm(
                Some(state.hwnd),
                tr(Key::DlgConfirmRemoveLegacyTitle),
                tr(Key::DlgConfirmRemoveLegacyText),
            ) {
                match crate::autostart::remove_legacy() {
                    Ok(()) => set_text(state.ctl.note, tr(Key::DlgConfigSavedText)),
                    Err(e) => {
                        log_warn!("cannot remove the legacy autostart entry: {e}");
                        set_text(state.ctl.note, tr(Key::DlgConfigSavedText));
                    }
                }
            }
        }
        ID_CANCEL if code == BN_CLICKED => unsafe {
            let _ = DestroyWindow(state.hwnd);
        },
        ID_APPLY if code == BN_CLICKED => {
            commit(state);
        }
        ID_OK if code == BN_CLICKED => {
            // Only close when the values were accepted; otherwise the window
            // stays open with the reason on the note line. Written as an early
            // return rather than `if commit(..) { .. }` so that the guard and
            // the side-effecting call are not merged into one condition.
            if !commit(state) {
                return;
            }
            unsafe {
                let _ = DestroyWindow(state.hwnd);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Opens the settings window, or brings the existing one to the front.
pub fn open(app: &mut App) {
    if is_open(app) {
        unsafe {
            let _ = SetForegroundWindow(app.settings_hwnd);
        }
        return;
    }
    if let Err(e) = create(app) {
        crate::log_error!("cannot open the settings window: {e}");
    }
}

fn create(app: &mut App) -> Result<()> {
    unsafe {
        let instance: HINSTANCE = GetModuleHandleW(None)?.into();
        register_class(instance)?;

        let icc = INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_BAR_CLASSES,
        };
        let _ = InitCommonControlsEx(&icc);

        // The card's DPI is authoritative when known; the window's own DPI is
        // only readable once it exists.
        let dpi = if app.dpi > 0 {
            app.dpi
        } else {
            GetDpiForWindow(app.hwnd)
        };
        let scale = dpi as f32 / 96.0;
        let font = message_font().unwrap_or_default();

        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
        let ex_style = WINDOW_EX_STYLE(0x0001_0000); // WS_EX_CONTROLPARENT
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: (CLIENT_W as f32 * scale).round() as i32,
            bottom: (CLIENT_H as f32 * scale).round() as i32,
        };
        let _ = AdjustWindowRectExForDpi(&mut rect, style, false, ex_style, dpi);

        let draft = app.cfg.clone();
        let preview_key = ScanOptions::from_config(&draft);
        let preview = library::spawn_preview(preview_key.clone());

        // The controls cannot be built until the parent exists, so the state box
        // travels in through lpCreateParams with an empty `ctl`.
        let state = Box::new(State {
            app: app as *mut App,
            hwnd: HWND::default(),
            instance,
            scale,
            font,
            draft,
            ctl: Controls::default(),
            preview,
            preview_key,
        });

        let title = HSTRING::from(tr(Key::SetTitle));
        let state_ptr = Box::into_raw(state);
        let hwnd = match CreateWindowExW(
            ex_style,
            SETTINGS_CLASS_NAME,
            &title,
            style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            rect.right - rect.left,
            rect.bottom - rect.top,
            None,
            None,
            Some(instance),
            Some(state_ptr as *const c_void),
        ) {
            Ok(hwnd) => hwnd,
            Err(e) => {
                // WM_CREATE never ran, so it never took ownership of the box.
                drop(Box::from_raw(state_ptr));
                return Err(e);
            }
        };

        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        Ok(())
    }
}

/// Registers the window class once per process.
fn register_class(instance: HINSTANCE) -> Result<()> {
    use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
    static REGISTERED: AtomicBool = AtomicBool::new(false);
    if REGISTERED.load(AtomicOrdering::Relaxed) {
        return Ok(());
    }
    unsafe {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(settings_proc),
            hInstance: instance,
            lpszClassName: SETTINGS_CLASS_NAME,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            // COLOR_WINDOW + 1 is the conventional dialog background brush.
            hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as isize as *mut c_void),
            ..Default::default()
        };
        // A failure here is fatal only if the class truly does not exist, which
        // the CreateWindowExW call below would then report anyway.
        let _ = RegisterClassW(&wc);
    }
    REGISTERED.store(true, AtomicOrdering::Relaxed);
    Ok(())
}

/// The shell's dialog font, so the window matches the rest of Windows at any DPI.
fn message_font() -> Option<HFONT> {
    unsafe {
        let mut ncm = NONCLIENTMETRICSW {
            cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32,
            ..Default::default()
        };
        SystemParametersInfoW(
            SPI_GETNONCLIENTMETRICS,
            ncm.cbSize,
            Some(&mut ncm as *mut _ as *mut c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .ok()?;
        let font = CreateFontIndirectW(&ncm.lfMessageFont);
        if font.0.is_null() {
            None
        } else {
            Some(font)
        }
    }
}

/// Closes the settings window if it is open.
pub fn close(app: &mut App) {
    if app.settings_hwnd.0.is_null() {
        return;
    }
    unsafe {
        let _ = DestroyWindow(app.settings_hwnd);
    }
    app.settings_hwnd = HWND::default();
}

pub fn is_open(app: &App) -> bool {
    !app.settings_hwnd.0.is_null()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_ids_are_unique_and_out_of_the_menu_range() {
        let ids = [
            ID_FOLDERS,
            ID_ADD,
            ID_REMOVE,
            ID_OPEN,
            ID_EXCLUDES,
            ID_EXCL_ADD,
            ID_EXCL_REMOVE,
            ID_RECURSIVE,
            ID_FOUND,
            ID_AUTOSTART,
            ID_AUTOPLAY,
            ID_PLAY_MODE,
            ID_VOLUME,
            ID_VOLUME_VALUE,
            ID_WIDTH,
            ID_HEIGHT,
            ID_CORNERS,
            ID_OPACITY,
            ID_OPACITY_VALUE,
            ID_SHOW_CONTROLS,
            ID_ZORDER,
            ID_MONITOR,
            ID_FONT,
            ID_LANGUAGE,
            ID_HOTKEYS,
            ID_HOTKEY_PRESET,
            ID_PATH_INFO,
            ID_NOTE,
            ID_DEFAULTS,
            ID_OPEN_CONFIG,
            ID_LEGACY,
            ID_CANCEL,
            ID_APPLY,
            ID_OK,
        ];
        let total = ids.len();
        let mut sorted = ids.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), total, "two settings controls share an id");
        // Settings ids must not collide with the menu's (1001..1085).
        assert!(
            ids.iter().all(|id| *id >= 2000),
            "id below the settings range"
        );
    }

    #[test]
    fn layout_fits_inside_the_client_area() {
        let rects: [(i32, i32, i32, i32); 7] = [
            (12, 10, 300, 150),   // folders group
            (12, 170, 300, 190),  // scan group
            (12, 370, 300, 150),  // general group
            (324, 10, 304, 106),  // startup group
            (324, 206, 304, 196), // appearance group
            (324, 412, 304, 68),  // font group
            (570, 582, 58, 30),   // OK button, the right-most control
        ];
        for (x, y, w, h) in rects {
            assert!(x >= 0 && y >= 0, "negative origin {x},{y}");
            assert!(x + w <= CLIENT_W, "control at x={x} w={w} overflows");
            assert!(y + h <= CLIENT_H, "control at y={y} h={h} overflows");
        }
        // The tallest left-hand group must stop above the footer row.
        let general_group_bottom = 370 + 150;
        let footer_top = 536;
        assert!(
            general_group_bottom < footer_top,
            "general group collides with the footer"
        );
    }

    #[test]
    fn index_lookup_is_total() {
        assert_eq!(index_of(config::PLAY_MODES, "shuffle"), 1);
        assert_eq!(index_of(config::MODES, "topmost"), 1);
        assert_eq!(index_of(config::LANGUAGES, "nonsense"), 0);
        assert_eq!(index_of(&[], "anything"), 0);
    }

    #[test]
    fn every_combo_source_matches_its_config_list() {
        // The combos index into these slices, so a mismatch would silently write
        // the wrong value.
        assert_eq!(config::PLAY_MODES.len(), 3);
        assert_eq!(config::SHOW_CONTROLS.len(), 2);
        assert_eq!(config::MODES.len(), 2);
        assert_eq!(config::LANGUAGES.len(), 3);
        assert_eq!(config::HOTKEY_PRESETS.len(), 2);
    }

    #[test]
    fn invalid_range_message_names_the_bounds() {
        let message = invalid_range(200, 900);
        assert!(message.contains("200..900"), "got {message}");
    }
}
