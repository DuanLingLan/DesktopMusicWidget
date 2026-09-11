//! Configuration: schema v2, validation, v1 migration and atomic saves.
//!
//! Loading must never fail. A missing file is created, a corrupt file is backed
//! up and replaced by defaults, and out-of-range values are clamped with a
//! warning — editing a text file should never be able to make the widget
//! unstartable.

use crate::cli::Args;
use crate::theme::Theme;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Bumped whenever the on-disk shape changes. See `migrate`.
pub const CONFIG_VERSION: u32 = 2;

/// Decoders actually compiled into rodio (`playback, flac, mp3, mp4, vorbis,
/// wav, symphonia-aiff`). Opus and WMA are deliberately absent: rodio 0.22 has
/// no support for them, so advertising those extensions would only produce
/// silent decode failures.
pub const DEFAULT_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "wav", "m4a", "m4b", "aac", "ogg", "oga", "aiff",
];

pub const PLAY_MODES: &[&str] = &["sequential", "shuffle", "repeat-one"];
pub const ANCHORS: &[&str] = &["top-left", "top-center", "top-right", "free"];
pub const MODES: &[&str] = &["bottom", "topmost"];
pub const SHOW_CONTROLS: &[&str] = &["hover", "always"];
pub const LANGUAGES: &[&str] = &["auto", "zh-CN", "en"];
pub const HOTKEY_PRESETS: &[&str] = &["ctrl-alt", "ctrl-shift-alt"];

pub const WIDTH_RANGE: (i32, i32) = (200, 900);
pub const HEIGHT_RANGE: (i32, i32) = (64, 400);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    /// Schema version of the file on disk.
    ///
    /// The field-level default (0) deliberately overrides the struct-level one:
    /// a file without this key is a v1 file, and treating it as current would
    /// silently skip the `shuffle` → `play_mode` migration.
    #[serde(default = "legacy_version")]
    pub config_version: u32,
    /// `auto` | `zh-CN` | `en`
    pub language: String,

    /// Directories to scan for music. Empty means "not configured yet", which
    /// triggers the first-run folder picker.
    pub library: Vec<String>,
    pub scan_recursive: bool,
    /// Folder names (matched case-insensitively) or absolute paths to skip.
    pub exclude_dirs: Vec<String>,
    /// Lower-case extensions without a leading dot.
    pub extensions: Vec<String>,

    /// `sequential` | `shuffle` | `repeat-one`
    pub play_mode: String,
    /// 0-100 UI volume.
    pub volume: u32,
    /// Start playing as soon as a queue is available.
    pub autoplay: bool,

    /// Mirror of the HKCU Run entry; the app reconciles the registry to match.
    pub autostart: bool,
    pub hotkeys_enabled: bool,
    /// `ctrl-alt` | `ctrl-shift-alt`
    pub hotkey_preset: String,

    /// `bottom` (desktop layer, default) | `topmost`
    pub mode: String,
    /// `-1` = primary monitor, otherwise a zero-based monitor index.
    pub monitor: i32,
    /// `top-left` | `top-center` | `top-right` | `free`
    pub anchor: String,
    /// With `anchor = "free"` these are absolute logical pixels on `monitor`,
    /// which is what dragging the card writes back.
    pub offset_x: i32,
    pub offset_y: i32,
    /// Card size in logical (96-dpi) pixels.
    pub width: i32,
    pub height: i32,
    pub corner_radius: f32,
    /// Card background opacity, 0.0-1.0.
    pub card_opacity: f32,
    /// `hover` (default) | `always`
    pub show_controls: String,
    pub font_family: String,

    pub theme: Theme,
}

/// A config file with no `config_version` key is from the v1 layout.
fn legacy_version() -> u32 {
    0
}

impl Default for Config {
    fn default() -> Self {
        Self {
            config_version: CONFIG_VERSION,
            language: "auto".into(),

            // Deliberately empty: the author's personal path must not ship as a
            // default. An empty library triggers the first-run folder picker.
            library: Vec::new(),
            scan_recursive: true,
            exclude_dirs: Vec::new(),
            extensions: DEFAULT_EXTENSIONS.iter().map(|s| s.to_string()).collect(),

            play_mode: "shuffle".into(),
            volume: 70,
            autoplay: true,

            autostart: false,
            hotkeys_enabled: true,
            hotkey_preset: "ctrl-alt".into(),

            mode: "bottom".into(),
            monitor: -1,
            anchor: "top-center".into(),
            offset_x: 0,
            offset_y: 24,
            width: 340,
            height: 96,
            corner_radius: 14.0,
            card_opacity: 0.78,
            show_controls: "hover".into(),
            font_family: "Segoe UI".into(),

            theme: Theme::default(),
        }
    }
}

impl Config {
    pub fn library_paths(&self) -> Vec<PathBuf> {
        self.library.iter().map(PathBuf::from).collect()
    }

    /// Configured folders that are not currently reachable. They are kept in the
    /// config on purpose — a removable drive may simply be unplugged.
    pub fn missing_dirs(&self) -> Vec<PathBuf> {
        self.library_paths()
            .into_iter()
            .filter(|p| !p.is_dir())
            .collect()
    }

    /// True when nothing should be scanned, which is the first-run state.
    pub fn needs_library_setup(&self) -> bool {
        self.library.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Path resolution
// ---------------------------------------------------------------------------

pub fn appdata() -> PathBuf {
    std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Pure so the precedence rules can be unit-tested. Order: `--config-dir` >
/// `--portable` > env var > a `config.toml` sitting next to the exe (an
/// unpacked portable zip) > `%APPDATA%\DesktopMusicWidget`.
fn choose_dir(
    flag: Option<&Path>,
    portable: bool,
    env: Option<&str>,
    exe_dir: &Path,
    appdata: &Path,
) -> PathBuf {
    if let Some(d) = flag {
        return d.to_path_buf();
    }
    if portable {
        return exe_dir.to_path_buf();
    }
    if let Some(e) = env {
        let e = e.trim();
        if !e.is_empty() {
            return PathBuf::from(e);
        }
    }
    if exe_dir.join("config.toml").exists() {
        return exe_dir.to_path_buf();
    }
    appdata.join("DesktopMusicWidget")
}

static RESOLVED_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Resolves and remembers the config directory. Call once, right after parsing
/// the command line.
pub fn init_paths(args: &Args) -> PathBuf {
    let dir = choose_dir(
        args.config_dir.as_deref(),
        args.portable,
        std::env::var("DESKTOP_MUSIC_WIDGET_CONFIG_DIR")
            .ok()
            .as_deref(),
        &exe_dir(),
        &appdata(),
    );
    let _ = RESOLVED_DIR.set(dir.clone());
    dir
}

pub fn config_dir() -> PathBuf {
    RESOLVED_DIR.get().cloned().unwrap_or_else(|| {
        choose_dir(
            None,
            false,
            std::env::var("DESKTOP_MUSIC_WIDGET_CONFIG_DIR")
                .ok()
                .as_deref(),
            &exe_dir(),
            &appdata(),
        )
    })
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

/// The old personal build stored its config here. Read-only: it is never
/// modified, so an existing install of the previous version keeps working.
pub fn legacy_config_path() -> PathBuf {
    appdata().join("HorizMusicWidget").join("config.toml")
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadNote {
    /// Read an existing config unchanged.
    Loaded,
    /// No config anywhere: defaults were written (first run).
    Fresh,
    /// Adopted the config from the pre-rename build.
    MigratedLegacy,
    /// The file could not be parsed; it was backed up and defaults are in use.
    RecoveredFromBadFile,
}

pub struct Loaded {
    pub cfg: Config,
    pub note: LoadNote,
    /// Human-readable corrections, already logged by `load`.
    pub warnings: Vec<String>,
}

pub fn load() -> Loaded {
    let path = config_path();
    let mut note = LoadNote::Loaded;

    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => {
            if let Ok(legacy) = std::fs::read_to_string(legacy_config_path()) {
                note = LoadNote::MigratedLegacy;
                legacy
            } else {
                note = LoadNote::Fresh;
                String::new()
            }
        }
    };

    let mut warnings = Vec::new();
    let mut cfg = if text.trim().is_empty() {
        Config::default()
    } else {
        match toml::from_str::<Config>(&text) {
            Ok(c) => migrate(c, &text),
            Err(e) => {
                let backup = path.with_extension("toml.bak");
                let _ = std::fs::write(&backup, &text);
                note = LoadNote::RecoveredFromBadFile;
                warnings.push(format!(
                    "config.toml 解析失败 ({e})，已备份到 {}",
                    backup.display()
                ));
                Config::default()
            }
        }
    };

    let corrections = validate(&mut cfg);
    let had_corrections = !corrections.is_empty();
    warnings.extend(corrections);

    for w in &warnings {
        crate::log_warn!("{w}");
    }

    // Rewrite when we created, migrated, recovered or corrected anything, so the
    // file on disk always describes what is actually running.
    if note != LoadNote::Loaded || had_corrections {
        if let Err(e) = save(&cfg) {
            crate::log_error!("cannot write {}: {e}", path.display());
        }
    }

    crate::log_info!(
        "config loaded from {} (note={note:?}, library={:?}, play_mode={})",
        path.display(),
        cfg.library,
        cfg.play_mode
    );

    Loaded {
        cfg,
        note,
        warnings,
    }
}

/// v1 → v2. The only shape change is `shuffle: bool` becoming `play_mode`.
fn migrate(mut cfg: Config, text: &str) -> Config {
    if cfg.config_version >= CONFIG_VERSION {
        return cfg;
    }
    if let Ok(value) = text.parse::<toml::Value>() {
        if let Some(shuffle) = value.get("shuffle").and_then(|v| v.as_bool()) {
            cfg.play_mode = if shuffle { "shuffle" } else { "sequential" }.into();
        }
    }
    cfg.config_version = CONFIG_VERSION;
    cfg
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

fn fix_enum(field: &mut String, allowed: &[&str], default: &str, name: &str, w: &mut Vec<String>) {
    let lowered = field.trim().to_ascii_lowercase();
    if allowed.contains(&lowered.as_str()) {
        *field = lowered;
        return;
    }
    w.push(format!("{name} = {field:?} 无效，已改为 {default:?}"));
    *field = default.to_string();
}

fn clamp_int(v: &mut i32, range: (i32, i32), name: &str, w: &mut Vec<String>) {
    let clamped = (*v).clamp(range.0, range.1);
    if clamped != *v {
        w.push(format!(
            "{name} = {v} 超出范围 {}..={}，已改为 {clamped}",
            range.0, range.1
        ));
        *v = clamped;
    }
}

fn clamp_unit(v: &mut f32, default: f32, name: &str, w: &mut Vec<String>) {
    if !v.is_finite() {
        w.push(format!("{name} 不是有效数字，已改为 {default}"));
        *v = default;
        return;
    }
    let clamped = v.clamp(0.0, 1.0);
    if (clamped - *v).abs() > f32::EPSILON {
        w.push(format!("{name} = {v} 超出 0..1，已改为 {clamped}"));
        *v = clamped;
    }
}

/// Deduplicates while preserving order (a sort would reorder user intent).
fn dedup_keep_order(items: &mut Vec<String>) {
    let mut seen: Vec<String> = Vec::with_capacity(items.len());
    items.retain(|i| {
        if seen.contains(i) {
            false
        } else {
            seen.push(i.clone());
            true
        }
    });
}

/// Clamps and normalises in place, returning a description of every change.
pub fn validate(cfg: &mut Config) -> Vec<String> {
    let mut w = Vec::new();
    cfg.config_version = CONFIG_VERSION;

    fix_enum(&mut cfg.language, LANGUAGES, "auto", "language", &mut w);
    fix_enum(
        &mut cfg.play_mode,
        PLAY_MODES,
        "shuffle",
        "play_mode",
        &mut w,
    );
    fix_enum(&mut cfg.mode, MODES, "bottom", "mode", &mut w);
    fix_enum(&mut cfg.anchor, ANCHORS, "top-center", "anchor", &mut w);
    fix_enum(
        &mut cfg.show_controls,
        SHOW_CONTROLS,
        "hover",
        "show_controls",
        &mut w,
    );
    fix_enum(
        &mut cfg.hotkey_preset,
        HOTKEY_PRESETS,
        "ctrl-alt",
        "hotkey_preset",
        &mut w,
    );

    clamp_int(&mut cfg.width, WIDTH_RANGE, "width", &mut w);
    clamp_int(&mut cfg.height, HEIGHT_RANGE, "height", &mut w);
    clamp_int(&mut cfg.monitor, (-1, 64), "monitor", &mut w);

    let max_radius = cfg.height as f32 / 2.0;
    if !cfg.corner_radius.is_finite() {
        w.push("corner_radius 不是有效数字，已改为 14".into());
        cfg.corner_radius = 14.0;
    } else {
        let clamped = cfg.corner_radius.clamp(0.0, max_radius);
        if (clamped - cfg.corner_radius).abs() > f32::EPSILON {
            w.push(format!(
                "corner_radius = {} 超出 0..{max_radius}，已改为 {clamped}",
                cfg.corner_radius
            ));
            cfg.corner_radius = clamped;
        }
    }

    clamp_unit(&mut cfg.card_opacity, 0.78, "card_opacity", &mut w);

    if cfg.volume > 100 {
        w.push(format!("volume = {} 超出 0..100，已改为 100", cfg.volume));
        cfg.volume = 100;
    }

    if cfg.font_family.trim().is_empty() {
        w.push("font_family 为空，已改为 Segoe UI".into());
        cfg.font_family = "Segoe UI".into();
    }

    // Library paths: drop blanks, keep duplicates out, keep missing paths.
    let before = cfg.library.len();
    cfg.library = cfg
        .library
        .iter()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    dedup_keep_order(&mut cfg.library);
    if cfg.library.len() != before {
        w.push("library 中的空白或重复项已清理".into());
    }

    cfg.exclude_dirs = cfg
        .exclude_dirs
        .iter()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    dedup_keep_order(&mut cfg.exclude_dirs);

    let mut exts: Vec<String> = cfg
        .extensions
        .iter()
        .map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase())
        .filter(|e| !e.is_empty())
        .collect();
    dedup_keep_order(&mut exts);
    if exts.is_empty() {
        w.push("extensions 为空，已恢复为默认列表".into());
        exts = DEFAULT_EXTENSIONS.iter().map(|s| s.to_string()).collect();
    }
    cfg.extensions = exts;

    w
}

// ---------------------------------------------------------------------------
// Saving
// ---------------------------------------------------------------------------

pub fn to_toml(cfg: &Config) -> String {
    // The config is plain data, but the Result is still handled rather than
    // unwrapped so a surprise cannot abort the process.
    toml::to_string_pretty(cfg).unwrap_or_else(|e| {
        crate::log_error!("cannot serialise config: {e}");
        String::new()
    })
}

/// Writes through a temporary file so a crash midway cannot leave a truncated
/// config behind. `std::fs::rename` replaces the destination on Windows.
pub fn save(cfg: &Config) -> std::io::Result<()> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;
    let text = to_toml(cfg);
    let tmp = dir.join("config.toml.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, config_path())
}

/// Backs up the current file and writes defaults. Returns the backup path when
/// one was made.
pub fn reset() -> std::io::Result<Option<PathBuf>> {
    let path = config_path();
    let backup = if path.exists() {
        let bak = path.with_extension("toml.bak");
        std::fs::copy(&path, &bak)?;
        Some(bak)
    } else {
        None
    };
    save(&Config::default())?;
    Ok(backup)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(cfg: &Config) -> Config {
        toml::from_str::<Config>(&to_toml(cfg)).expect("serialised config must parse")
    }

    #[test]
    fn defaults_round_trip_unchanged() {
        let cfg = Config::default();
        assert_eq!(round_trip(&cfg), cfg);
        assert_eq!(validate(&mut cfg.clone()), Vec::<String>::new());
    }

    #[test]
    fn default_library_is_empty_not_a_personal_path() {
        let cfg = Config::default();
        assert!(cfg.library.is_empty(), "must not ship a hardcoded folder");
        assert!(cfg.needs_library_setup());
    }

    #[test]
    fn default_extensions_match_the_compiled_decoders() {
        let cfg = Config::default();
        assert!(cfg.extensions.contains(&"m4a".to_string()));
        assert!(cfg.extensions.contains(&"aiff".to_string()));
        // rodio 0.22 cannot decode these, so they must not be advertised.
        assert!(!cfg.extensions.contains(&"opus".to_string()));
        assert!(!cfg.extensions.contains(&"wma".to_string()));
    }

    #[test]
    fn v1_shuffle_true_becomes_shuffle_mode() {
        let v1 = r#"
            library = ['F:\Music']
            mode = "bottom"
            anchor = "top-center"
            offset_x = 0
            offset_y = 24
            width = 340
            height = 96
            corner_radius = 14.0
            card_opacity = 0.78
            shuffle = true
            volume = 35
            show_controls = "hover"
        "#;
        let parsed: Config = toml::from_str(v1).unwrap();
        let cfg = migrate(parsed, v1);
        assert_eq!(cfg.config_version, CONFIG_VERSION);
        assert_eq!(cfg.play_mode, "shuffle");
        assert_eq!(cfg.library, vec!["F:\\Music".to_string()]);
        assert_eq!(cfg.volume, 35, "other v1 fields must survive");
        assert!(cfg.autoplay, "autoplay is new and defaults to on");
    }

    #[test]
    fn v1_shuffle_false_becomes_sequential() {
        let v1 = "shuffle = false\n";
        let parsed: Config = toml::from_str(v1).unwrap();
        assert_eq!(
            parsed.config_version, 0,
            "a file without config_version must be recognised as v1"
        );
        assert_eq!(migrate(parsed, v1).play_mode, "sequential");
    }

    #[test]
    fn v1_without_shuffle_keeps_the_old_default_of_on() {
        let v1 = "library = ['C:\\M']\n";
        let parsed: Config = toml::from_str(v1).unwrap();
        assert_eq!(migrate(parsed, v1).play_mode, "shuffle");
    }

    #[test]
    fn already_current_config_is_not_migrated_again() {
        let text = to_toml(&Config {
            play_mode: "repeat-one".into(),
            ..Config::default()
        });
        // Even with a stray legacy key present, v2 must be left alone.
        let text = format!("{text}\nshuffle = true\n");
        let parsed: Config = toml::from_str(&text).unwrap();
        assert_eq!(migrate(parsed, &text).play_mode, "repeat-one");
    }

    #[test]
    fn invalid_enum_falls_back_and_is_reported() {
        let mut cfg = Config {
            play_mode: "party".into(),
            mode: "floating".into(),
            anchor: "middle".into(),
            show_controls: "sometimes".into(),
            language: "fr".into(),
            hotkey_preset: "vim".into(),
            ..Config::default()
        };
        let w = validate(&mut cfg);
        assert_eq!(w.len(), 6, "each bad enum reports once: {w:?}");
        assert_eq!(cfg.play_mode, "shuffle");
        assert_eq!(cfg.mode, "bottom");
        assert_eq!(cfg.anchor, "top-center");
        assert_eq!(cfg.show_controls, "hover");
        assert_eq!(cfg.language, "auto");
        assert_eq!(cfg.hotkey_preset, "ctrl-alt");
    }

    #[test]
    fn numbers_are_clamped_into_range() {
        let mut cfg = Config {
            width: 99999,
            height: 1,
            volume: 500,
            card_opacity: 3.0,
            corner_radius: 900.0,
            monitor: -7,
            ..Config::default()
        };
        let w = validate(&mut cfg);
        assert_eq!(cfg.width, WIDTH_RANGE.1);
        assert_eq!(cfg.height, HEIGHT_RANGE.0);
        assert_eq!(cfg.volume, 100);
        assert_eq!(cfg.card_opacity, 1.0);
        assert_eq!(cfg.monitor, -1);
        // The radius is bounded by half the (clamped) height, so the card cannot
        // turn into a circle that clips its own contents.
        assert!(cfg.corner_radius <= cfg.height as f32 / 2.0);
        assert!(!w.is_empty());
    }

    #[test]
    fn nan_opacity_is_replaced() {
        let mut cfg = Config {
            card_opacity: f32::NAN,
            ..Config::default()
        };
        validate(&mut cfg);
        assert_eq!(cfg.card_opacity, 0.78);
    }

    #[test]
    fn extensions_are_normalised() {
        let mut cfg = Config {
            extensions: vec![" .MP3 ".into(), "flac".into(), "mp3".into(), "".into()],
            ..Config::default()
        };
        validate(&mut cfg);
        assert_eq!(cfg.extensions, vec!["mp3".to_string(), "flac".to_string()]);
    }

    #[test]
    fn empty_extension_list_is_restored() {
        let mut cfg = Config {
            extensions: vec!["  ".into()],
            ..Config::default()
        };
        let w = validate(&mut cfg);
        assert!(!cfg.extensions.is_empty());
        assert!(w.iter().any(|m| m.contains("extensions")));
    }

    #[test]
    fn library_blanks_and_duplicates_are_cleaned() {
        let mut cfg = Config {
            library: vec!["C:\\M".into(), "  ".into(), "C:\\M".into(), "D:\\N".into()],
            ..Config::default()
        };
        validate(&mut cfg);
        assert_eq!(cfg.library, vec!["C:\\M".to_string(), "D:\\N".to_string()]);
    }

    #[test]
    fn missing_dirs_are_reported_but_kept() {
        let mut cfg = Config {
            library: vec!["Z:\\definitely\\not\\here".into()],
            ..Config::default()
        };
        validate(&mut cfg);
        assert_eq!(cfg.missing_dirs().len(), 1, "unplugged drives must survive");
        assert_eq!(cfg.library.len(), 1);
    }

    #[test]
    fn dir_precedence_is_flag_then_portable_then_env_then_exe_then_appdata() {
        let exe = Path::new("C:\\app");
        let appdata = Path::new("C:\\Users\\x\\AppData\\Roaming");
        let flag = Path::new("C:\\flag");

        assert_eq!(
            choose_dir(Some(flag), true, Some("C:\\env"), exe, appdata),
            PathBuf::from("C:\\flag")
        );
        assert_eq!(
            choose_dir(None, true, Some("C:\\env"), exe, appdata),
            PathBuf::from("C:\\app"),
            "--portable wins over the env var"
        );
        assert_eq!(
            choose_dir(None, false, Some("C:\\env"), exe, appdata),
            PathBuf::from("C:\\env")
        );
        assert_eq!(
            choose_dir(None, false, Some("   "), exe, appdata),
            appdata.join("DesktopMusicWidget"),
            "a blank env var is ignored"
        );
        assert_eq!(
            choose_dir(None, false, None, exe, appdata),
            appdata.join("DesktopMusicWidget"),
            "no config.toml next to the exe means %APPDATA%"
        );
    }

    #[test]
    fn legacy_path_is_under_the_old_app_name() {
        let p = legacy_config_path();
        assert!(p.to_string_lossy().contains("HorizMusicWidget"));
        assert!(p.to_string_lossy().contains("config.toml"));
    }

    #[test]
    fn serialised_config_is_human_editable() {
        let text = to_toml(&Config::default());
        for key in [
            "config_version",
            "scan_recursive",
            "exclude_dirs",
            "play_mode",
            "autoplay",
            "autostart",
            "monitor",
            "theme",
        ] {
            assert!(text.contains(key), "config.toml omits {key}");
        }
        assert!(text.contains("[theme]"), "theme must be a readable table");
    }
}
