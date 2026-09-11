//! Background library scanning.
//!
//! Walking a large collection (thousands of files, possibly on a drive that has
//! not spun up yet) takes long enough that startup cannot block on it, so the
//! walk sends two waves:
//!
//! 1. **Starter** — the contents of one randomly chosen top-level folder.
//!    Listing the top level plus a single album folder takes milliseconds, so
//!    playback begins immediately with a genuinely random album.
//! 2. **Rest** — the full library, once the walk finishes.
//!
//! Progress is published through atomics rather than a channel: the UI only ever
//! needs the current state, and the settings window polls a separate lightweight
//! preview scan.

use crate::audio::{AudioHandle, Cmd};
use crate::config::Config;
use rand::seq::IndexedRandom;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Files collected between progress reports.
const PROGRESS_EVERY: usize = 256;
/// Cheap insurance against a junction/symlink cycle turning the walk infinite.
const MAX_DEPTH: usize = 64;
/// At boot the drive may not be spun up yet; retry before calling it empty.
const RETRIES: usize = 12;

pub const STATE_IDLE: u32 = 0;
pub const STATE_SCANNING: u32 = 1;
pub const STATE_READY: u32 = 2;
pub const STATE_EMPTY: u32 = 3;
pub const STATE_NO_DIRS: u32 = 4;

/// What the library currently looks like, for the card's status line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanStatus {
    Idle,
    /// Still walking; `found` is a running count.
    Scanning {
        found: usize,
    },
    Ready {
        total: usize,
    },
    /// Folders configured, but no playable files in them.
    Empty,
    /// No folders configured at all (first run).
    NoDirs,
}

/// Shared scan status, readable from the UI thread on every timer tick.
#[derive(Debug)]
pub struct ScanState {
    state: AtomicU32,
    total: AtomicUsize,
}

impl ScanState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: AtomicU32::new(STATE_IDLE),
            total: AtomicUsize::new(0),
        })
    }

    pub fn status(&self) -> ScanStatus {
        let total = self.total.load(Ordering::Relaxed);
        match self.state.load(Ordering::Relaxed) {
            STATE_SCANNING => ScanStatus::Scanning { found: total },
            STATE_READY => ScanStatus::Ready { total },
            STATE_EMPTY => ScanStatus::Empty,
            STATE_NO_DIRS => ScanStatus::NoDirs,
            _ => ScanStatus::Idle,
        }
    }

    fn set(&self, state: u32, total: usize) {
        self.total.store(total, Ordering::Relaxed);
        self.state.store(state, Ordering::Relaxed);
    }

    /// Publishes a running count while a walk is in progress.
    pub fn report(&self, n: usize) {
        self.total.store(n, Ordering::Relaxed);
    }
}

/// Everything the scanner needs, taken from the config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanOptions {
    pub dirs: Vec<PathBuf>,
    pub recursive: bool,
    /// Folder names or absolute paths, as typed by the user.
    pub excludes: Vec<String>,
    /// Extensions without a leading dot.
    pub extensions: Vec<String>,
}

impl ScanOptions {
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            dirs: cfg.library_paths(),
            recursive: cfg.scan_recursive,
            excludes: cfg.exclude_dirs.clone(),
            extensions: cfg.extensions.clone(),
        }
    }

    pub fn has_dirs(&self) -> bool {
        !self.dirs.is_empty()
    }
}

/// Pre-lowercased view of the options so the per-file hot path does no
/// allocation.
struct Prepared {
    recursive: bool,
    excludes: Vec<String>,
    extensions: Vec<String>,
}

impl Prepared {
    fn new(opts: &ScanOptions) -> Self {
        Self {
            recursive: opts.recursive,
            excludes: opts
                .excludes
                .iter()
                .map(|e| e.trim().to_lowercase())
                .filter(|e| !e.is_empty())
                .collect(),
            extensions: opts
                .extensions
                .iter()
                .map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase())
                .filter(|e| !e.is_empty())
                .collect(),
        }
    }
}

fn is_audio(path: &Path, p: &Prepared) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => {
            let ext = ext.to_ascii_lowercase();
            p.extensions.contains(&ext)
        }
        None => false,
    }
}

/// An excluded entry is either a bare folder name (matched against any folder
/// with that name) or a path/prefix (matched against the full path). The two
/// forms are told apart by the presence of a separator or drive letter.
fn is_excluded(path: &Path, p: &Prepared) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();
    if p.excludes.contains(&name) {
        return true;
    }
    let full = path.to_string_lossy().to_lowercase();
    p.excludes
        .iter()
        .filter(|e| e.contains('\\') || e.contains('/') || e.contains(':'))
        .any(|e| full.starts_with(e.as_str()))
}

/// Recursively collects audio files under `dir`. Unreadable directories are
/// skipped rather than aborting the whole walk.
fn walk_into(
    dir: &Path,
    p: &Prepared,
    depth: usize,
    out: &mut Vec<PathBuf>,
    publish: &mut dyn FnMut(usize),
) {
    if depth > MAX_DEPTH {
        crate::log_warn!("[library] depth limit reached at {}", dir.display());
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if !p.recursive || is_excluded(&path, p) {
                continue;
            }
            walk_into(&path, p, depth + 1, out, publish);
        } else if path.is_file() && !is_excluded(&path, p) && is_audio(&path, p) {
            // Files are checked against the excludes too: excluding a folder path
            // must also drop the loose tracks sitting directly inside it.
            out.push(path);
            if out.len() % PROGRESS_EVERY == 0 {
                publish(out.len());
            }
        }
    }
}

fn walk_all(dirs: &[PathBuf], p: &Prepared, publish: &mut dyn FnMut(usize)) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for d in dirs {
        walk_into(d, p, 0, &mut out, publish);
    }
    out
}

/// Picks a random top-level entry (file or directory) and returns its tracks.
/// Falls back to a shallow scan of the library roots when the choice yields
/// nothing.
fn pick_starter(p: &Prepared, dirs: &[PathBuf], rng: &mut rand::rngs::ThreadRng) -> Vec<PathBuf> {
    let mut entries: Vec<PathBuf> = Vec::new();
    for d in dirs {
        if let Ok(rd) = std::fs::read_dir(d) {
            entries.extend(
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|x| !is_excluded(x, p)),
            );
        }
    }
    if entries.is_empty() {
        return Vec::new();
    }

    // Prefer directories so a starter is a whole album rather than one orphan file.
    let dirs_only: Vec<&PathBuf> = entries.iter().filter(|x| x.is_dir()).collect();
    let chosen: Option<&PathBuf> = if !dirs_only.is_empty() {
        dirs_only.choose(rng).copied()
    } else {
        entries.choose(rng)
    };

    let mut out = Vec::new();
    if let Some(c) = chosen {
        let mut noop = |_: usize| {};
        walk_into(c, p, 0, &mut out, &mut noop);
    }
    if out.is_empty() {
        // Last resort: the loose files sitting directly in the library roots.
        out = entries.into_iter().filter(|x| is_audio(x, p)).collect();
    }
    out
}

/// What the caller wants the found tracks to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanTarget {
    /// Startup: play a starter album immediately, then queue the rest.
    Play,
    /// Rescan: replace the track pool but keep the current track playing.
    Pool,
}

/// Starts the walk on a background thread. Everything it finds is sent straight
/// to the audio thread as it becomes available.
pub fn spawn(opts: ScanOptions, target: ScanTarget, audio: AudioHandle, state: Arc<ScanState>) {
    let _ = std::thread::Builder::new()
        .name("library".into())
        .spawn(move || run(opts, target, audio, state));
}

fn run(opts: ScanOptions, target: ScanTarget, audio: AudioHandle, state: Arc<ScanState>) {
    if !opts.has_dirs() {
        crate::log_info!("[library] no folders configured");
        state.set(STATE_NO_DIRS, 0);
        return;
    }

    let p = Prepared::new(&opts);
    state.set(STATE_SCANNING, 0);

    if target == ScanTarget::Pool {
        let mut publish = |n: usize| state.report(n);
        let all = walk_all(&opts.dirs, &p, &mut publish);
        let total = all.len();
        crate::log_info!("[library] rescan complete: {total} files");
        state.set(if total == 0 { STATE_EMPTY } else { STATE_READY }, total);
        audio.send(Cmd::SetPool(all));
        return;
    }

    // Wave 1: something to play right now.
    let mut rng = rand::rng();
    let starter = pick_starter(&p, &opts.dirs, &mut rng);
    if !starter.is_empty() {
        crate::log_info!("[library] starter album: {} files", starter.len());
        audio.send(Cmd::SetQueue(starter.clone()));
    }

    // Wave 2: the whole library. Retry a few times because the drive may not be
    // spun up yet at boot.
    let mut all: Vec<PathBuf> = Vec::new();
    for attempt in 0..RETRIES {
        let mut publish = |n: usize| state.report(n);
        all = walk_all(&opts.dirs, &p, &mut publish);
        if !all.is_empty() {
            break;
        }
        if attempt == 0 {
            crate::log_info!("[library] no files found yet, waiting for the drive");
        }
        std::thread::sleep(Duration::from_millis(1000));
    }

    // The starter is already playing; queueing it again would replay the album.
    if !starter.is_empty() {
        let seen: HashSet<&PathBuf> = starter.iter().collect();
        all.retain(|x| !seen.contains(x));
    }

    let total = all.len() + starter.len();
    crate::log_info!("[library] walk complete: {total} files");
    state.set(if total == 0 { STATE_EMPTY } else { STATE_READY }, total);
    audio.send(Cmd::Enqueue(all));
}

/// One-shot count, used by `--count-tracks` so CI can cover the scanner without
/// a GUI.
pub fn count_tracks(opts: &ScanOptions) -> usize {
    let p = Prepared::new(opts);
    let mut noop = |_: usize| {};
    walk_all(&opts.dirs, &p, &mut noop).len()
}

// ---------------------------------------------------------------------------
// Preview scan (settings window / first-run wizard)
// ---------------------------------------------------------------------------

/// A lightweight "how many tracks would this find?" scan. The settings window
/// polls it on a timer while the user toggles recursion or excludes.
#[derive(Debug)]
pub struct PreviewState {
    running: AtomicBool,
    done: AtomicBool,
    count: AtomicUsize,
}

impl PreviewState {
    pub fn running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
    pub fn done(&self) -> bool {
        self.done.load(Ordering::Relaxed)
    }
    pub fn count(&self) -> usize {
        self.count.load(Ordering::Relaxed)
    }
}

pub fn spawn_preview(opts: ScanOptions) -> Arc<PreviewState> {
    let state = Arc::new(PreviewState {
        running: AtomicBool::new(true),
        done: AtomicBool::new(false),
        count: AtomicUsize::new(0),
    });

    if !opts.has_dirs() {
        state.running.store(false, Ordering::Relaxed);
        state.done.store(true, Ordering::Relaxed);
        return state;
    }

    let shared = Arc::clone(&state);
    let _ = std::thread::Builder::new()
        .name("preview".into())
        .spawn(move || {
            let p = Prepared::new(&opts);
            let mut publish = |n: usize| shared.count.store(n, Ordering::Relaxed);
            let all = walk_all(&opts.dirs, &p, &mut publish);
            shared.count.store(all.len(), Ordering::Relaxed);
            shared.running.store(false, Ordering::Relaxed);
            shared.done.store(true, Ordering::Relaxed);
        });

    state
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Builds a throwaway tree. Each test uses its own name because tests run in
    /// parallel inside one process.
    fn fixture(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("dmw-lib-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("Album")).unwrap();
        fs::create_dir_all(root.join("Skip")).unwrap();
        fs::write(root.join("loose.mp3"), b"x").unwrap();
        fs::write(root.join("notes.txt"), b"x").unwrap();
        fs::write(root.join("Album").join("one.mp3"), b"x").unwrap();
        fs::write(root.join("Album").join("two.FLAC"), b"x").unwrap();
        fs::write(root.join("Skip").join("three.mp3"), b"x").unwrap();
        root
    }

    fn opts(root: &Path) -> ScanOptions {
        ScanOptions {
            dirs: vec![root.to_path_buf()],
            recursive: true,
            excludes: Vec::new(),
            extensions: crate::config::DEFAULT_EXTENSIONS
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }

    #[test]
    fn finds_audio_recursively_and_ignores_other_files() {
        let root = fixture("recurse");
        // loose.mp3, Album/one.mp3, Album/two.FLAC (case-insensitive), Skip/three.mp3
        assert_eq!(count_tracks(&opts(&root)), 4);
    }

    #[test]
    fn non_recursive_stops_at_the_top_level() {
        let root = fixture("flat");
        let mut o = opts(&root);
        o.recursive = false;
        assert_eq!(count_tracks(&o), 1, "only loose.mp3");
    }

    #[test]
    fn exclusion_by_folder_name_is_case_insensitive() {
        let root = fixture("excl-name");
        let mut o = opts(&root);
        o.excludes = vec!["skip".into()];
        assert_eq!(count_tracks(&o), 3);
    }

    #[test]
    fn exclusion_by_absolute_path_works() {
        let root = fixture("excl-path");
        let mut o = opts(&root);
        o.excludes = vec![root.join("Skip").to_string_lossy().to_string()];
        assert_eq!(count_tracks(&o), 3);
    }

    #[test]
    fn exclusion_by_path_covers_subfolders() {
        let root = fixture("excl-prefix");
        let mut o = opts(&root);
        // Excluding the root itself removes everything below it.
        o.excludes = vec![root.to_string_lossy().to_string()];
        assert_eq!(count_tracks(&o), 0);
    }

    #[test]
    fn extension_filter_is_applied() {
        let root = fixture("ext");
        let mut o = opts(&root);
        o.extensions = vec!["MP3".into()]; // normalised to lower case
        assert_eq!(count_tracks(&o), 3);
        o.extensions = vec!["flac".into()];
        assert_eq!(count_tracks(&o), 1);
    }

    #[test]
    fn empty_configuration_yields_nothing() {
        let o = ScanOptions {
            dirs: Vec::new(),
            recursive: true,
            excludes: Vec::new(),
            extensions: vec!["mp3".into()],
        };
        assert_eq!(count_tracks(&o), 0);
        assert!(!o.has_dirs());
    }

    #[test]
    fn missing_directories_are_skipped_not_fatal() {
        let o = ScanOptions {
            dirs: vec![PathBuf::from("Z:\\nope\\nowhere")],
            recursive: true,
            excludes: Vec::new(),
            extensions: vec!["mp3".into()],
        };
        assert_eq!(count_tracks(&o), 0);
    }

    #[test]
    fn bare_name_excludes_do_not_match_paths() {
        // A name-only exclude must not swallow an unrelated absolute path that
        // merely begins with the same letters.
        let root = fixture("bare");
        let mut o = opts(&root);
        o.excludes = vec!["ski".into()];
        assert_eq!(count_tracks(&o), 4, "'ski' is not the folder name 'Skip'");
    }

    #[test]
    fn scan_state_reports_progress_and_completion() {
        let state = ScanState::new();
        assert_eq!(state.status(), ScanStatus::Idle);
        state.set(STATE_SCANNING, 0);
        state.report(512);
        assert_eq!(state.status(), ScanStatus::Scanning { found: 512 });
        state.set(STATE_READY, 900);
        assert_eq!(state.status(), ScanStatus::Ready { total: 900 });
        state.set(STATE_EMPTY, 0);
        assert_eq!(state.status(), ScanStatus::Empty);
        state.set(STATE_NO_DIRS, 0);
        assert_eq!(state.status(), ScanStatus::NoDirs);
    }

    #[test]
    fn preview_finishes_and_counts() {
        let root = fixture("preview");
        let state = spawn_preview(opts(&root));
        for _ in 0..200 {
            if state.done() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(state.done(), "preview must finish");
        assert!(!state.running());
        assert_eq!(state.count(), 4);
    }

    #[test]
    fn preview_without_folders_completes_immediately() {
        let state = spawn_preview(ScanOptions {
            dirs: Vec::new(),
            recursive: true,
            excludes: Vec::new(),
            extensions: vec!["mp3".into()],
        });
        assert!(state.done());
        assert_eq!(state.count(), 0);
    }
}
