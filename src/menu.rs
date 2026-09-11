//! The context menu, shared by the tray icon and the card.
//!
//! Two deliberate choices:
//!
//! * `TPM_RETURNCMD` instead of letting the menu post `WM_COMMAND` — the chosen
//!   id comes straight back from `TrackPopupMenu`, so no live id→action table
//!   has to survive while a modal menu is up.
//! * Every id is an explicit constant, so radio groups are contiguous ranges
//!   (`CheckMenuRadioItem` needs that) and a unit test can prove no two entries
//!   collide.

use crate::app::{monitors, App};
use crate::audio::PlayMode;
use crate::i18n::{tr, tr_num, Key};
use windows::{
    core::HSTRING,
    Win32::{
        Foundation::{LPARAM, POINT, WPARAM},
        UI::WindowsAndMessaging::{
            AppendMenuW, CheckMenuRadioItem, CreatePopupMenu, DestroyMenu, GetCursorPos,
            PostMessageW, SetForegroundWindow, TrackPopupMenu, HMENU, MENU_ITEM_FLAGS,
            MF_BYCOMMAND, MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, MF_UNCHECKED,
            TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_NULL,
        },
    },
};

const ID_PLAYPAUSE: u32 = 1001;
const ID_PREV: u32 = 1002;
const ID_NEXT: u32 = 1003;

/// Play-mode group: three contiguous ids.
pub const PLAY_MODE_FIRST: u32 = 1010;
pub const PLAY_MODES: [PlayMode; 3] =
    [PlayMode::Sequential, PlayMode::Shuffle, PlayMode::RepeatOne];
const PLAY_MODE_IDS: [u32; 3] = [PLAY_MODE_FIRST, PLAY_MODE_FIRST + 1, PLAY_MODE_FIRST + 2];

const ID_ADD_FOLDER: u32 = 1020;
const ID_REPLACE_FOLDER: u32 = 1021;
const ID_OPEN_FOLDER: u32 = 1022;
const ID_CLEAR_FOLDERS: u32 = 1023;

const ID_TOGGLE_RECURSIVE: u32 = 1030;
const ID_EXCLUDE_FOLDER: u32 = 1031;
const ID_RESCAN: u32 = 1032;

const ID_AUTOSTART: u32 = 1040;
const ID_AUTOPLAY: u32 = 1041;

/// Controls group: two contiguous ids.
pub const CONTROLS_FIRST: u32 = 1050;
pub const CONTROLS: [&str; 2] = ["hover", "always"];
const CONTROLS_IDS: [u32; 2] = [CONTROLS_FIRST, CONTROLS_FIRST + 1];

/// Window-layer group: two contiguous ids.
pub const MODE_FIRST: u32 = 1060;
pub const MODES: [&str; 2] = ["bottom", "topmost"];
const MODE_IDS: [u32; 2] = [MODE_FIRST, MODE_FIRST + 1];

/// Monitor group: `MONITOR_FIRST + index`.
pub const MONITOR_FIRST: u32 = 1070;
/// Enough ids for any realistic wall; anything beyond falls back to primary.
pub const MONITOR_LIMIT: u32 = 16;

const ID_TOGGLE_MOVE: u32 = 1100;

const ID_SETTINGS: u32 = 1090;
const ID_OPEN_CONFIG: u32 = 1091;
const ID_RELOAD_CONFIG: u32 = 1092;
const ID_ABOUT: u32 = 1093;
const ID_EXIT: u32 = 1099;

/// Every command the menu can produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    TogglePlay,
    Prev,
    Next,
    PlayMode(PlayMode),
    AddFolder,
    ReplaceFolder,
    OpenFirstFolder,
    ClearFolders,
    ToggleRecursive,
    ExcludeFolder,
    Rescan,
    ToggleAutostart,
    ToggleAutoplay,
    ShowControls(&'static str),
    WindowMode(&'static str),
    Monitor(i32),
    ToggleMove,
    Settings,
    OpenConfig,
    ReloadConfig,
    About,
    Exit,
}

/// Collects the id→action mapping while the menu is assembled.
struct Builder {
    menu: HMENU,
    map: Vec<(u32, Action)>,
}

impl Builder {
    fn new(menu: HMENU) -> Self {
        Self {
            menu,
            map: Vec::new(),
        }
    }

    fn raw(&mut self, flags: MENU_ITEM_FLAGS, label: &str, id: u32, action: Option<Action>) {
        let text = HSTRING::from(label);
        unsafe {
            let _ = AppendMenuW(self.menu, flags, id as usize, &text);
        }
        if let Some(action) = action {
            self.map.push((id, action));
        }
    }

    fn item(&mut self, label: &str, id: u32, action: Action) {
        self.raw(MF_STRING, label, id, Some(action));
    }

    fn checked(&mut self, label: &str, on: bool, id: u32, action: Action) {
        let flags = MF_STRING | if on { MF_CHECKED } else { MF_UNCHECKED };
        self.raw(flags, label, id, Some(action));
    }

    fn disabled(&mut self, label: &str, id: u32) {
        self.raw(MF_STRING | MF_GRAYED, label, id, None);
    }

    fn separator(&mut self) {
        unsafe {
            let _ = AppendMenuW(self.menu, MF_SEPARATOR, 0, None);
        }
    }

    fn empty_submenu(&self) -> Builder {
        let child = unsafe { CreatePopupMenu().unwrap_or_default() };
        Builder::new(child)
    }

    fn attach(&mut self, child: Builder, label: &str) {
        let text = HSTRING::from(label);
        unsafe {
            // The parent menu takes ownership of the submenu handle, so the child
            // must never be destroyed on its own.
            let _ = AppendMenuW(self.menu, MF_POPUP, child.menu.0 as usize, &text);
        }
        self.map.extend(child.map);
    }
}

/// Marks one entry of a contiguous group as the selected radio button.
fn check_radio(menu: HMENU, group: &[u32], selected: usize) {
    if group.is_empty() {
        return;
    }
    let index = selected.min(group.len() - 1);
    unsafe {
        let _ = CheckMenuRadioItem(
            menu,
            group[0],
            *group.last().unwrap(),
            group[index],
            MF_BYCOMMAND.0,
        );
    }
}

/// A built menu plus its id→action table. Dropping it releases the handle.
pub struct BuiltMenu {
    menu: HMENU,
    map: Vec<(u32, Action)>,
}

impl BuiltMenu {
    pub fn action(&self, id: u32) -> Option<Action> {
        self.map
            .iter()
            .find(|(candidate, _)| *candidate == id)
            .map(|(_, action)| *action)
    }
}

impl Drop for BuiltMenu {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyMenu(self.menu);
        }
    }
}

fn play_mode_label(mode: PlayMode) -> &'static str {
    match mode {
        PlayMode::Sequential => tr(Key::PlayModeSequential),
        PlayMode::Shuffle => tr(Key::PlayModeShuffle),
        PlayMode::RepeatOne => tr(Key::PlayModeRepeatOne),
    }
}

/// Index of `needle` in `haystack`, or 0 — every group in this menu is
/// guaranteed to contain the configured value because `config::validate` runs
/// first, but a defensive fallback keeps the radio group well-formed anyway.
fn index_of<T: PartialEq + Copy>(haystack: &[T], needle: T) -> usize {
    haystack.iter().position(|x| *x == needle).unwrap_or(0)
}

/// Builds the menu for the app's current state.
pub fn build(app: &App) -> Option<BuiltMenu> {
    let menu = unsafe { CreatePopupMenu().ok()? };
    let mut b = Builder::new(menu);

    let playing = app.state.playing.load(std::sync::atomic::Ordering::Relaxed);
    b.item(
        if playing {
            tr(Key::MenuPause)
        } else {
            tr(Key::MenuPlay)
        },
        ID_PLAYPAUSE,
        Action::TogglePlay,
    );
    b.item(tr(Key::MenuNext), ID_NEXT, Action::Next);
    b.item(tr(Key::MenuPrev), ID_PREV, Action::Prev);
    b.separator();

    // --- play mode ---------------------------------------------------------
    let current_mode = PlayMode::from_config(&app.cfg.play_mode);
    let mut mode_menu = b.empty_submenu();
    let mode_selected = index_of(&PLAY_MODES, current_mode);
    for (i, mode) in PLAY_MODES.iter().enumerate() {
        mode_menu.item(
            play_mode_label(*mode),
            PLAY_MODE_IDS[i],
            Action::PlayMode(*mode),
        );
    }
    check_radio(mode_menu.menu, &PLAY_MODE_IDS, mode_selected);
    b.attach(mode_menu, tr(Key::MenuPlayMode));
    b.separator();

    // --- music folders -----------------------------------------------------
    let mut lib = b.empty_submenu();
    lib.item(tr(Key::MenuAddFolder), ID_ADD_FOLDER, Action::AddFolder);
    lib.item(
        tr(Key::MenuReplaceFolder),
        ID_REPLACE_FOLDER,
        Action::ReplaceFolder,
    );
    if app.cfg.library.is_empty() {
        lib.disabled(tr(Key::MenuOpenFolder), ID_OPEN_FOLDER);
        lib.disabled(tr(Key::MenuClearFolders), ID_CLEAR_FOLDERS);
    } else {
        lib.item(
            tr(Key::MenuOpenFolder),
            ID_OPEN_FOLDER,
            Action::OpenFirstFolder,
        );
        lib.item(
            tr(Key::MenuClearFolders),
            ID_CLEAR_FOLDERS,
            Action::ClearFolders,
        );
    }
    b.attach(lib, tr(Key::MenuLibrary));

    // --- scan scope --------------------------------------------------------
    let mut scan = b.empty_submenu();
    scan.checked(
        tr(Key::MenuScanRecursive),
        app.cfg.scan_recursive,
        ID_TOGGLE_RECURSIVE,
        Action::ToggleRecursive,
    );
    scan.item(
        tr(Key::MenuExcludeFolder),
        ID_EXCLUDE_FOLDER,
        Action::ExcludeFolder,
    );
    scan.separator();
    scan.item(tr(Key::MenuRescan), ID_RESCAN, Action::Rescan);
    b.attach(scan, tr(Key::MenuScan));
    b.separator();

    // --- startup -----------------------------------------------------------
    b.checked(
        tr(Key::MenuAutostart),
        app.cfg.autostart,
        ID_AUTOSTART,
        Action::ToggleAutostart,
    );
    b.checked(
        tr(Key::MenuAutoplay),
        app.cfg.autoplay,
        ID_AUTOPLAY,
        Action::ToggleAutoplay,
    );

    // --- controls ----------------------------------------------------------
    let mut controls = b.empty_submenu();
    for (i, option) in CONTROLS.iter().enumerate() {
        controls.item(
            if *option == "always" {
                tr(Key::ShowControlsAlways)
            } else {
                tr(Key::ShowControlsHover)
            },
            CONTROLS_IDS[i],
            Action::ShowControls(option),
        );
    }
    check_radio(
        controls.menu,
        &CONTROLS_IDS,
        index_of(&CONTROLS, app.cfg.show_controls.as_str()),
    );
    b.attach(controls, tr(Key::MenuShowControls));

    // --- window layer ------------------------------------------------------
    let mut layer = b.empty_submenu();
    for (i, option) in MODES.iter().enumerate() {
        layer.item(
            if *option == "topmost" {
                tr(Key::ModeTopmost)
            } else {
                tr(Key::ModeBottom)
            },
            MODE_IDS[i],
            Action::WindowMode(option),
        );
    }
    check_radio(
        layer.menu,
        &MODE_IDS,
        index_of(&MODES, app.cfg.mode.as_str()),
    );
    b.attach(layer, tr(Key::MenuZOrder));

    // --- monitor -----------------------------------------------------------
    let all = monitors();
    let shown = all.len().min(MONITOR_LIMIT as usize);
    let mut mon = b.empty_submenu();
    for i in 0..shown {
        mon.item(
            &tr_num(Key::MonitorItem, i + 1),
            MONITOR_FIRST + i as u32,
            Action::Monitor(i as i32),
        );
    }
    if shown > 1 {
        // -1 means "primary", which is whichever monitor sits at the origin.
        let selected = if app.cfg.monitor < 0 {
            all.iter()
                .position(|r| r.left == 0 && r.top == 0)
                .unwrap_or(0)
        } else {
            (app.cfg.monitor as usize).min(shown - 1)
        };
        let ids: Vec<u32> = (0..shown).map(|i| MONITOR_FIRST + i as u32).collect();
        check_radio(mon.menu, &ids, selected);
    } else {
        mon.disabled(&tr_num(Key::MonitorItem, 1), MONITOR_FIRST);
    }
    b.attach(mon, tr(Key::MenuMonitor));

    // --- move mode ---------------------------------------------------------
    b.checked(
        tr(Key::MenuMoveWindow),
        app.moving,
        ID_TOGGLE_MOVE,
        Action::ToggleMove,
    );
    b.separator();

    b.item(tr(Key::MenuSettings), ID_SETTINGS, Action::Settings);
    b.item(tr(Key::MenuOpenConfig), ID_OPEN_CONFIG, Action::OpenConfig);
    b.item(
        tr(Key::MenuReloadConfig),
        ID_RELOAD_CONFIG,
        Action::ReloadConfig,
    );
    b.separator();
    b.item(tr(Key::MenuAbout), ID_ABOUT, Action::About);
    b.separator();
    b.item(tr(Key::MenuExit), ID_EXIT, Action::Exit);

    Some(BuiltMenu {
        menu: b.menu,
        map: b.map,
    })
}

/// Shows the menu and returns the chosen action, if any.
pub fn popup(app: &App, built: &BuiltMenu, x: i32, y: i32) -> Option<Action> {
    unsafe {
        // TrackPopupMenu requires the owner to be foreground, otherwise the menu
        // will not dismiss when clicking elsewhere.
        let _ = SetForegroundWindow(app.hwnd);
        let chosen = TrackPopupMenu(
            built.menu,
            TPM_LEFTALIGN | TPM_RIGHTBUTTON | TPM_RETURNCMD,
            x,
            y,
            Some(0),
            app.hwnd,
            None,
        );
        // The card is WS_EX_NOACTIVATE, so the foreground handshake above often
        // does not complete. Posting a null message after the menu closes is the
        // standard workaround that makes the dismissal reliable.
        let _ = PostMessageW(Some(app.hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        built.action(chosen.0 as u32)
    }
}

/// Builds the menu, shows it at the cursor, and returns the chosen action.
pub fn show_at_cursor(app: &App) -> Option<Action> {
    let built = build(app)?;
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    popup(app, &built, pt.x, pt.y)
}

/// The complete id list, used by the tests below.
#[cfg(test)]
fn all_ids() -> Vec<u32> {
    let mut ids = vec![
        ID_PLAYPAUSE,
        ID_PREV,
        ID_NEXT,
        ID_ADD_FOLDER,
        ID_REPLACE_FOLDER,
        ID_OPEN_FOLDER,
        ID_CLEAR_FOLDERS,
        ID_TOGGLE_RECURSIVE,
        ID_EXCLUDE_FOLDER,
        ID_RESCAN,
        ID_AUTOSTART,
        ID_AUTOPLAY,
        ID_TOGGLE_MOVE,
        ID_SETTINGS,
        ID_OPEN_CONFIG,
        ID_RELOAD_CONFIG,
        ID_ABOUT,
        ID_EXIT,
    ];
    ids.extend(PLAY_MODE_IDS);
    ids.extend(CONTROLS_IDS);
    ids.extend(MODE_IDS);
    ids.extend(MONITOR_FIRST..MONITOR_FIRST + MONITOR_LIMIT);
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_two_menu_entries_share_an_id() {
        let mut ids = all_ids();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total, "duplicate menu id");
    }

    #[test]
    fn radio_groups_are_contiguous() {
        // CheckMenuRadioItem takes a first/last id range, so each group must be
        // consecutive with no gaps and no foreign ids inside.
        for (name, group) in [
            ("play mode", PLAY_MODE_IDS.as_slice()),
            ("controls", CONTROLS_IDS.as_slice()),
            ("window layer", MODE_IDS.as_slice()),
        ] {
            for pair in group.windows(2) {
                assert_eq!(pair[1], pair[0] + 1, "{name} group is not contiguous");
            }
        }
    }

    #[test]
    fn groups_do_not_overlap_each_other() {
        let ranges = [
            (PLAY_MODE_IDS[0], *PLAY_MODE_IDS.last().unwrap()),
            (CONTROLS_IDS[0], *CONTROLS_IDS.last().unwrap()),
            (MODE_IDS[0], *MODE_IDS.last().unwrap()),
            (MONITOR_FIRST, MONITOR_FIRST + MONITOR_LIMIT - 1),
        ];
        for (i, (a_start, a_end)) in ranges.iter().enumerate() {
            for (b_start, b_end) in ranges.iter().skip(i + 1) {
                assert!(
                    a_end < b_start || b_end < a_start,
                    "id ranges overlap: {a_start}..{a_end} and {b_start}..{b_end}"
                );
            }
        }
    }

    #[test]
    fn groups_cover_every_configured_value() {
        assert_eq!(PLAY_MODES.len(), crate::config::PLAY_MODES.len());
        assert_eq!(PLAY_MODE_IDS.len(), PLAY_MODES.len());
        assert_eq!(CONTROLS.len(), crate::config::SHOW_CONTROLS.len());
        assert_eq!(MODE_IDS.len(), MODES.len());
        for c in CONTROLS {
            assert!(
                crate::config::SHOW_CONTROLS.contains(&c),
                "{c} is not valid"
            );
        }
        for m in MODES {
            assert!(crate::config::MODES.contains(&m), "{m} is not valid");
        }
    }

    #[test]
    fn every_play_mode_has_a_label() {
        for mode in PLAY_MODES {
            assert!(!play_mode_label(mode).is_empty());
        }
    }

    #[test]
    fn selection_falls_back_to_the_first_entry_for_unknown_values() {
        assert_eq!(index_of(&PLAY_MODES, PlayMode::RepeatOne), 2);
        assert_eq!(index_of(&PLAY_MODES, PlayMode::Sequential), 0);
        assert_eq!(index_of(&CONTROLS, "nonsense"), 0);
        assert_eq!(index_of(&MODES, "topmost"), 1);
    }

    #[test]
    fn check_radio_tolerates_an_out_of_range_selection() {
        // Must not panic or index past the group.
        check_radio(HMENU::default(), &MODE_IDS, 99);
        check_radio(HMENU::default(), &[], 0);
    }
}
