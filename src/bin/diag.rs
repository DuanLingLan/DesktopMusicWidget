//! Diagnostic: is the card actually sitting in the desktop band?
//!
//! Run this while the widget is running. It answers the one question that
//! matters for a **standalone** install (no Wallpaper Engine): is the card above
//! the desktop icon layer, or is Windows hiding it behind the wallpaper?
//!
//! The card must be ABOVE the desktop band or Wallpaper Engine would cover it,
//! and must have no ordinary windows below it or it would cover them.

use desktop_music_widget::app::{desktop_anchor_kind, monitors, AnchorKind};
use desktop_music_widget::{CLASS_NAME, SETTINGS_CLASS_NAME};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        UI::WindowsAndMessaging::{
            EnumChildWindows, EnumWindows, FindWindowW, GetClassNameW, GetClientRect,
            GetWindowLongPtrW, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId,
            IsWindowVisible, SendMessageW, GWL_EXSTYLE, WS_EX_TOOLWINDOW,
        },
    },
};

struct Entry {
    hwnd: HWND,
    title: String,
    class: String,
    ex: isize,
    pid: u32,
}

unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let out = &mut *(lparam.0 as *mut Vec<Entry>);
    if !IsWindowVisible(hwnd).as_bool() {
        return BOOL(1);
    }

    let mut name = [0u16; 128];
    let n = GetWindowTextW(hwnd, &mut name);
    let title = String::from_utf16_lossy(&name[..n as usize]);

    let mut class = [0u16; 128];
    let c = GetClassNameW(hwnd, &mut class);
    let class = String::from_utf16_lossy(&class[..c as usize]);

    let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));

    // Skip zero-size helper windows. Do NOT filter WS_EX_NOACTIVATE - that is the
    // widget's own style and would hide it from the list.
    let mut r = RECT::default();
    let _ = GetWindowRect(hwnd, &mut r);
    let tiny = (r.right - r.left) < 16 || (r.bottom - r.top) < 16;

    if !tiny {
        out.push(Entry {
            hwnd,
            title,
            class,
            ex,
            pid,
        });
    }
    BOOL(1)
}

fn main() {
    unsafe {
        let mut entries: Vec<Entry> = Vec::new();
        let _ = EnumWindows(Some(collect), LPARAM(&mut entries as *mut _ as isize));

        let widget = match FindWindowW(CLASS_NAME, None) {
            Ok(h) => h,
            Err(_) => {
                println!("widget window not found - is DesktopMusicWidget running?");
                report_anchor();
                report_monitors();
                report_settings();
                return;
            }
        };
        let widget_index = match entries.iter().position(|e| e.hwnd == widget) {
            Some(i) => i,
            None => {
                println!("widget not in the visible list (covered, or not yet shown?)");
                report_anchor();
                report_monitors();
                report_settings();
                return;
            }
        };

        println!("--- z-order, top first ---");
        for (i, e) in entries.iter().enumerate() {
            let mark = if i == widget_index { "<<< WIDGET" } else { "" };
            let tool = if (e.ex as u32 & WS_EX_TOOLWINDOW.0) != 0 {
                " tool"
            } else {
                ""
            };
            println!(
                "#{:<3} hwnd={:?} pid={:<6}{tool} class={:?} {:?} {}",
                i, e.hwnd, e.pid, e.class, e.title, mark
            );
        }

        println!("\n--- verdict ---");
        // EnumWindows walks top-to-bottom, so a LARGER index means LOWER in the
        // stack. The widget must sit above the desktop layer or the wallpaper
        // would hide it.
        //
        // The desktop band is identified by window class: Explorer uses "Progman"
        // and "WorkerW", and may also title a window "Program Manager" or
        // "FolderView". Wallpaper Engine's window is matched by title, since the
        // localised shell strings cannot be relied on.
        let desktop = entries.iter().position(|e| {
            e.class == "Progman"
                || e.class == "WorkerW"
                || e.title.contains("Program Manager")
                || e.title.contains("FolderView")
                || e.title.to_ascii_lowercase().contains("wallpaper")
        });
        match desktop {
            Some(di) if di > widget_index => println!(
                "OK   widget #{widget_index} is ABOVE the desktop band (#{di}) - the wallpaper cannot hide it"
            ),
            Some(di) => println!(
                "FAIL widget #{widget_index} is BELOW the desktop band (#{di}) - the wallpaper will cover it"
            ),
            None => println!("?    no desktop band window found; cannot judge"),
        }

        println!("\n--- windows ABOVE the widget (these cover it) ---");
        for e in entries.iter().take(widget_index) {
            println!("  hwnd={:?} class={:?} {:?}", e.hwnd, e.class, e.title);
        }

        report_anchor();
        report_monitors();
        report_settings();
    }
}

unsafe extern "system" fn count_child(_hwnd: HWND, lparam: LPARAM) -> BOOL {
    let count = &mut *(lparam.0 as *mut u32);
    *count += 1;
    BOOL(1)
}

unsafe extern "system" fn dump_child(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // A list box's window text is empty: its content lives in the item list, so
    // it has to be read back through LB_GETTEXT. The same applies to the
    // selected index of a combo box.
    const LB_GETCOUNT: u32 = 0x018B;
    const LB_GETTEXT: u32 = 0x0189;
    const LB_GETTEXTLEN: u32 = 0x018A;
    const CB_GETCURSEL: u32 = 0x0147;

    let out = &mut *(lparam.0 as *mut Vec<String>);
    let mut class = [0u16; 64];
    let c = GetClassNameW(hwnd, &mut class);
    let class = String::from_utf16_lossy(&class[..c as usize]);

    // `GetWindowTextW` deliberately refuses to fetch text from controls that live
    // in another process (it would have to send a message and could hang), so an
    // EDIT always reads back empty. Sending WM_GETTEXT explicitly works, and the
    // widget is a different process from this diagnostic.
    const WM_GETTEXT: u32 = 0x000D;
    let mut text = [0u16; 256];
    let len = SendMessageW(
        hwnd,
        WM_GETTEXT,
        Some(WPARAM(text.len())),
        Some(LPARAM(text.as_mut_ptr() as isize)),
    )
    .0
    .clamp(0, text.len() as isize) as usize;
    let mut line = format!("    [{class}] {}", String::from_utf16_lossy(&text[..len]));

    if class == "ListBox" {
        let count = SendMessageW(hwnd, LB_GETCOUNT, None, None).0;
        line.push_str(&format!(" ({count} items)"));
        for i in 0..count {
            let len = SendMessageW(hwnd, LB_GETTEXTLEN, Some(WPARAM(i as usize)), None).0;
            if len <= 0 {
                continue;
            }
            let mut buf = vec![0u16; len as usize + 1];
            SendMessageW(
                hwnd,
                LB_GETTEXT,
                Some(WPARAM(i as usize)),
                Some(LPARAM(buf.as_mut_ptr() as isize)),
            );
            line.push_str(&format!(
                "\n        - {}",
                String::from_utf16_lossy(&buf[..len as usize])
            ));
        }
    } else if class == "ComboBox" {
        let selected = SendMessageW(hwnd, CB_GETCURSEL, None, None).0;
        line.push_str(&format!(" (selected index {selected})"));
    }

    out.push(line);
    BOOL(1)
}

/// Prints every control with its class, caption and contents. This is the fastest
/// way to tell "the labels are wrong" apart from "the controls were never
/// created", and it also proves the lists and combos were actually populated.
fn report_settings_controls(hwnd: HWND) {
    let mut children: Vec<String> = Vec::new();
    unsafe {
        let _ = EnumChildWindows(
            Some(hwnd),
            Some(dump_child),
            LPARAM(&mut children as *mut Vec<String> as isize),
        );
    }
    for line in &children {
        println!("{line}");
    }
}

/// Reports the settings window when it is open. This is what to ask for when a
/// user says "the settings window is blank": it shows whether the controls were
/// actually created and how big the client area ended up at their DPI.
fn report_settings() {
    println!("\n--- settings window ---");
    let hwnd = unsafe { FindWindowW(SETTINGS_CLASS_NAME, None) };
    let Ok(hwnd) = hwnd else {
        println!("not open (start the app with --open-settings to check it)");
        return;
    };
    unsafe {
        let mut wr = RECT::default();
        let _ = GetWindowRect(hwnd, &mut wr);
        let mut cr = RECT::default();
        let _ = GetClientRect(hwnd, &mut cr);
        let mut children = 0u32;
        let _ = EnumChildWindows(
            Some(hwnd),
            Some(count_child),
            LPARAM(&mut children as *mut u32 as isize),
        );
        println!("hwnd: {hwnd:?}");
        println!(
            "outer: {}x{} at ({}, {})",
            wr.right - wr.left,
            wr.bottom - wr.top,
            wr.left,
            wr.top
        );
        println!("client: {}x{}", cr.right, cr.bottom);
        println!("child controls: {children}");
        if children < 30 {
            println!("WARN: fewer controls than expected (34 are laid out)");
        } else {
            println!("OK   all controls created");
        }
    }
    println!("--- controls ---");
    report_settings_controls(hwnd);
}

fn report_anchor() {
    let (anchor, kind) = desktop_anchor_kind();
    let description = match kind {
        AnchorKind::WallpaperEngine => "a Wallpaper Engine window",
        AnchorKind::DesktopBand => "the Explorer desktop band (Progman/WorkerW)",
        AnchorKind::None => "nothing - HWND_BOTTOM fallback",
    };
    println!("\n--- desktop anchor ---");
    println!("kind: {description}");
    println!("hwnd: {anchor:?}");
    match kind {
        AnchorKind::WallpaperEngine => {
            println!("note: Wallpaper Engine is running; the card is stacked above it")
        }
        AnchorKind::DesktopBand => {
            println!("note: no Wallpaper Engine found - this is the standalone path, and it works")
        }
        AnchorKind::None => {
            println!("WARN: no anchor found; the card may sit under the desktop icons")
        }
    }
}

fn report_monitors() {
    println!("\n--- monitors (menu order) ---");
    for (i, r) in monitors().iter().enumerate() {
        println!(
            "  {}: {}x{} at ({}, {}){}",
            i + 1,
            r.right - r.left,
            r.bottom - r.top,
            r.left,
            r.top,
            if r.left == 0 && r.top == 0 {
                "  <- primary"
            } else {
                ""
            }
        );
    }
}
