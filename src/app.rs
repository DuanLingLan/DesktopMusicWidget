//! Application state and window-message handling.

use crate::audio::{AudioHandle, Cmd, PlayMode, PlayerState};
use crate::config::{self, Config};
use crate::dialogs;
use crate::hotkeys::{self, Preset};
use crate::i18n::{self, tr, tr_num, Key};
use crate::library::{self, ScanOptions, ScanState, ScanStatus, ScanTarget};
use crate::menu::{self, Action};
use crate::render::{Frame, Surface, SurfaceStyle};
use crate::{art, autostart, log_error, log_info, log_warn};

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Graphics::Gdi::{
            EnumDisplayMonitors, GetMonitorInfoW, ScreenToClient, HDC, HMONITOR, MONITORINFO,
        },
        System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION,
        },
        UI::{
            HiDpi::GetDpiForWindow,
            Input::KeyboardAndMouse::*,
            Shell::{
                Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD,
                NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
            },
            WindowsAndMessaging::*,
        },
    },
};

pub const WM_TRAY: u32 = WM_APP + 1;
/// windows-rs only exposes this under the very large `Win32_UI_Controls` module,
/// so define it locally (0x02A3).
const WM_MOUSELEAVE: u32 = 0x02A3;
const WM_DISPLAYCHANGE: u32 = 0x007E;
const WM_DPICHANGED: u32 = 0x02E0;
const REDRAW_TIMER: usize = 1;
/// Exposed so `main` can arm the redraw timer after the window exists. 250ms is
/// 4Hz: smooth enough for a progress bar, cheap when idle.
pub const REDRAW_INTERVAL_MS: u32 = 250;
pub const REDRAW_TIMER_ID: usize = REDRAW_TIMER;
const TRAY_UID: u32 = 1;

/// Physical pixels -> logical (96-dpi) units, which is what the layout uses.
fn scale_from_physical(px: i32, dpi: u32) -> f32 {
    px as f32 * 96.0 / dpi as f32
}

pub fn primary_monitor_rect() -> RECT {
    unsafe {
        RECT {
            left: 0,
            top: 0,
            right: GetSystemMetrics(SM_CXSCREEN),
            bottom: GetSystemMetrics(SM_CYSCREEN),
        }
    }
}

unsafe extern "system" fn enum_monitor_proc(
    monitor: HMONITOR,
    _hdc: HDC,
    _clip: *mut RECT,
    data: LPARAM,
) -> BOOL {
    let out = &mut *(data.0 as *mut Vec<RECT>);
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if GetMonitorInfoW(monitor, &mut info).as_bool() {
        out.push(info.rcMonitor);
    }
    TRUE
}

/// Every monitor, in a stable left-to-right, top-to-bottom order so "Monitor 2"
/// means the same thing between runs.
pub fn monitors() -> Vec<RECT> {
    let mut out: Vec<RECT> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(enum_monitor_proc),
            LPARAM(&mut out as *mut Vec<RECT> as isize),
        );
    }
    out.sort_by_key(|r| (r.left, r.top));
    if out.is_empty() {
        out.push(primary_monitor_rect());
    }
    out
}

/// The rectangle the card is positioned inside.
fn monitor_rect(index: i32) -> RECT {
    let all = monitors();
    if index < 0 {
        // "Primary" is whichever monitor sits at the origin; if none does (an
        // unusual virtual-desktop layout) fall back to the first.
        return all
            .iter()
            .find(|r| r.left == 0 && r.top == 0)
            .copied()
            .unwrap_or(all[0]);
    }
    all.get(index as usize).copied().unwrap_or(all[0])
}

pub struct App {
    pub hwnd: HWND,
    pub dpi: u32,
    pub x: i32,
    pub y: i32,
    pub cfg: Config,
    pub surface: Surface,
    pub frame: Frame,
    pub audio: AudioHandle,
    pub state: Arc<PlayerState>,
    pub scan: Arc<ScanState>,
    /// Zero while the settings window is closed.
    pub settings_hwnd: HWND,
    last_gen: u64,
    last_progress_bucket: u32,
    last_playing: bool,
    last_status: Option<String>,
    last_tip: String,
    wic: Option<art::Wic>,
    /// Path the current cover was decoded from, so we do not re-decode on every
    /// progress tick.
    art_for: PathBuf,
    current_title: String,
    current_artist: String,
    hovering: bool,
    tracking: bool,
    seeking: bool,
    /// "Move window" mode: a plain left-drag repositions the card.
    pub moving: bool,
    dragging: bool,
    drag_cursor: (i32, i32),
    drag_window: (i32, i32),
    tray: NOTIFYICONDATAW,
    tray_installed: bool,
    notified_no_audio: bool,
    notified_legacy: bool,
    notified_moving: bool,
}

impl App {
    pub fn new_boxed(
        cfg: Config,
        dpi: u32,
        audio: AudioHandle,
        state: Arc<PlayerState>,
        scan: Arc<ScanState>,
    ) -> Result<Box<Self>> {
        let style = style_for(&cfg);
        Ok(Box::new(Self {
            hwnd: HWND::default(),
            dpi,
            x: 0,
            y: 0,
            cfg,
            surface: Surface::new(1, 1, dpi as f32, style)?,
            frame: Frame {
                title: tr(Key::Loading).to_string(),
                artist: String::new(),
                progress: None,
                art: None,
                show_controls: false,
                playing: false,
            },
            audio,
            state,
            scan,
            settings_hwnd: HWND::default(),
            last_gen: 0,
            last_progress_bucket: 0,
            last_playing: false,
            last_status: None,
            last_tip: String::new(),
            wic: art::Wic::new().ok(),
            art_for: PathBuf::new(),
            current_title: String::new(),
            current_artist: String::new(),
            hovering: false,
            tracking: false,
            seeking: false,
            moving: false,
            dragging: false,
            drag_cursor: (0, 0),
            drag_window: (0, 0),
            tray: NOTIFYICONDATAW::default(),
            tray_installed: false,
            notified_no_audio: false,
            notified_legacy: false,
            notified_moving: false,
        }))
    }

    // -----------------------------------------------------------------------
    // Geometry
    // -----------------------------------------------------------------------

    /// Converts a physical pixel coordinate from a mouse message into logical
    /// (96-dpi) units, which is what the layout uses.
    pub fn to_logical(&self, px: i32) -> f32 {
        scale_from_physical(px, self.dpi)
    }

    /// Card geometry for hit-testing. (Distinct from `layout()`, which positions
    /// the window on screen.)
    pub fn card_layout(&self) -> crate::render::Layout {
        crate::render::layout_for(self.cfg.width as f32, self.cfg.height as f32)
    }

    pub fn phys(&self, logical: i32) -> i32 {
        (logical as f32 * self.dpi as f32 / 96.0).round() as i32
    }

    pub fn phys_f(&self, logical: f32) -> f32 {
        logical * self.dpi as f32 / 96.0
    }

    fn seek_to(&mut self, logical_x: f32) {
        let lay = self.card_layout();
        let span = lay.progress.right - lay.progress.left;
        if span > 0.0 {
            let f = ((logical_x - lay.progress.left) / span).clamp(0.0, 1.0);
            self.audio.send(Cmd::SeekFraction(f));
        }
    }

    /// Positions and sizes the window for the current config and DPI.
    pub fn layout(&mut self) -> Result<()> {
        let w = self.phys(self.cfg.width);
        let h = self.phys(self.cfg.height);
        let area = monitor_rect(self.cfg.monitor);
        let area_w = area.right - area.left;

        let x = match self.cfg.anchor.as_str() {
            "top-left" | "free" => area.left + self.phys(self.cfg.offset_x),
            "top-right" => area.right - w - self.phys(self.cfg.offset_x),
            _ => area.left + (area_w - w) / 2 + self.phys(self.cfg.offset_x),
        };
        let y = area.top + self.phys(self.cfg.offset_y);

        self.x = x;
        self.y = y;
        if self.surface.w != w || self.surface.h != h {
            self.surface.resize(w, h, self.dpi as f32)?;
        }
        unsafe {
            SetWindowPos(self.hwnd, None, x, y, w, h, SWP_NOACTIVATE | SWP_NOZORDER)?;
        }
        self.apply_z_order();
        self.redraw();
        Ok(())
    }

    pub fn apply_z_order(&self) {
        unsafe {
            if self.cfg.mode == "topmost" {
                let _ = SetWindowPos(
                    self.hwnd,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
                return;
            }

            // Desktop layer: sit directly above whatever draws the wallpaper, so
            // the card is above the icons' background but below normal windows.
            match desktop_anchor() {
                Some(anchor) => {
                    let _ = SetWindowPos(
                        self.hwnd,
                        Some(anchor),
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                    );
                }
                None => {
                    let _ = SetWindowPos(
                        self.hwnd,
                        Some(HWND_BOTTOM),
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                    );
                }
            }
        }
    }
    fn sync_surface_style(&mut self) -> Result<()> {
        self.surface.set_style(style_for(&self.cfg))
    }

    // -----------------------------------------------------------------------
    // Drawing
    // -----------------------------------------------------------------------

    pub fn redraw(&mut self) {
        let radius = self.cfg.corner_radius;
        let opacity = self.cfg.card_opacity;
        self.frame.show_controls = match self.cfg.show_controls.as_str() {
            "always" => true,
            _ => self.hovering,
        };
        self.frame.playing = self.state.playing.load(Ordering::Relaxed);
        if let Err(e) = self.surface.draw(&self.frame, opacity, radius) {
            log_error!("draw failed: {e}");
            return;
        }
        let _ = self.surface.present(self.hwnd, self.x, self.y);
    }

    /// The line shown under the title when something is wrong or not set up yet.
    fn status_line(&self) -> Option<String> {
        if self.state.device_error.load(Ordering::Relaxed) {
            return Some(tr(Key::StatusNoAudioDevice).to_string());
        }
        match self.scan.status() {
            ScanStatus::NoDirs => Some(tr(Key::StatusNoLibrary).to_string()),
            ScanStatus::Empty => Some(tr(Key::StatusEmptyLibrary).to_string()),
            ScanStatus::Scanning { .. } if self.current_title.is_empty() => {
                Some(tr(Key::StatusScanning).to_string())
            }
            _ => None,
        }
    }

    /// Pulls the latest playback state and redraws only when something visible
    /// changed, so an idle widget costs nothing.
    pub fn poll_state(&mut self) {
        let gen = self.state.generation.load(Ordering::Relaxed);
        let dur_ms = self.state.dur_ms.load(Ordering::Relaxed);
        let pos_ms = self.state.pos_ms.load(Ordering::Relaxed);
        let playing = self.state.playing.load(Ordering::Relaxed);

        let progress = if dur_ms > 0 {
            Some((pos_ms as f32 / dur_ms as f32).clamp(0.0, 1.0))
        } else {
            None
        };
        // Redraw once the bar has moved ~1% of its width.
        let bucket = (progress.unwrap_or(0.0) * 100.0) as u32;
        let status = self.status_line();

        // Tell the user once when there is no output device, rather than leaving
        // them with a silent widget and no explanation.
        if self.state.device_error.load(Ordering::Relaxed) && !self.notified_no_audio {
            self.notified_no_audio = true;
            self.notify(tr(Key::TrayNoAudioTitle), tr(Key::TrayNoAudioText));
        }

        let track_changed = gen != self.last_gen;
        // The transport glyph depends on this, so a click must repaint even when
        // the progress bar has not moved.
        let playing_changed = playing != self.last_playing;
        // A status change (library finished, device appeared) must repaint too.
        let status_changed = status != self.last_status;

        if !track_changed
            && !playing_changed
            && !status_changed
            && bucket == self.last_progress_bucket
        {
            return;
        }
        self.last_gen = gen;
        self.last_playing = playing;
        self.last_progress_bucket = bucket;
        self.last_status = status.clone();

        if track_changed {
            let (title, artist, path) = match self.state.now.lock() {
                Ok(n) => (n.title.clone(), n.artist.clone(), n.path.clone()),
                Err(_) => (String::new(), String::new(), PathBuf::new()),
            };
            self.current_title = title;
            self.current_artist = artist;

            // Also goes to the log: the first question in any bug report is
            // "what were you playing?".
            log_info!(
                "now playing: {} - {}",
                if self.current_title.is_empty() {
                    "(none)"
                } else {
                    &self.current_title
                },
                if self.current_artist.is_empty() {
                    "(no artist tag)"
                } else {
                    &self.current_artist
                }
            );

            // Decode cover art for the new track only.
            if path != self.art_for {
                self.art_for = path.clone();
                self.frame.art = if path.as_os_str().is_empty() {
                    None
                } else {
                    let size = (self.phys(self.cfg.height - 20) as u32).max(32);
                    self.wic.as_ref().and_then(|w| w.art_for_track(&path, size))
                };
            }
        }

        self.frame.title = if self.current_title.is_empty() {
            tr(Key::NotPlaying).to_string()
        } else {
            self.current_title.clone()
        };
        self.frame.artist = match &status {
            Some(s) => s.clone(),
            None => self.current_artist.clone(),
        };
        self.frame.progress = progress;

        self.update_tray_tip();
        self.redraw();
    }

    // -----------------------------------------------------------------------
    // Tray
    // -----------------------------------------------------------------------

    fn fill_tip(&mut self) {
        let text = if self.current_title.is_empty() {
            match self.scan.status() {
                ScanStatus::NoDirs => tr(Key::StatusNoLibrary).to_string(),
                ScanStatus::Empty => tr(Key::StatusEmptyLibrary).to_string(),
                ScanStatus::Scanning { .. } => tr(Key::StatusScanning).to_string(),
                _ => tr(Key::TrayTip).to_string(),
            }
        } else if self.current_artist.is_empty() {
            self.current_title.clone()
        } else {
            format!("{} — {}", self.current_title, self.current_artist)
        };
        let wide: Vec<u16> = text.encode_utf16().collect();
        let n = wide.len().min(self.tray.szTip.len() - 1);
        self.tray.szTip[..n].copy_from_slice(&wide[..n]);
        for slot in self.tray.szTip[n..].iter_mut() {
            *slot = 0;
        }
    }

    pub fn update_tray_tip(&mut self) {
        if !self.tray_installed {
            return;
        }
        self.fill_tip();
        let tip: String = String::from_utf16_lossy(
            &self.tray.szTip[..self
                .tray
                .szTip
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(self.tray.szTip.len())],
        );
        if tip == self.last_tip {
            return;
        }
        self.last_tip = tip;
        self.tray.uFlags = NIF_TIP;
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &self.tray);
        }
    }

    pub fn install_tray(&mut self) {
        let nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: TRAY_UID,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: WM_TRAY,
            hIcon: art::tray_icon()
                .unwrap_or_else(|| unsafe { LoadIconW(None, IDI_APPLICATION).unwrap_or_default() }),
            ..Default::default()
        };
        self.tray = nid;
        self.fill_tip();
        self.tray_installed = unsafe { Shell_NotifyIconW(NIM_ADD, &self.tray).as_bool() };
        // NOTE: deliberately not switching to NOTIFYICON_VERSION_4. Under that
        // version the callback reports NIN_SELECT / WM_CONTEXTMENU instead of
        // WM_LBUTTONUP / WM_RBUTTONUP, so the classic messages (which this code
        // handles, and which provide a real double-click) would never arrive.
        if !self.tray_installed {
            log_warn!("could not add the tray icon");
        }
    }

    pub fn remove_tray(&mut self) {
        if !self.tray_installed {
            return;
        }
        let nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: TRAY_UID,
            ..Default::default()
        };
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
        }
        self.tray_installed = false;
    }

    /// Shows a balloon once per reason, so a recurring condition does not nag.
    pub fn notify(&mut self, title: &str, text: &str) {
        if !self.tray_installed {
            return;
        }
        let mut nid = self.tray;
        nid.uFlags = NIF_INFO;
        nid.dwInfoFlags = NIIF_INFO;
        fill_wide(&mut nid.szInfoTitle, title);
        fill_wide(&mut nid.szInfo, text);
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
        }
    }

    // -----------------------------------------------------------------------
    // Actions
    // -----------------------------------------------------------------------

    pub fn save_config(&mut self) {
        if let Err(e) = config::save(&self.cfg) {
            log_error!("cannot save the config: {e}");
        }
    }

    pub fn show_menu(&mut self) {
        if let Some(action) = menu::show_at_cursor(self) {
            self.perform(action);
        }
        // `TrackPopupMenu` requires the card to be the foreground window, and
        // `SetForegroundWindow` lifts a window to the top of its band — so the
        // card has to be put back into the desktop layer as soon as the menu
        // closes, or it stays floating above ordinary application windows.
        // Doing it here, once per menu, rather than on a timer keeps an idle
        // widget at zero cost.
        self.apply_z_order();
    }

    fn set_volume(&mut self, volume: u32) {
        let v = volume.min(100);
        self.cfg.volume = v;
        self.audio.send(Cmd::Volume(v));
        self.save_config();
    }

    pub fn set_autostart(&mut self, on: bool) {
        let result = if on {
            autostart::install()
        } else {
            autostart::remove()
        };
        match result {
            Ok(()) => {
                self.cfg.autostart = on;
                self.save_config();
            }
            Err(e) => {
                log_error!("cannot change the autostart entry: {e}");
                // Leave the config untouched so the checkbox does not lie about
                // a registry write that did not happen.
                self.cfg.autostart = autostart::is_installed();
            }
        }
    }

    fn add_folders(&mut self) {
        let start = self
            .cfg
            .library_paths()
            .into_iter()
            .next()
            .or_else(dialogs::default_music_dir);
        let picked =
            dialogs::pick_folders(None, tr(Key::DlgPickFolderTitle), start.as_deref(), true);
        if picked.is_empty() {
            return;
        }
        for p in picked {
            let text = p.to_string_lossy().to_string();
            if !self.cfg.library.contains(&text) {
                self.cfg.library.push(text);
            }
        }
        self.on_library_changed();
    }

    fn replace_folders(&mut self) {
        let start = self
            .cfg
            .library_paths()
            .into_iter()
            .next()
            .or_else(dialogs::default_music_dir);
        let picked =
            dialogs::pick_folders(None, tr(Key::DlgPickFolderTitle), start.as_deref(), true);
        if picked.is_empty() {
            return;
        }
        self.cfg.library = picked
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        self.on_library_changed();
    }

    fn clear_folders(&mut self) {
        if self.cfg.library.is_empty() {
            return;
        }
        if !dialogs::confirm(None, tr(Key::MenuClearFolders), tr(Key::DlgFirstRunText)) {
            return;
        }
        self.cfg.library.clear();
        self.on_library_changed();
    }

    fn exclude_folder(&mut self) {
        let start = self
            .cfg
            .library_paths()
            .into_iter()
            .next()
            .or_else(dialogs::default_music_dir);
        let picked =
            dialogs::pick_folders(None, tr(Key::DlgPickExcludeTitle), start.as_deref(), true);
        if picked.is_empty() {
            return;
        }
        for p in picked {
            let text = p.to_string_lossy().to_string();
            if !self.cfg.exclude_dirs.contains(&text) {
                self.cfg.exclude_dirs.push(text);
            }
        }
        self.save_config();
        self.rescan();
    }

    /// Common tail for anything that changes which folders are scanned.
    fn on_library_changed(&mut self) {
        self.save_config();
        let opts = ScanOptions::from_config(&self.cfg);
        let count = library::count_tracks(&opts);
        if count == 0 {
            dialogs::warn(None, tr(Key::DlgFoundTitle), tr(Key::DlgFoundNoneText));
        }
        self.start_scan(ScanTarget::Play);
    }

    pub fn start_scan(&mut self, target: ScanTarget) {
        let opts = ScanOptions::from_config(&self.cfg);
        library::spawn(opts, target, self.audio.clone(), Arc::clone(&self.scan));
    }

    pub fn rescan(&mut self) {
        // Pool replacement keeps the current track playing.
        self.start_scan(ScanTarget::Pool);
    }

    pub fn open_config(&self) {
        dialogs::open_path(&config::config_path());
    }

    pub fn reload_config(&mut self) {
        let old = self.cfg.clone();
        let loaded = config::load();
        self.cfg = loaded.cfg;
        self.apply_config(&old);
        log_info!("config reloaded from disk");
    }

    pub fn show_about(&self) {
        let target = if cfg!(target_env = "gnu") {
            "x86_64-pc-windows-gnu"
        } else {
            "x86_64-pc-windows-msvc"
        };
        let config_path = config::config_path().display().to_string();
        let log_path = crate::log::path()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let text = i18n::tr_fmt(
            Key::DlgAboutText,
            &[
                env!("CARGO_PKG_VERSION"),
                target,
                &config_path,
                &log_path,
                "github.com/DuanLingLan/DesktopMusicWidget",
            ],
        );
        dialogs::info(None, tr(Key::DlgAboutTitle), &text);
    }

    pub fn toggle_move_mode(&mut self) {
        self.moving = !self.moving;
        if self.moving && !self.notified_moving {
            self.notified_moving = true;
            self.notify(tr(Key::DlgHintTitle), tr(Key::DlgMoveModeOn));
        }
    }

    fn persist_position(&mut self) {
        let all = monitors();
        let center = (self.x + self.surface.w / 2, self.y + self.surface.h / 2);
        let index = all
            .iter()
            .position(|r| {
                center.0 >= r.left && center.0 < r.right && center.1 >= r.top && center.1 < r.bottom
            })
            .unwrap_or(0);
        let area = all.get(index).copied().unwrap_or_else(primary_monitor_rect);
        self.cfg.monitor = index as i32;
        self.cfg.anchor = "free".into();
        self.cfg.offset_x = scale_from_physical(self.x - area.left, self.dpi).round() as i32;
        self.cfg.offset_y = scale_from_physical(self.y - area.top, self.dpi).round() as i32;
        self.save_config();
    }

    /// Applies everything that can change without a restart, comparing against
    /// the previous config so only the affected subsystems are touched.
    pub fn apply_config(&mut self, old: &Config) {
        if old.language != self.cfg.language {
            i18n::init(&self.cfg.language);
        }

        self.audio
            .send(Cmd::SetPlayMode(PlayMode::from_config(&self.cfg.play_mode)));
        self.audio.send(Cmd::SetAutoplay(self.cfg.autoplay));
        self.audio.send(Cmd::Volume(self.cfg.volume));

        if old.hotkeys_enabled != self.cfg.hotkeys_enabled
            || old.hotkey_preset != self.cfg.hotkey_preset
        {
            if self.cfg.hotkeys_enabled {
                let result =
                    hotkeys::register(self.hwnd, Preset::from_config(&self.cfg.hotkey_preset));
                if !result.failed.is_empty() {
                    log_warn!("hotkeys already taken: {:?}", result.failed);
                }
            } else {
                hotkeys::unregister(self.hwnd);
            }
        }

        if old.autostart != self.cfg.autostart {
            let _ = if self.cfg.autostart {
                autostart::install()
            } else {
                autostart::remove()
            };
        }

        if ScanOptions::from_config(old) != ScanOptions::from_config(&self.cfg) {
            self.rescan();
        }

        if let Err(e) = self.sync_surface_style() {
            log_error!("cannot apply the theme: {e}");
        }
        if let Err(e) = self.layout() {
            log_error!("cannot relayout: {e}");
        }
        // A language change alters the card's fallback text.
        self.current_title.clear();
        self.current_artist.clear();
        self.last_gen = u64::MAX;
        self.poll_state();
        self.update_tray_tip();
        self.save_config();
    }

    pub fn perform(&mut self, action: Action) {
        match action {
            Action::TogglePlay => self.audio.send(Cmd::TogglePause),
            Action::Prev => self.audio.send(Cmd::Prev),
            Action::Next => self.audio.send(Cmd::Next),
            Action::PlayMode(mode) => {
                self.cfg.play_mode = mode.as_str().to_string();
                self.audio.send(Cmd::SetPlayMode(mode));
                self.save_config();
            }
            Action::AddFolder => self.add_folders(),
            Action::ReplaceFolder => self.replace_folders(),
            Action::OpenFirstFolder => {
                if let Some(first) = self.cfg.library_paths().into_iter().next() {
                    if first.is_dir() {
                        dialogs::open_in_explorer(&first);
                    } else {
                        let text = i18n::tr_fmt(
                            Key::DlgFolderMissingText,
                            &[&first.display().to_string()],
                        );
                        dialogs::warn(None, tr(Key::DlgFolderMissingTitle), &text);
                    }
                }
            }
            Action::ClearFolders => self.clear_folders(),
            Action::ToggleRecursive => {
                self.cfg.scan_recursive = !self.cfg.scan_recursive;
                self.save_config();
                self.rescan();
            }
            Action::ExcludeFolder => self.exclude_folder(),
            Action::Rescan => self.rescan(),
            Action::ToggleAutostart => {
                let on = !self.cfg.autostart;
                self.set_autostart(on);
            }
            Action::ToggleAutoplay => {
                self.cfg.autoplay = !self.cfg.autoplay;
                self.audio.send(Cmd::SetAutoplay(self.cfg.autoplay));
                self.save_config();
            }
            Action::ShowControls(value) => {
                self.cfg.show_controls = value.to_string();
                self.save_config();
                self.redraw();
            }
            Action::WindowMode(value) => {
                self.cfg.mode = value.to_string();
                self.save_config();
                self.apply_z_order();
            }
            Action::Monitor(index) => {
                self.cfg.monitor = index;
                self.save_config();
                if let Err(e) = self.layout() {
                    log_error!("cannot move to monitor {index}: {e}");
                }
            }
            Action::ToggleMove => self.toggle_move_mode(),
            Action::Settings => crate::settings::open(self),
            Action::OpenConfig => self.open_config(),
            Action::ReloadConfig => self.reload_config(),
            Action::About => self.show_about(),
            Action::Exit => unsafe {
                let _ = PostMessageW(Some(self.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            },
        }
    }

    /// First run (or a library that vanished): ask for a folder before doing
    /// anything else, so the card never sits on "loading" with nothing to play.
    pub fn first_run_if_needed(&mut self) {
        if !self.cfg.needs_library_setup() {
            return;
        }
        let fallback = dialogs::default_music_dir();

        loop {
            let picked =
                dialogs::pick_folders(None, tr(Key::DlgFirstRunTitle), fallback.as_deref(), true);
            let mut dirs: Vec<PathBuf> = picked;
            if dirs.is_empty() {
                match &fallback {
                    Some(d) => dirs.push(d.clone()),
                    None => {
                        log_info!("first run cancelled and no default music folder exists");
                        return;
                    }
                }
            }

            self.cfg.library = dirs
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect();
            let count = library::count_tracks(&ScanOptions::from_config(&self.cfg));

            if count == 0 {
                // Ask whether to try again; declining keeps the choice anyway so
                // the user is not stuck in the wizard.
                let again =
                    dialogs::confirm(None, tr(Key::DlgFoundTitle), tr(Key::DlgFoundNoneText));
                self.save_config();
                if again {
                    continue;
                }
                break;
            }

            let text = tr_num(Key::DlgFoundText, count);
            if !dialogs::confirm(None, tr(Key::DlgFoundTitle), &text) {
                // They kept the folders but do not want playback to start on its
                // own; the setting is visible in the menu.
                self.cfg.autoplay = false;
                self.audio.send(Cmd::SetAutoplay(false));
            }
            self.save_config();
            break;
        }

        self.start_scan(ScanTarget::Play);
    }

    /// Reconciles the registry and warns about a leftover entry from the
    /// pre-rename build. Called once, after the tray exists.
    pub fn reconcile_startup(&mut self) {
        if self.cfg.autostart {
            match autostart::ensure(true) {
                Ok(true) => log_info!("autostart entry repaired"),
                Ok(false) => {}
                Err(e) => log_warn!("cannot repair the autostart entry: {e}"),
            }
        }
        if !self.notified_legacy && autostart::legacy_installed() {
            self.notified_legacy = true;
            self.notify(tr(Key::TrayLegacyTitle), tr(Key::TrayLegacyText));
        }
    }

    fn on_hotkey(&mut self, id: i32) {
        match hotkeys::action_for(id) {
            Some(hotkeys::HotkeyAction::TogglePause) => self.audio.send(Cmd::TogglePause),
            Some(hotkeys::HotkeyAction::Next) => self.audio.send(Cmd::Next),
            Some(hotkeys::HotkeyAction::Prev) => self.audio.send(Cmd::Prev),
            Some(hotkeys::HotkeyAction::VolumeUp) => {
                let next = (self.cfg.volume as i32 + 5).clamp(0, 100) as u32;
                self.set_volume(next);
            }
            Some(hotkeys::HotkeyAction::VolumeDown) => {
                let next = (self.cfg.volume as i32 - 5).clamp(0, 100) as u32;
                self.set_volume(next);
            }
            None => {}
        }
    }

    fn on_tray_message(&mut self, lparam: LPARAM) {
        let event = (lparam.0 & 0xffff) as u32;
        match event {
            WM_RBUTTONUP | WM_CONTEXTMENU => self.show_menu(),
            WM_LBUTTONDBLCLK => crate::settings::open(self),
            WM_LBUTTONUP => self.audio.send(Cmd::TogglePause),
            _ => {}
        }
    }

    fn begin_drag(&mut self) {
        let mut pt = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut pt);
            SetCapture(self.hwnd);
        }
        self.dragging = true;
        self.drag_cursor = (pt.x, pt.y);
        self.drag_window = (self.x, self.y);
    }

    fn update_drag(&mut self) {
        let mut pt = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut pt);
        }
        self.x = self.drag_window.0 + (pt.x - self.drag_cursor.0);
        self.y = self.drag_window.1 + (pt.y - self.drag_cursor.1);
        // UpdateLayeredWindow places the window, so re-presenting is the move.
        self.redraw();
    }

    fn end_drag(&mut self) {
        self.dragging = false;
        unsafe {
            let _ = ReleaseCapture();
        }
        self.persist_position();
    }

    pub fn on_message(&mut self, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        match msg {
            WM_NCHITTEST => {
                let sx = (lparam.0 & 0xffff) as i16 as i32;
                let sy = ((lparam.0 >> 16) & 0xffff) as i16 as i32;
                let mut pt = POINT { x: sx, y: sy };
                unsafe {
                    let _ = ScreenToClient(self.hwnd, &mut pt);
                }
                let hw = self.surface.w as f32 / 2.0;
                let hh = self.surface.h as f32 / 2.0;
                let d = Surface::rounded_rect_sdf(
                    pt.x as f32 + 0.5 - hw,
                    pt.y as f32 + 0.5 - hh,
                    hw,
                    hh,
                    self.phys_f(self.cfg.corner_radius),
                );
                // Best-effort: let events fall through outside the card. Only
                // reliable for the small rounded corners, which is why the window
                // is kept tight to the card bounds instead.
                LRESULT(if d <= 0.0 {
                    HTCLIENT as isize
                } else {
                    HTTRANSPARENT as isize
                })
            }
            WM_MOUSEMOVE => {
                if self.dragging {
                    self.update_drag();
                    return LRESULT(0);
                }
                let cx = self.to_logical((lparam.0 & 0xffff) as i16 as i32);
                if self.seeking {
                    self.seek_to(cx);
                    return LRESULT(0);
                }
                if !self.tracking {
                    unsafe {
                        let mut tme = TRACKMOUSEEVENT {
                            cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                            dwFlags: TME_LEAVE,
                            hwndTrack: self.hwnd,
                            dwHoverTime: 0,
                        };
                        let _ = TrackMouseEvent(&mut tme);
                        self.tracking = true;
                    }
                }
                if !self.hovering {
                    self.hovering = true;
                    self.redraw();
                }
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                self.tracking = false;
                if self.hovering {
                    self.hovering = false;
                    self.redraw();
                }
                LRESULT(0)
            }
            // Never take focus — we must stay below application windows.
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            WM_MOUSEWHEEL => {
                // High word of wparam is the wheel delta; one notch is 120.
                let delta = (((wparam.0 >> 16) & 0xffff) as i16) as i32;
                let steps = delta / 120;
                if steps != 0 {
                    let cur = self.state.volume.load(Ordering::Relaxed) as i32;
                    let next = (cur + steps * 5).clamp(0, 100) as u32;
                    self.set_volume(next);
                }
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                // Move mode, or Ctrl held, repositions the card instead of
                // triggering a control.
                let ctrl = unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0;
                if self.moving || ctrl {
                    self.begin_drag();
                    return LRESULT(0);
                }
                let cx = self.to_logical((lparam.0 & 0xffff) as i16 as i32);
                let cy = self.to_logical(((lparam.0 >> 16) & 0xffff) as i16 as i32);
                match self.card_layout().hit(cx, cy) {
                    Some(0) => self.audio.send(Cmd::Prev),
                    Some(1) => self.audio.send(Cmd::TogglePause),
                    Some(2) => self.audio.send(Cmd::Next),
                    Some(3) => {
                        self.seeking = true;
                        unsafe {
                            SetCapture(self.hwnd);
                        }
                        self.seek_to(cx);
                    }
                    _ => {}
                }
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                if self.dragging {
                    self.end_drag();
                } else if self.seeking {
                    self.seeking = false;
                    unsafe {
                        let _ = ReleaseCapture();
                    }
                }
                LRESULT(0)
            }
            WM_CONTEXTMENU => {
                self.show_menu();
                LRESULT(0)
            }
            WM_HOTKEY => {
                self.on_hotkey(wparam.0 as i32);
                LRESULT(0)
            }
            WM_TRAY => {
                self.on_tray_message(lparam);
                LRESULT(0)
            }
            // Fires when the monitor resolution or DPI changes (e.g. dragging the
            // window to another monitor, or a scaling change). The DIB and render
            // target are sized in physical pixels, so both must be rebuilt.
            WM_DISPLAYCHANGE | WM_DPICHANGED => {
                let new_dpi = unsafe { GetDpiForWindow(self.hwnd) };
                if new_dpi != self.dpi || self.surface.w != self.phys(self.cfg.width) {
                    self.dpi = new_dpi;
                    if let Err(e) = self.layout() {
                        log_error!("relayout after a DPI change failed: {e}");
                    }
                }
                self.apply_z_order();
                LRESULT(0)
            }
            WM_WINDOWPOSCHANGING => {
                // Belt and braces: refuse to be hidden outright. The desktop being
                // raised over the card is handled by `restore_z_order_if_covered`;
                // this only catches a shell that tries to hide the window.
                unsafe {
                    let pos = &mut *(lparam.0 as *mut WINDOWPOS);
                    let flags = pos.flags.0;
                    if flags & SWP_HIDEWINDOW.0 != 0 {
                        pos.flags = SET_WINDOW_POS_FLAGS(flags & !SWP_HIDEWINDOW.0);
                    }
                    DefWindowProcW(self.hwnd, msg, wparam, lparam)
                }
            }
            WM_TIMER => {
                self.poll_state();
                LRESULT(0)
            }
            _ => unsafe { DefWindowProcW(self.hwnd, msg, wparam, lparam) },
        }
    }
}

/// Copies `text` into a fixed-size NUL-terminated wide buffer.
pub fn fill_wide(dst: &mut [u16], text: &str) {
    let wide: Vec<u16> = text.encode_utf16().collect();
    let n = wide.len().min(dst.len().saturating_sub(1));
    dst[..n].copy_from_slice(&wide[..n]);
    for slot in dst[n..].iter_mut() {
        *slot = 0;
    }
}

pub fn style_for(cfg: &Config) -> SurfaceStyle {
    SurfaceStyle {
        palette: crate::theme::Palette::resolve(&cfg.theme),
        font_family: cfg.font_family.clone(),
    }
}

unsafe extern "system" fn enum_desktop_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let (we, shell) = &mut *(lparam.0 as *mut (Vec<HWND>, Vec<HWND>));
    if !IsWindowVisible(hwnd).as_bool() {
        return TRUE;
    }

    // Wallpaper Engine's own windows.
    let mut pid: u32 = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid != 0 {
        if let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            let mut buf = [0u16; 260];
            let mut len = buf.len() as u32;
            if QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_WIN32,
                PWSTR(buf.as_mut_ptr()),
                &mut len,
            )
            .is_ok()
            {
                let name = String::from_utf16_lossy(&buf[..len as usize]);
                let file = name
                    .rsplit(['\\', '/'])
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if file.starts_with("wallpaper") {
                    we.push(hwnd);
                }
            }
            let _ = CloseHandle(handle);
        }
    }

    // The Explorer desktop band, present on every Windows install.
    let mut class = [0u16; 64];
    let n = GetClassNameW(hwnd, &mut class);
    let class = String::from_utf16_lossy(&class[..n as usize]);
    if class == "Progman" || class == "WorkerW" {
        shell.push(hwnd);
    }
    TRUE
}

/// What the card ended up being stacked against, reported by `diag`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorKind {
    /// A `wallpaper*.exe` window (Wallpaper Engine is running).
    WallpaperEngine,
    /// The Explorer desktop band (`Progman` / `WorkerW`).
    DesktopBand,
    /// Nothing usable was found; the card falls back to `HWND_BOTTOM`.
    None,
}

/// The window the card should sit directly above, and what kind of window it is.
///
/// Wallpaper Engine creates its own bottom-most window, so when it is running
/// that is the right anchor. **Without** Wallpaper Engine there is nothing
/// there, and plain `HWND_BOTTOM` can land underneath the desktop icon layer —
/// so the Explorer desktop band is used instead. That is what makes the widget
/// work standalone.
pub fn desktop_anchor_kind() -> (Option<HWND>, AnchorKind) {
    // One tuple so the callback can fill both lists without moving them.
    let mut found: (Vec<HWND>, Vec<HWND>) = (Vec::new(), Vec::new());
    unsafe {
        let _ = EnumWindows(
            Some(enum_desktop_proc),
            LPARAM(&mut found as *mut (Vec<HWND>, Vec<HWND>) as isize),
        );
    }
    let (we, shell) = found;
    if let Some(h) = we.into_iter().next() {
        return (Some(h), AnchorKind::WallpaperEngine);
    }
    if let Some(h) = shell.into_iter().next() {
        return (Some(h), AnchorKind::DesktopBand);
    }
    (None, AnchorKind::None)
}

/// True for the windows Explorer uses to paint the desktop.
pub fn desktop_anchor() -> Option<HWND> {
    desktop_anchor_kind().0
}

/// Window procedure for the card.
///
/// # Safety
///
/// Installed via `RegisterClassW`, so it is only ever called by Windows with a
/// valid `HWND`. The `App` is parked in `GWLP_USERDATA` by `WM_CREATE` and freed
/// by `WM_DESTROY`; every other message is dispatched only while that pointer is
/// still live, so no message can observe a dangling `App`.
pub unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if let Some(taskbar_created) = crate::MSG_TASKBAR_CREATED.get() {
        if msg == *taskbar_created {
            let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
            if !p.is_null() {
                (*p).apply_z_order();
            }
            return LRESULT(0);
        }
    }

    match msg {
        WM_CREATE => {
            let cs = &*(lparam.0 as *const CREATESTRUCTW);
            let app = Box::from_raw(cs.lpCreateParams as *mut App);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(app) as isize);
            LRESULT(0)
        }
        WM_DESTROY => {
            let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
            if !p.is_null() {
                // Tear the settings window down first: it holds a pointer to the
                // App, so letting it outlive the App would leave it dangling.
                crate::settings::close(&mut *p);
                (*p).remove_tray();
                drop(Box::from_raw(p));
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            }
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => {
            let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
            if p.is_null() {
                DefWindowProcW(hwnd, msg, wparam, lparam)
            } else {
                (*p).on_message(msg, wparam, lparam)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaling_round_trips_at_100_percent() {
        assert_eq!(scale_from_physical(340, 96), 340.0);
        assert_eq!(scale_from_physical(510, 144), 340.0);
    }

    #[test]
    fn primary_monitor_has_a_usable_size() {
        let r = primary_monitor_rect();
        assert!(r.right > r.left && r.bottom > r.top);
    }

    #[test]
    fn monitor_list_is_never_empty_and_is_sorted() {
        let all = monitors();
        assert!(!all.is_empty());
        let mut sorted = all.clone();
        sorted.sort_by_key(|r| (r.left, r.top));
        assert_eq!(
            all.iter().map(|r| (r.left, r.top)).collect::<Vec<_>>(),
            sorted.iter().map(|r| (r.left, r.top)).collect::<Vec<_>>(),
            "monitors must come back in a stable order"
        );
    }

    #[test]
    fn monitor_lookup_falls_back_instead_of_panicking() {
        // A config from another machine may name a monitor that is not attached.
        let weird = monitor_rect(99);
        assert!(weird.right > weird.left);
        let primary = monitor_rect(-1);
        assert!(primary.right > primary.left);
    }

    #[test]
    fn wide_buffers_are_nul_terminated_and_truncated() {
        let mut buf = [1u16; 8];
        fill_wide(&mut buf, "abc");
        assert_eq!(&buf[..4], &[b'a' as u16, b'b' as u16, b'c' as u16, 0]);
        assert!(buf[4..].iter().all(|c| *c == 0), "tail must be zeroed");

        let mut small = [1u16; 4];
        fill_wide(&mut small, "abcdef");
        assert_eq!(small[3], 0, "must stay terminated when truncated");
    }

    #[test]
    fn tray_callback_is_a_wm_app_range_message() {
        // WM_TRAY must be a WM_APP-range message so it cannot be confused with a
        // system message delivered to the same window procedure. Routing it
        // through a binding keeps the assertion a runtime check rather than a
        // compile-time constant clippy would fold away.
        let callback: u32 = WM_TRAY;
        let wm_app_base: u32 = 0x8000;
        assert!(
            callback >= wm_app_base,
            "tray callback must be WM_APP based"
        );
        assert_eq!(callback, WM_APP + 1);
    }
}
