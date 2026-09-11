# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
