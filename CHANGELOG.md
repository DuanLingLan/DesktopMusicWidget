# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.5] - 2026-09-20

### Changed

- The Show Desktop guard now checks the cached desktop anchor directly without
  walking the entire window Z-order or allocating class-name strings every 250 ms.
- Full desktop discovery now runs only when the cached anchor is invalid or the
  widget is no longer immediately above it.

[1.0.5]: https://github.com/DuanLingLan/DesktopMusicWidget/releases/tag/v1.0.5

## [1.0.4] - 2026-09-20

### Fixed

- The widget remains visible above the desktop after clicking Show Desktop or
  pressing `Win+D`.
- When the desktop is dismissed, the widget returns below normal application
  windows without taking focus.
- Desktop and Wallpaper Engine anchors are selected by their current Z-order, so
  Explorer cannot cover the widget after raising the desktop band.

[1.0.4]: https://github.com/DuanLingLan/DesktopMusicWidget/releases/tag/v1.0.4

## [1.0.3] - 2026-09-13

### Fixed

- Playback now follows the Windows default output device. cpal binds a stream to the
  endpoint that was default when the stream was opened, and never follows later
  changes, so plugging in headphones moved every other application to the headphones
  while the widget kept feeding the speakers. The default device is re-checked every
  two seconds and the stream is re-opened on the new device.
- The current track resumes from where it left off when the device changes, instead
  of restarting. The decoder is seeked *before* it is queued, because
  `Player::try_seek` returns early without doing anything while no sound is playing
  yet - which is exactly the state of a freshly rebuilt player.
- Playback state survives the switch: pausing first and then plugging in headphones
  no longer starts playback on its own.
- The chosen output device is logged at startup and whenever it changes, which is
  what makes this class of problem diagnosable from `widget.log`.

### Changed

- The output no longer goes through `open_default_sink()`, which falls back to
  enumerating every output device and opening the first one that works. On a machine
  with virtual audio devices that can silently be a device other than the default.
  The current default device is opened explicitly instead.

[1.0.3]: https://github.com/DuanLingLan/DesktopMusicWidget/releases/tag/v1.0.3
## [1.0.2] - 2026-09-12

### Changed

- Removed the once-a-second Z-order re-assert introduced in 1.0.1. It did not fix
  Show Desktop (see the known issue below) and cost roughly 0.5% of one core while
  idle, which is a bad trade for a widget whose whole point is being cheap when
  nothing is happening. The repair now happens once, where the damage is actually
  done: immediately after the context menu closes.
- Context menu handlers therefore no longer leave the card floating above ordinary
  application windows.

### Known issue

- **"Show Desktop" (the taskbar's right-hand edge, and Win+D) still takes the card
  with it.** That action does not minimise the windows it covers: it raises the
  desktop band to the top of the Z order, so a desktop-layer card is buried rather
  than minimised - `IsIconic` stays false and `IsWindowVisible` stays true, which
  is why the symptom reads as "the widget vanished but Windows insists it is
  visible". Re-inserting the card above the desktop window, once a second or once
  per event, does not survive it. The likely correct fix is to reparent the card
  into the desktop band (`SetParent` to `Progman`/`WorkerW`), which is invasive:
  input routing, clipping and the `topmost` mode all have to be reworked. Many
  desktop widgets simply accept this behaviour.

## [1.0.1] - 2026-09-12

### Fixed

- The card no longer drifts out of the desktop layer. Opening the context menu
  calls `SetForegroundWindow`, which lifts a window to the top of its band, so
  after the first right-click the card floated above ordinary application windows
  instead of sitting above the wallpaper (measured at index 52 of 230 rather than
  224). Explorer restarting had the same effect by leaving a dead Z-order anchor
  behind.

### Notes

- This release also carried a once-a-second Z-order re-assert intended to fix Show
  Desktop. **It did not work** and was removed in 1.0.2. The changelog entry was
  corrected after the fact rather than left claiming a fix that never shipped.
## [1.0.0] - 2026-09-11

First public release. DesktopMusicWidget is the published descendant of a
personal-only build and this is the first version released for other people to
use and build.

### Added

- First-run folder wizard: an empty `library` makes the app ask for folders, scan
  them, and report how many tracks it found before anything is committed.
- A settings window covering music folders, scan scope, startup and playback,
  appearance and position, and general options, with Restore defaults, Cancel,
  Apply and OK. Nothing is written until Apply or OK, except the language picker,
  which applies immediately.
- A full right-click menu, shared by the card and the tray icon: play/pause,
  next/previous, play mode, music folders, scan options, autostart, autoplay,
  show controls, window layer, monitor and move-window, followed by Settings,
  Open config file, Reload config, About and Exit.
- Scan scope control: a recursive-scan toggle, exclusions matched either by a bare
  folder name (case-insensitive, anywhere) or by a path (that folder and
  everything under it), and a live "found N tracks" preview counter that updates as
  the settings change.
- Play modes: sequential, shuffle and repeat-one.
- Autostart and autoplay toggles. Autostart writes the per-user `HKCU\...\Run`
  entry only, so it never asks for admin, and the entry is repaired at startup when
  the executable has moved.
- Multi-monitor support: choose which monitor the card sits on, plus
  drag-to-position ("Move window"), which writes absolute coordinates back to the
  config.
- Bilingual UI: English and Chinese, following the Windows UI language, with an
  explicit override in the config.
- Configurable appearance: a font family and a six-colour theme, parsed from the
  config with a fallback to the defaults when a value is unparseable.
- A rotating log file (`widget.log`, rotated at 1 MB) together with a panic hook
  that logs the panic and shows a message box naming the log file.
- Command-line modes that need no window: `--count-tracks`, `--dump-config`,
  `--reset-config` and `--open-settings`, alongside the existing help, version,
  config-directory, portable, language, autoplay, autostart and play flags.
- CI and release workflows: formatting, clippy with `-D warnings`, the test suite
  and a headless command-line smoke test on every push, and a tag-triggered release
  that builds both the GNU and the MSVC target and publishes zips with a combined
  `SHA256SUMS.txt`.

### Changed

- The configuration file is now schema v2, with automatic migration from v1. The
  only shape change is `shuffle: bool` becoming `play_mode`.
- The application was renamed to DesktopMusicWidget throughout: its own window
  classes, single-instance mutex, `HKCU\...\Run` value and config directory. A
  config left behind by the previous build is adopted on first start, and its
  autostart entry is detected and offered for removal from Settings rather than
  deleted silently.
- The autostart `Run` value is now quoted, so an executable under a path containing
  spaces actually launches instead of being split at the first space by the shell.
- The settings window and the embedded manifest give per-monitor-v2 DPI awareness
  and themed (Common Controls v6) controls.
- rodio is built with `default-features = false` and only the features the app
  uses, dropping unused decoders and their dependencies.

### Fixed

- Files sitting directly in an excluded folder are now excluded too. The exclude
  list was previously consulted only while descending into subfolders, so a
  non-recursive scan of an excluded folder still returned its files.
- A config file without a `config_version` key is now correctly treated as v1.
  Previously such a file was assumed to be current, and a `shuffle = false` setting
  was silently not migrated to `play_mode`.
- The card's status line now reports a missing music folder, an empty library and a
  missing audio device, instead of sitting on the loading text forever.
- `language = "zh-CN"` is no longer rejected. Enum values were compared after
  lowercasing only the configured value, so a value that contains capitals never
  matched the allowed list, the language was reset to `auto`, and the setting
  silently reverted on the next start. Comparison is now case-insensitive in both
  directions and the canonical spelling is written back.

[1.0.0]: https://github.com/DuanLingLan/DesktopMusicWidget/releases/tag/v1.0.0
