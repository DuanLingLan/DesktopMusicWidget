<div align="center">

# DesktopMusicWidget

**A lightweight, always-on-desktop music widget for Windows 10/11.**

It sits in the desktop layer — above the wallpaper and desktop icons, below your
normal windows — and plays a folder of music with a small glass card you can
click through without ever leaving what you were doing.

[![CI](https://github.com/DuanLingLan/DesktopMusicWidget/actions/workflows/ci.yml/badge.svg)](https://github.com/DuanLingLan/DesktopMusicWidget/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
![Platform](https://img.shields.io/badge/platform-Windows%2010%2F11%20x64-lightgrey)

[中文说明](README.zh-CN.md)

</div>

---

<!-- Screenshots: drop yours in as assets/screenshot-card.png, screenshot-menu.png
     and screenshot-settings.png, then uncomment the block below.
     Suggested capture: Win+Shift+S over the card, the right-click menu, and the
     settings window.

<p align="center">
  <img src="assets/screenshot-card.png" width="420" alt="The widget on the desktop">
  <img src="assets/screenshot-menu.png" width="320" alt="The right-click menu">
  <br>
  <img src="assets/screenshot-settings.png" width="520" alt="The settings window">
</p>
-->

## Features

- **Desktop layer, not a window.** Stays out of the way: never steals focus,
  never appears in Alt-Tab, never covers your work.
- **Wallpaper Engine is optional.** With it, the card stacks above the running
  wallpaper. Without it, the card anchors to the Explorer desktop band instead.
  Either way it works out of the box.
- **No installer, no runtime.** One self-contained `.exe`. Nothing to set up.
- **First-run wizard.** Pick your music folders; the app tells you how many
  tracks it found before you commit.
- **Scan scope you control.** Include or skip subfolders, exclude folders by
  name or path, and choose which extensions count as music.
- **A full right-click menu** on both the card and the tray icon: transport,
  play mode, folders, scan scope, autostart, autoplay, window layer, monitor.
- **A real settings window** for size, position, opacity, volume, font, colours,
  language and hotkeys — applied live, saved to a readable TOML file.
- **Optional autostart and autoplay.** HKCU only, so it never asks for admin.
- **Bilingual.** English and Chinese, following the Windows UI language.
- **Tiny and quiet.** ~3 MB, CPU-assisted Direct2D rendering at ~2 MB private
  memory, and it only repaints when something actually changes.

## Download and run

1. Grab `DesktopMusicWidget-v1.0.0-windows-x64-msvc.zip` (or the `-gnu` build —
   they are the same program built with two toolchains) from
   [Releases](../../releases).
2. Unzip anywhere. There is no installer.
3. Run `desktop-music-widget.exe`.
4. The first-run wizard asks for your music folders. Pick them, confirm the
   track count, and playback starts.

Optional: right-click the card (or the tray icon) → **Start with Windows**.

To verify your download, check the zip against `SHA256SUMS.txt` in the same
release.

## Using it

### The card

| Action | What it does |
|---|---|
| Hover | Reveals the transport controls |
| Click ‹ / ▶ / › | Previous / play-pause / next |
| Click or drag the bar | Seek |
| Mouse wheel | Volume (only when the card has focus; use the hotkeys otherwise) |
| Right-click | The full menu |
| `Ctrl` + drag, or **Move window** | Reposition the card; the position is saved |
| Left-click the tray icon | Play / pause |
| Double-click the tray icon | Settings |

### The right-click menu

```
Play / Pause
Next track
Previous track
─────────────────────────────
Play mode          ▸  In order / Shuffle / Repeat one
─────────────────────────────
Music folders      ▸  Add folder… / Replace with… / Open first folder / Clear
Scan options       ▸  Include subfolders / Exclude folder… / Rescan library
─────────────────────────────
Start with Windows
Play on launch
Show controls      ▸  On hover / Always
Window layer       ▸  Desktop layer (recommended) / Always on top
Monitor            ▸  Monitor 1 / Monitor 2 / …
─────────────────────────────
Move window (drag)
Settings…
Open config file / Reload config
─────────────────────────────
About DesktopMusicWidget
Exit
```

There is no `Remove` for folders in the menu because the settings window has a
list with one; the menu is for the things you change often.

### The settings window

| Section | What is in it |
|---|---|
| Music folders | The folder list, with add / remove / open |
| Scan scope | Include subfolders, the exclude list, and a **live "found N tracks"** counter that updates as you change things |
| Startup & playback | Start with Windows, play on launch, play mode, volume |
| Appearance & position | Width, height, corner radius, opacity, when to show controls, window layer, monitor, font |
| General | Language, global hotkeys and their preset |
| Footer | Restore defaults, open the config file, cancel / apply / OK |

Nothing is written until you press **Apply** or **OK**, so **Cancel** really does
discard. The one exception is the language picker, which applies immediately so
you can see what you chose.

### Keyboard

| Hotkey | Action |
|---|---|
| `Ctrl`+`Alt`+`Space` | Play / pause |
| `Ctrl`+`Alt`+`←` / `→` | Previous / next track |
| `Ctrl`+`Alt`+`↑` / `↓` | Volume up / down |

`Ctrl+Alt+arrows` are claimed by the Intel graphics driver on some machines, so
the widget automatically retries with `Shift` added. You can switch presets or
turn hotkeys off in Settings; if something still could not be registered,
Settings reports which combination.

## Wallpaper Engine

Wallpaper Engine is **not required**. The card is not a Wallpaper Engine
wallpaper and does not depend on it.

- **With Wallpaper Engine running**, the card is stacked directly above its
  window, which makes the ordering deterministic.
- **Without it**, there is nothing to stack against, so a plain "bottom of the
  Z order" placement can end up *underneath* the desktop icon layer. The widget
  therefore anchors itself to the Explorer desktop band (`Progman` / `WorkerW`)
  instead, which is the window that paints the wallpaper and icons.

If the card does not appear, run `diag.exe` — it prints the Z order, tells you
which anchor was chosen, and says whether the card is above or below the desktop
band.

## Configuration

The settings window writes `%APPDATA%\DesktopMusicWidget\config.toml`. Everything
is also documented in [`config.example.toml`](config.example.toml), and **Reload
config** in the right-click menu applies hand edits without a restart.

A minimal file:

```toml
library = ["C:\\Users\\You\\Music"]
play_mode = "shuffle"
autoplay = true
autostart = false
```

Useful fields:

| Field | Default | Notes |
|---|---|---|
| `config_version` | `2` | Schema version. Leave it alone: an absent value is read as v1 and migrated. |
| `library` | `[]` | Absolute paths. Empty triggers the first-run wizard. |
| `scan_recursive` | `true` | Scan subfolders too. |
| `exclude_dirs` | `[]` | A bare name matches any folder with that name; a path matches that folder and everything under it. |
| `extensions` | mp3, flac, wav, m4a, m4b, aac, ogg, oga, aiff | Must match what the audio backend can decode. |
| `play_mode` | `"shuffle"` | `sequential`, `shuffle`, `repeat-one`. |
| `volume` | `70` | 0–100, mapped through a power curve. |
| `autoplay` | `true` | Off loads the library but stays paused. |
| `autostart` | `false` | Writes `HKCU\...\Run`; repaired automatically if you move the exe. |
| `hotkeys_enabled` | `true` | Register the global hotkeys at all. |
| `hotkey_preset` | `"ctrl-alt"` | Or `ctrl-shift-alt`. |
| `mode` | `"bottom"` | `bottom` (desktop layer) or `topmost`. |
| `monitor` | `-1` | `-1` = primary, otherwise a zero-based index. |
| `anchor` | `"top-center"` | Or `top-left`, `top-right`, `free` (absolute coordinates). |
| `offset_x` / `offset_y` | 0 / 24 | From the anchor, in logical pixels. Absolute when `anchor = "free"`. |
| `width` / `height` | 340 / 96 | Logical pixels, 200–900 by 64–400. |
| `corner_radius` | `14.0` | 0 to `height / 2`. |
| `card_opacity` | `0.78` | 0.0–1.0. |
| `show_controls` | `"hover"` | Or `always`. |
| `font_family` | `"Segoe UI"` | Any installed font. |
| `language` | `"auto"` | `auto`, `zh-CN`, `en`. |
| `[theme]` | see below | Six `#rrggbb` / `#rrggbbaa` colours. |

`[theme]` keys are `background`, `art_bg`, `title`, `subtitle`, `bar_bg` and
`bar_fg`; a value that cannot be parsed silently falls back to the default rather
than rendering an invisible card.

Bad values never stop the app: they are clamped or reset to the default, logged
to `widget.log`, and written back so the file always describes what is running. A
corrupt file is backed up to `config.toml.bak` and replaced with defaults.

### Portable mode

If a `config.toml` sits next to the exe, that file is used and nothing is written
to `%APPDATA%`. Handy for a USB stick or a self-contained folder.

## Command line

```
desktop-music-widget.exe [OPTIONS]

  -h, --help              Show help
  -V, --version           Show version
      --config-dir <DIR>  Use DIR for config and logs
      --portable          Keep config next to the exe
      --lang <LANG>       auto | zh-CN | en
      --autoplay, --no-autoplay
                          Override autoplay for this run
      --install-autostart Add to the logon entry
      --remove-autostart  Remove the logon entry
      --open-config       Open config.toml and exit
      --open-settings     Open the settings window at startup
      --reset-config      Back up and rewrite defaults
      --dump-config       Print the effective config
      --count-tracks [--dir <DIR>]
                          Count tracks and exit
      --play <FILE>...    Play these files instead of the library
  -v, --verbose           Also log to the console
```

`--count-tracks` and `--dump-config` need no window, which is what the CI job
uses to cover the scanner and the config code on a headless runner.

## Troubleshooting

**Windows SmartScreen warns me.** The binaries are not code-signed (a
certificate costs money for a hobby project). Choose **More info** → **Run
anyway**. Verify the SHA256 first if you like.

**The card is not visible.** Run `diag.exe`. It prints the Z order, whether the
card is above or below the desktop band, and which anchor was chosen. The most
common causes are a full-screen window on top, or `mode` set to `topmost` with
something above it.

**No sound.** Check `widget.log` for "audio device not available". The app
retries the default output device for ~20 seconds at startup, because it may
start before the audio service is ready. The card shows "No audio output device"
and a tray balloon once.

**Nothing plays / "No music files in that location".** Open Settings → Scan
scope and look at the found-tracks counter. Usually it is a wrong folder, a
mismatched extension, or an exclude rule that matches more than intended (a bare
name excludes *every* folder with that name).

**It found some of my music but not all.** Opus and WMA are not supported by the
audio backend and are intentionally not in `extensions`; adding them would
produce silent failures rather than playback. `.aiff`, `.m4b` and `.aac` are
supported.

**A track shows the file name instead of the title.** That file has no title
tag — normal for WAV, and for rips that were never tagged.

**Text is blurry.** The app is per-monitor-DPI aware via an embedded manifest, so
this should not happen. If it does, check that you are not running it through a
compatibility-mode DPI override (right-click the exe → Properties →
Compatibility).

**The mouse wheel does not change the volume.** Windows sends the wheel to the
focused window, and the card deliberately never takes focus. Use the hotkeys,
the tray menu, or the settings slider.

**Chinese / Japanese text looks wrong in the settings window.** Only Latin text
was laid out with the wrong font fallback if the system locale is unusual; song
titles always use the system locale. Please open an issue with your locale.

**It starts two copies at logon.** You probably have both this build and an older
build registered. Settings has a **Remove legacy autostart** button for the
pre-rename entry.

**Uninstalling.** Exit from the tray menu, delete the folder, and (if you enabled
it) untick **Start with Windows** first, or delete the `DesktopMusicWidget` value
under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`. Settings and logs live
in `%APPDATA%\DesktopMusicWidget`.

## Building from source

Requirements: [Rust](https://rustup.rs) 1.80 or newer, and a Windows linker for
your chosen toolchain.

```powershell
git clone https://github.com/DuanLingLan/DesktopMusicWidget
cd DesktopMusicWidget

# GNU toolchain (matches what the maintainer develops against; needs MinGW's
# gcc and windres on PATH)
rustup target add x86_64-pc-windows-gnu
cargo build --release

# or MSVC (needs Visual Studio Build Tools)
cargo build --release --target x86_64-pc-windows-msvc
```

The exe lands in `target/release/`, next to `diag.exe`.

The embedded manifest, icon and version resource are generated by `build.rs`.
`assets/icon.ico` is produced by `assets/make_icon.ps1` (plain arithmetic, no
drawing library) and committed, so you only need to run that script if you change
the glyph. If no resource compiler is available the build continues with a
warning — you lose the icon and the themed controls, nothing else.

## Verifying a build

```powershell
cargo test --all-targets          # 95 unit tests: config, migration, scanner, menus, i18n
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check

# End-to-end smoke test against the release binary: generates silent WAV
# fixtures, then checks decoding, the first-run wizard, the settings window and
# the card's Z order.
powershell -File tools\verify.ps1
```

`tools/verify.ps1` needs a full-language PowerShell (the `System.IO` stream types
it uses are unavailable in constrained-language mode).

## How it works

A single Win32 process, one UI thread, and three background threads.

```
main.rs          arguments, single-instance mutex, message loop
  app.rs         window procedure, layout, Z order, tray, actions
    menu.rs      the shared right-click menu (TPM_RETURNCMD, so no WM_COMMAND)
    settings.rs  the settings window (standard Win32 controls)
    dialogs.rs   folder picker, Explorer, message boxes
  render.rs      Direct2D + DirectWrite onto a WIC bitmap, blitted to a
                 layered window with UpdateLayeredWindow
  theme.rs       colours and fonts, parsed from config
  library.rs     background folder walk + preview counter
  audio.rs       rodio player on its own thread
  config.rs      schema, validation, v1 migration, atomic saves
  autostart.rs   HKCU Run entry
  hotkeys.rs     global hotkeys
  i18n.rs        the string catalog
  log.rs         rotating log file and a panic hook
```

Points worth knowing if you want to change something:

- **Z order.** The card is `WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`
  and `WM_MOUSEACTIVATE` returns `MA_NOACTIVATE`, so it never takes focus. See
  `app::desktop_anchor_kind` for the Wallpaper-Engine-or-desktop-band choice.
- **Alpha.** `UpdateLayeredWindow` needs premultiplied BGRA, so the Direct2D
  target is created with `D2D1_ALPHA_MODE_PREMULTIPLIED` and the DIB is never
  touched by GDI afterwards. `SourceConstantAlpha` must stay 255 — the card's
  opacity is baked into the brush instead.
- **Playback starts fast.** Scanning a large library takes seconds, so the walk
  sends a *starter* album first (one randomly chosen top-level folder, which is
  milliseconds of work) and the rest of the library afterwards.
- **Track hand-off.** Auto-advance is detected by comparing `Player::len()`
  against the accounting invariant `current + appended`, not by watching for the
  length to drop — the refill runs first and would mask that transition.
- **Tray callbacks** deliberately stay on the legacy `NOTIFYICON_VERSION`, where
  the callback reports `WM_LBUTTONUP` / `WM_RBUTTONUP`. Under version 4 it
  reports `NIN_SELECT` / `WM_CONTEXTMENU` instead, and the classic double-click
  is lost.

Contributions are welcome — see [CONTRIBUTING.md](CONTRIBUTING.md).

## Known limitations

- Windows 10/11 x64 only. The rendering and windowing code is Win32 and Direct2D
  throughout; other platforms would need a different front end.
- Opus and WMA cannot be played: the audio backend has no decoder for them.
- The binaries are unsigned, so SmartScreen will warn on first run.
- Lyrics, streaming services, playlists and an EQ are explicitly out of scope —
  this is a lightweight folder player, not a library manager.

## License

[MIT](LICENSE). Uses [rodio](https://github.com/RustAudio/rodio) for playback,
[lofty](https://github.com/Serial-ATA/lofty-rs) for tags, and the
[windows](https://github.com/microsoft/windows-rs) crate for the Win32 APIs.
