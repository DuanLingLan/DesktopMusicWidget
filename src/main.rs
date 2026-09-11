//! DesktopMusicWidget - the binary.
//!
//! This file is deliberately thin: parse arguments, handle the command-line-only
//! modes, then create the window and run the message loop. Everything testable
//! lives in the library.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use desktop_music_widget::{
    app::{self, App},
    audio::{self, Cmd, PlayMode, PlayerState},
    autostart, cli, config, dialogs, hotkeys, i18n, library, log, CLASS_NAME, INSTANCE_MUTEX,
    MSG_TASKBAR_CREATED, WINDOW_TITLE,
};

use std::path::PathBuf;
use std::sync::Arc;
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::{
            Console::{AttachConsole, ATTACH_PARENT_PROCESS},
            LibraryLoader::*,
            Threading::*,
        },
        UI::{HiDpi::*, WindowsAndMessaging::*},
    },
};

/// A GUI-subsystem process has no console of its own, so `--help` from a
/// terminal would otherwise print nowhere. Attaching to the parent console makes
/// the command-line modes usable.
fn attach_console() {
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

/// Writes to stdout without ever panicking.
///
/// `println!` panics when the write fails, and a GUI-subsystem process launched
/// with neither a console nor a redirection has no valid stdout handle - with
/// `panic = "abort"` that would kill the process silently, which for `--help` is
/// exactly the wrong outcome. `--version` and friends must never take the app
/// down.
fn out(text: &str) {
    use std::io::Write;
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(text.as_bytes());
    let _ = stdout.flush();
}

fn err(text: &str) {
    use std::io::Write;
    let mut stderr = std::io::stderr();
    let _ = stderr.write_all(text.as_bytes());
    let _ = stderr.write_all(b"\n");
    let _ = stderr.flush();
}

fn main() -> Result<()> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = match cli::parse(&raw) {
        Ok(args) => args,
        Err(message) => {
            attach_console();
            let text = format!("{message}\n\n{}", cli::usage());
            err(&text);
            dialogs::error(None, "DesktopMusicWidget", &text);
            std::process::exit(2);
        }
    };

    if args.help {
        attach_console();
        out(&format!("{}\n", cli::usage()));
        return Ok(());
    }
    if args.version {
        attach_console();
        out(&format!(
            "DesktopMusicWidget {}\n",
            env!("CARGO_PKG_VERSION")
        ));
        return Ok(());
    }

    // The config directory must be known before anything is loaded or logged.
    let dir = config::init_paths(&args);
    let log_path = log::init(&dir, args.verbose);
    log::install_panic_hook();
    i18n::init(&args.lang.clone().unwrap_or_else(|| "auto".to_string()));

    // --- command-line-only modes ------------------------------------------
    // These must run before the single-instance mutex is taken: they are
    // maintenance commands, not a second instance of the widget.
    if args.install_autostart {
        attach_console();
        autostart::install()?;
        out("added to the logon entry\n");
        return Ok(());
    }
    if args.remove_autostart {
        attach_console();
        autostart::remove()?;
        out("removed from the logon entry\n");
        return Ok(());
    }

    let loaded = config::load();
    // An explicit --lang wins over the stored setting for this run only.
    let language = args
        .lang
        .clone()
        .unwrap_or_else(|| loaded.cfg.language.clone());
    i18n::init(&language);
    let mut cfg = loaded.cfg;

    if args.reset_config {
        attach_console();
        let backup = config::reset()?;
        match backup {
            Some(p) => out(&format!(
                "config reset; previous file kept at {}\n",
                p.display()
            )),
            None => out("config reset\n"),
        }
        return Ok(());
    }

    if args.open_config {
        dialogs::open_path(&config::config_path());
        return Ok(());
    }

    if args.dump_config {
        attach_console();
        out(&config::to_toml(&cfg));
        return Ok(());
    }

    if args.count_tracks {
        attach_console();
        let mut opts = library::ScanOptions::from_config(&cfg);
        if let Some(dir) = &args.dir {
            opts.dirs = vec![dir.clone()];
        }
        out(&format!("{}\n", library::count_tracks(&opts)));
        return Ok(());
    }

    if let Some(on) = args.autoplay {
        cfg.autoplay = on;
    }

    unsafe { run(cfg, args.play, args.open_settings, log_path) }
}

unsafe fn run(
    cfg: config::Config,
    forced: Vec<PathBuf>,
    open_settings: bool,
    log_path: PathBuf,
) -> Result<()> {
    // GNU toolchains embed no manifest, so DPI awareness must be set in code.
    // Without this the widget is bitmap-stretched (blurry) at 150% scaling.
    let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

    let mutex = CreateMutexW(None, true, desktop_music_widget::MUTEX_NAME)?;
    if GetLastError() == ERROR_ALREADY_EXISTS {
        log::log_line(
            log::Level::Info,
            "another instance already owns the mutex; exiting",
        );
        return Ok(());
    }
    let _ = INSTANCE_MUTEX.set(mutex.0 as isize);

    let _ = MSG_TASKBAR_CREATED.set(RegisterWindowMessageW(w!("TaskbarCreated")));

    let hinstance: HINSTANCE = GetModuleHandleW(None)?.into();
    let wc = WNDCLASSW {
        lpfnWndProc: Some(app::wnd_proc),
        hInstance: hinstance,
        lpszClassName: CLASS_NAME,
        hCursor: LoadCursorW(None, IDC_ARROW)?,
        ..Default::default()
    };
    RegisterClassW(&wc);

    let dpi = GetDpiForSystem();

    let state = Arc::new(PlayerState::new(cfg.volume));
    let handle = audio::spawn(Arc::clone(&state));
    let scan = library::ScanState::new();
    let app = App::new_boxed(cfg, dpi, handle, state, scan)?;

    let hwnd = CreateWindowExW(
        WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        CLASS_NAME,
        WINDOW_TITLE,
        WS_POPUP | WS_VISIBLE,
        0,
        0,
        1,
        1,
        None,
        None,
        Some(hinstance),
        Some(Box::into_raw(app) as *const std::ffi::c_void),
    )?;

    // WM_CREATE has run by now, so the App is in GWLP_USERDATA with hwnd still 0.
    let app = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App);
    app.hwnd = hwnd;
    // GetDpiForSystem() only reports the *system* DPI (96 here) while this
    // monitor runs at 144. The window's real DPI is what decides how big the
    // card must be, so re-read it now that we have an HWND.
    app.dpi = GetDpiForWindow(hwnd);
    app.layout()?;
    app.install_tray();

    // Push the persisted playback settings into the audio thread before any
    // queue arrives, so autoplay is honoured for the very first track.
    app.audio
        .send(Cmd::SetPlayMode(PlayMode::from_config(&app.cfg.play_mode)));
    app.audio.send(Cmd::SetAutoplay(app.cfg.autoplay));
    app.audio.send(Cmd::Volume(app.cfg.volume));

    if app.cfg.hotkeys_enabled {
        let registration =
            hotkeys::register(hwnd, hotkeys::Preset::from_config(&app.cfg.hotkey_preset));
        if !registration.failed.is_empty() {
            log::log_line(
                log::Level::Warn,
                &format!("hotkeys already taken: {:?}", registration.failed),
            );
        }
    }

    // Reconcile the logon entry and warn about a leftover pre-rename one.
    app.reconcile_startup();

    // --- first library ------------------------------------------------------
    // `--play <path>...` bypasses the library walk, which makes it possible to
    // check specific tracks (e.g. ones with cover art) deterministically.
    if !forced.is_empty() {
        app.audio.send(Cmd::SetQueue(forced));
    } else if app.cfg.needs_library_setup() {
        app.first_run_if_needed();
        if app.cfg.needs_library_setup() {
            // Nothing was configured even after the wizard: start the walk
            // anyway so the status settles on "no folder configured".
            app.start_scan(library::ScanTarget::Play);
        }
    } else {
        app.start_scan(library::ScanTarget::Play);
    }

    // 250ms is 4Hz: smooth enough for a progress bar, cheap when idle.
    SetTimer(
        Some(hwnd),
        app::REDRAW_TIMER_ID,
        app::REDRAW_INTERVAL_MS,
        None,
    );

    if open_settings {
        desktop_music_widget::settings::open(app);
    }

    log::log_line(
        log::Level::Info,
        &format!("window ready; log at {}", log_path.display()),
    );

    let mut msg = MSG::default();
    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }

    // Nothing to clean up here: the card's WM_DESTROY removed the tray icon,
    // closed the settings window and freed the App.
    log::log_line(log::Level::Info, "message loop ended");
    Ok(())
}
