//! Audio engine: a dedicated thread owning the rodio sink and player.
//!
//! The sink and player must never be dropped or playback stops, so both live for
//! the whole lifetime of this thread.
//!
//! Track advancing relies on a behaviour verified against rodio 0.22: `Player`
//! plays appended sources sequentially and removes a source from its queue the
//! moment it finishes, so `Player::len()` dropping from 2 to 1 means the queued
//! track has just become the current one. We always keep one track queued ahead
//! so the hand-off is seamless.

use crate::i18n::{tr, Key};
// `HostTrait` / `DeviceTrait` are not re-exported by `rodio` itself, but `cpal`
// is, and `default_output_device()` / `id()` / `name()` are trait methods — so
// both traits have to be in scope for the device polling below to resolve.
// `Source` is here for `try_seek`, which is how a track resumes on a new device.
use rodio::{
    cpal, cpal::traits::HostTrait, DeviceSinkBuilder, DeviceTrait, MixerDeviceSink, Player, Source,
};
use std::collections::VecDeque;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

/// rodio's `set_volume` is linear amplitude, which is perceptually wrong, so map
/// the 0-100 UI slider through a power curve. Exponent 2.5 with a default of 70
/// was calibrated by ear: 0.28 gain was too quiet and 0.65 too loud.
pub fn gain_for(ui_volume: u32) -> f32 {
    let x = ui_volume.min(100) as f32 / 100.0;
    if x <= 0.0 {
        0.0
    } else {
        x.powf(2.5)
    }
}

/// How the next track is chosen once the current one ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayMode {
    Sequential,
    Shuffle,
    RepeatOne,
}

impl PlayMode {
    pub fn from_config(value: &str) -> Self {
        match value {
            "sequential" => Self::Sequential,
            "repeat-one" => Self::RepeatOne,
            _ => Self::Shuffle,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sequential => "sequential",
            Self::Shuffle => "shuffle",
            Self::RepeatOne => "repeat-one",
        }
    }
}

/// What the audio thread is currently playing. The UI copies this out and decodes
/// cover art itself (WIC/Direct2D objects are not shareable across threads here).
#[derive(Default, Clone)]
pub struct NowPlaying {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
}

/// Shared with the UI thread. Scalars are atomics; the text is behind a mutex and
/// guarded by `generation`, which the audio thread bumps whenever it changes.
pub struct PlayerState {
    pub pos_ms: AtomicU64,
    pub dur_ms: AtomicU64,
    pub playing: AtomicBool,
    pub volume: AtomicU32,
    pub generation: AtomicU64,
    pub now: Mutex<NowPlaying>,
    /// Set when the output device could not be opened (e.g. not ready at boot).
    pub device_error: AtomicBool,
}

impl PlayerState {
    pub fn new(volume: u32) -> Self {
        Self {
            pos_ms: AtomicU64::new(0),
            dur_ms: AtomicU64::new(0),
            playing: AtomicBool::new(false),
            volume: AtomicU32::new(volume.min(100)),
            generation: AtomicU64::new(0),
            now: Mutex::new(NowPlaying::default()),
            device_error: AtomicBool::new(false),
        }
    }
}

pub enum Cmd {
    /// Replace the track pool and start playing.
    SetQueue(Vec<PathBuf>),
    /// Add to the track pool without disturbing what is playing.
    Enqueue(Vec<PathBuf>),
    /// Replace the track pool but keep the current track playing. Used when the
    /// user changes the scan scope and the library is rescanned.
    SetPool(Vec<PathBuf>),
    SetPlayMode(PlayMode),
    /// Gate automatic start. Turning it off mid-session never stops playback.
    SetAutoplay(bool),
    TogglePause,
    Next,
    Prev,
    /// Seek to a fraction (0.0-1.0) of the current track.
    SeekFraction(f32),
    Volume(u32),
    Quit,
}

#[derive(Clone)]
pub struct AudioHandle {
    tx: mpsc::Sender<Cmd>,
}

impl AudioHandle {
    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }
}

/// Spawns the audio thread. Playback starts as soon as a queue is set, unless
/// autoplay is switched off.
pub fn spawn(state: Arc<PlayerState>) -> AudioHandle {
    let (tx, rx) = mpsc::channel::<Cmd>();
    std::thread::Builder::new()
        .name("audio".into())
        .spawn(move || audio_thread(rx, state))
        .expect("failed to spawn audio thread");
    AudioHandle { tx }
}

fn lofty_duration(path: &Path) -> Option<Duration> {
    use lofty::file::AudioFile;
    let tf = lofty::read_from_path(path).ok()?;
    let d = tf.properties().duration();
    if d.is_zero() {
        None
    } else {
        Some(d)
    }
}

/// Reads just the title/artist, falling back to the file stem when untagged
/// (common for WAV).
fn read_tags(path: &Path) -> (String, String) {
    use lofty::file::TaggedFileExt;
    use lofty::tag::Accessor;

    let fallback = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| tr(Key::UnknownTrack).to_string());

    let Ok(tf) = lofty::read_from_path(path) else {
        return (fallback, String::new());
    };
    let Some(tag) = tf.primary_tag().or_else(|| tf.first_tag()) else {
        return (fallback, String::new());
    };
    let title = tag
        .title()
        .map(|c| c.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback);
    let artist = tag
        .artist()
        .map(|c| c.trim().to_string())
        .unwrap_or_default();
    (title, artist)
}

fn decode(path: &Path) -> Option<rodio::decoder::Decoder<BufReader<File>>> {
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);
    // with_gapless trims LAME/Xing padding so MP3s do not click between tracks.
    // FLAC/WAV are inherently gapless and get the plain decoder.
    let is_mp3 = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("mp3"))
        .unwrap_or(false);
    if is_mp3 {
        rodio::Decoder::builder()
            .with_data(reader)
            .with_hint("mp3")
            .with_gapless(true)
            .build()
            .ok()
    } else {
        rodio::Decoder::new(reader).ok()
    }
}

/// Picks the next track. `RepeatOne` replays the current track, falling back to
/// the sequential cursor for the very first pick.
fn pick(
    pool: &[PathBuf],
    mode: PlayMode,
    seq_pos: &mut usize,
    current: Option<&PathBuf>,
    rng: &mut rand::rngs::ThreadRng,
) -> Option<PathBuf> {
    if pool.is_empty() {
        return None;
    }
    let next_sequential = |seq_pos: &mut usize| {
        let p = pool[*seq_pos % pool.len()].clone();
        *seq_pos = (*seq_pos + 1) % pool.len();
        p
    };
    match mode {
        PlayMode::Sequential => Some(next_sequential(seq_pos)),
        PlayMode::Shuffle => {
            use rand::Rng;
            Some(pool[rng.random_range(0..pool.len())].clone())
        }
        PlayMode::RepeatOne => Some(match current {
            Some(p) => p.clone(),
            None => next_sequential(seq_pos),
        }),
    }
}

/// How often to ask Windows which output device is the default.
const DEVICE_POLL: Duration = Duration::from_secs(2);

/// Opens the *current* default output device and reports which one it is.
///
/// `DeviceSinkBuilder::open_default_sink()` cannot be used for this. It does not
/// say which device it picked, so a later change cannot be noticed; and when the
/// default device fails to open it falls back to enumerating every output device
/// and opening the first one that works — which can quietly be the speakers while
/// Windows' default is the headphones.
fn open_default_device() -> Option<(MixerDeviceSink, String, String)> {
    let device = cpal::default_host().default_output_device()?;
    let key = device.id().ok()?.to_string();
    // `description()` rather than the deprecated `name()`, which rodio points at
    // for a full device description; only the display name is wanted here.
    let name = device
        .description()
        .map(|d| d.name().to_string())
        .unwrap_or_else(|_| "unknown output device".to_string());
    let sink = DeviceSinkBuilder::from_device(device)
        .ok()?
        .open_stream()
        .ok()?;
    Some((sink, key, name))
}

/// Identity of the current default output device, or `None` when Windows reports
/// no output device at all.
fn default_device_key() -> Option<String> {
    cpal::default_host()
        .default_output_device()?
        .id()
        .ok()
        .map(|id| id.to_string())
}

fn audio_thread(rx: mpsc::Receiver<Cmd>, state: Arc<PlayerState>) {
    // At boot the audio device may not be up yet, so retry rather than dying.
    let mut sink: Option<MixerDeviceSink> = None;
    // Identity of the device the sink is open on, so a change can be noticed.
    let mut device_key: Option<String> = None;
    for attempt in 0..40 {
        match open_default_device() {
            Some((s, key, name)) => {
                crate::log_info!("output device: {name}");
                device_key = Some(key);
                sink = Some(s);
                break;
            }
            None => {
                if attempt == 0 {
                    crate::log_warn!("no default output device yet, retrying");
                }
                state.device_error.store(true, Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(500));
            }
        }
        // Allow an early exit while retrying.
        if let Ok(Cmd::Quit) = rx.try_recv() {
            return;
        }
    }
    let mut sink = match sink {
        Some(s) => s,
        None => {
            // Nothing on the default device. Fall back to whatever rodio can open
            // so the widget is not silent, but say so: that device may not be the
            // one Windows considers default, which is exactly the "music comes out
            // of the speakers while everything else uses the headphones" report.
            match DeviceSinkBuilder::open_default_sink() {
                Ok(s) => {
                    crate::log_warn!(
                        "could not open the default output device; using a fallback device"
                    );
                    s
                }
                Err(e) => {
                    crate::log_error!("giving up on the audio device ({e})");
                    return;
                }
            }
        }
    };
    state.device_error.store(false, Ordering::Relaxed);

    let mut player = Player::connect_new(sink.mixer());
    player.set_volume(gain_for(state.volume.load(Ordering::Relaxed)));
    // NOTE: play() must be called once a source is queued — calling it on an
    // empty player is a no-op and the player stays paused.

    // Every track the library walk has found so far.
    let mut pool: Vec<PathBuf> = Vec::new();
    let mut rng = rand::rng();
    let mut mode = PlayMode::Shuffle;
    // Automatic start. Cleared by the UI when the user turns autoplay off.
    let mut autoplay = true;
    let mut seq_pos = 0usize;
    // Set by Prev so the next fill plays the previous track instead of picking.
    let mut override_next: Option<PathBuf> = None;
    // Tracks appended to the player but not yet started, in order.
    let mut appended: VecDeque<(PathBuf, Duration)> = VecDeque::new();
    // The track currently audible.
    let mut current: Option<(PathBuf, Duration)> = None;
    let mut history: Vec<PathBuf> = Vec::new();
    // Set whenever the queue is rebuilt; cleared once playback actually starts.
    let mut needs_play = true;
    // Set when a device change has to resume playback even though `autoplay` is
    // off, because the user was already listening.
    let mut force_play = false;
    let mut last_device_check = Instant::now();
    // Where to resume the current track after an output-device change.
    let mut pending_seek: Option<(PathBuf, Duration)> = None;
    // The last default-device key that could not be opened, so a genuinely
    // unopenable device is reported once instead of every two seconds.
    let mut last_failed_key: Option<String> = None;

    let publish = |state: &Arc<PlayerState>, current: &Option<(PathBuf, Duration)>| {
        let (path, dur) = match current {
            Some((p, d)) => (p.clone(), *d),
            None => {
                state.dur_ms.store(0, Ordering::Relaxed);
                return;
            }
        };
        let (title, artist) = read_tags(&path);
        if let Ok(mut now) = state.now.lock() {
            now.path = path;
            now.title = title;
            now.artist = artist;
        }
        state
            .dur_ms
            .store(dur.as_millis() as u64, Ordering::Relaxed);
        state.generation.fetch_add(1, Ordering::Relaxed);
    };

    'outer: loop {
        // --- 1. drain commands -------------------------------------------------
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                Cmd::Quit => break 'outer,
                Cmd::Enqueue(paths) => pool.extend(paths),
                Cmd::SetPool(paths) => {
                    // Replace the pool without clearing the player: changing the
                    // scan scope must not interrupt the track being listened to.
                    pool = paths;
                    seq_pos = 0;
                }
                Cmd::SetPlayMode(m) => mode = m,
                Cmd::SetAutoplay(v) => {
                    autoplay = v;
                    if !v {
                        // Cancel a pending automatic start, but never pause
                        // something that is already playing.
                        needs_play = false;
                    }
                }
                Cmd::SetQueue(paths) => {
                    pending_seek = None;
                    player.clear();
                    appended.clear();
                    current = None;
                    history.clear();
                    pool = paths;
                    seq_pos = 0;
                    needs_play = true;
                }
                Cmd::TogglePause => {
                    if player.is_paused() {
                        player.play();
                    } else {
                        player.pause();
                    }
                    needs_play = false;
                }
                Cmd::Next => {
                    player.clear();
                    appended.clear();
                    if let Some((p, _)) = current.take() {
                        history.push(p);
                    }
                    needs_play = true;
                }
                Cmd::Prev => {
                    if let Some(prev) = history.pop() {
                        player.clear();
                        appended.clear();
                        // The track being skipped goes back on the history so a
                        // following Next can return to it.
                        if let Some((p, _)) = current.take() {
                            history.push(p);
                        }
                        override_next = Some(prev);
                        needs_play = true;
                    }
                }
                Cmd::SeekFraction(f) => {
                    let dur = Duration::from_millis(state.dur_ms.load(Ordering::Relaxed));
                    if !dur.is_zero() {
                        let target = dur.mul_f32(f.clamp(0.0, 1.0));
                        // Some sources cannot seek; the UI hides seeking for those.
                        let _ = player.try_seek(target);
                    }
                }
                Cmd::Volume(v) => {
                    let v = v.min(100);
                    state.volume.store(v, Ordering::Relaxed);
                    player.set_volume(gain_for(v));
                }
            }
        }

        // --- 1b. follow the Windows default output device ---------------------
        // cpal binds a stream to the endpoint that was default when it was opened
        // and never follows later changes. Plugging in headphones therefore moves
        // every other application to the headphones while this player keeps
        // feeding the speakers. Re-open on the new default, restarting the current
        // track there.
        if last_device_check.elapsed() >= DEVICE_POLL {
            last_device_check = Instant::now();
            let now = default_device_key();
            if now.is_some() && now != device_key {
                match open_default_device() {
                    Some((new_sink, key, name)) => {
                        let resume = current.as_ref().map(|(p, _)| p.clone());
                        // Remember where to pick up, clamped just short of the end
                        // so a seek cannot run past the last sample.
                        let resume_pos = player.get_pos();
                        let resume_pos = match current.as_ref().map(|(_, d)| *d) {
                            Some(d) if !d.is_zero() && resume_pos + Duration::from_secs(1) >= d => {
                                d.saturating_sub(Duration::from_secs(1))
                            }
                            _ => resume_pos,
                        };
                        pending_seek = resume.clone().map(|p| (p, resume_pos));
                        // Keep the paused/playing state across the switch: someone
                        // who paused and then plugged in headphones must not have
                        // playback start on its own.
                        let was_playing = !player.is_paused() && player.len() > 0;
                        force_play = was_playing;
                        crate::log_info!("output device changed to {name}; reopening");
                        sink = new_sink;
                        player = Player::connect_new(sink.mixer());
                        player.set_volume(gain_for(state.volume.load(Ordering::Relaxed)));
                        appended.clear();
                        current = None;
                        override_next = resume;
                        device_key = Some(key);
                        last_failed_key = None;
                        needs_play = was_playing;
                    }
                    None => {
                        if last_failed_key != now {
                            crate::log_warn!(
                                "the default output device changed but could not be opened; keeping the current one"
                            );
                            last_failed_key = now;
                        }
                    }
                }
            }
        }

        // --- 2. publish the first track once something is queued --------------
        if current.is_none() {
            if let Some(entry) = appended.pop_front() {
                current = Some(entry);
                publish(&state, &current);
            }
        }

        // --- 3. detect the hand-off to the next track -------------------------
        // The player holds `current` plus everything in `appended`, so a shortfall
        // means a source finished and the head of `appended` is now audible.
        //
        // This used to watch for `player.len()` dropping, which looks natural but
        // is wrong: the refill below runs first and restores len to 2, masking the
        // 2 -> 1 transition. Auto-advance then never published, so the title,
        // artist, cover and duration all stayed on the previous track. Comparing
        // against the accounting invariant is immune to refill ordering.
        let queued = usize::from(current.is_some()) + appended.len();
        if player.len() < queued {
            if let Some((p, _)) = current.take() {
                history.push(p);
                if history.len() > 200 {
                    history.remove(0);
                }
            }
            if let Some(entry) = appended.pop_front() {
                current = Some(entry);
            }
            publish(&state, &current);
        }

        // --- 4. keep one track queued ahead of the current ---------------------
        while player.len() < 2 {
            let current_path = current.as_ref().map(|(p, _)| p);
            let Some(path) = override_next
                .take()
                .or_else(|| pick(&pool, mode, &mut seq_pos, current_path, &mut rng))
            else {
                break;
            };
            let dur = lofty_duration(&path).unwrap_or(Duration::ZERO);
            match decode(&path) {
                Some(mut src) => {
                    // Seek the decoder *before* queueing it. Player::try_seek
                    // returns early without doing anything while no sound is playing
                    // yet, which is exactly the state of a player that was just
                    // rebuilt for a new output device.
                    if let Some((want, pos)) = pending_seek.take() {
                        if want == path {
                            if let Err(e) = src.try_seek(pos) {
                                crate::log_warn!("cannot resume at {pos:?}: {e:?}");
                            }
                        } else {
                            pending_seek = Some((want, pos));
                        }
                    }
                    player.append(src);
                    appended.push_back((path, dur));
                }
                None => crate::log_warn!("cannot decode {}", path.display()),
            }
        }

        // --- 5. start playback once a source is actually queued ---------------
        if needs_play && (autoplay || force_play) && player.len() > 0 {
            player.play();
            needs_play = false;
            force_play = false;
        }

        // --- 6. publish position ----------------------------------------------
        let len = player.len();
        state
            .pos_ms
            .store(player.get_pos().as_millis() as u64, Ordering::Relaxed);
        state
            .playing
            .store(!player.is_paused() && len > 0, Ordering::Relaxed);

        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_curve_is_monotonic_and_bounded() {
        assert_eq!(gain_for(0), 0.0);
        assert!((gain_for(100) - 1.0).abs() < 1e-6);
        let mut last = -1.0;
        for v in 0..=100 {
            let g = gain_for(v);
            assert!(g >= last, "gain must not decrease at {v}");
            assert!((0.0..=1.0).contains(&g));
            last = g;
        }
    }

    #[test]
    fn volume_above_100_is_clamped() {
        assert_eq!(gain_for(250), gain_for(100));
    }

    #[test]
    fn play_mode_maps_from_and_to_config_strings() {
        for mode in [PlayMode::Sequential, PlayMode::Shuffle, PlayMode::RepeatOne] {
            assert_eq!(PlayMode::from_config(mode.as_str()), mode);
        }
        // Unknown values fall back to the historical default.
        assert_eq!(PlayMode::from_config("nonsense"), PlayMode::Shuffle);
        assert_eq!(PlayMode::from_config(""), PlayMode::Shuffle);
    }

    #[test]
    fn sequential_pick_wraps_around() {
        let pool = vec![PathBuf::from("a"), PathBuf::from("b")];
        let mut pos = 0usize;
        let mut rng = rand::rng();
        let mut next = || {
            pick(&pool, PlayMode::Sequential, &mut pos, None, &mut rng)
                .unwrap()
                .to_string_lossy()
                .to_string()
        };
        assert_eq!(next(), "a");
        assert_eq!(next(), "b");
        assert_eq!(next(), "a", "wraps instead of ending");
    }

    #[test]
    fn repeat_one_replays_the_current_track() {
        let pool = vec![PathBuf::from("a"), PathBuf::from("b")];
        let mut pos = 0usize;
        let mut rng = rand::rng();
        let current = PathBuf::from("b");
        for _ in 0..3 {
            let got = pick(
                &pool,
                PlayMode::RepeatOne,
                &mut pos,
                Some(&current),
                &mut rng,
            )
            .unwrap();
            assert_eq!(got, current);
        }
    }

    #[test]
    fn repeat_one_falls_back_to_sequential_without_a_current_track() {
        let pool = vec![PathBuf::from("a"), PathBuf::from("b")];
        let mut pos = 0usize;
        let mut rng = rand::rng();
        assert_eq!(
            pick(&pool, PlayMode::RepeatOne, &mut pos, None, &mut rng).unwrap(),
            PathBuf::from("a")
        );
        assert_eq!(
            pos, 1,
            "the cursor advances so the first pick is not repeated"
        );
    }

    #[test]
    fn empty_pool_picks_nothing() {
        let mut pos = 0usize;
        let mut rng = rand::rng();
        for mode in [PlayMode::Sequential, PlayMode::Shuffle, PlayMode::RepeatOne] {
            assert!(pick(&[], mode, &mut pos, None, &mut rng).is_none());
        }
    }

    #[test]
    fn shuffle_only_returns_tracks_from_the_pool() {
        let pool = vec![PathBuf::from("a"), PathBuf::from("b")];
        let mut pos = 0usize;
        let mut rng = rand::rng();
        for _ in 0..50 {
            let got = pick(&pool, PlayMode::Shuffle, &mut pos, None, &mut rng).unwrap();
            assert!(pool.contains(&got));
        }
    }
}
