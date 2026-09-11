# Contributing to DesktopMusicWidget

Thanks for taking the time. This is a small, deliberately scoped Win32/Rust project,
and the notes below are everything a first patch needs.

## Prerequisites

- Windows 10/11 x64 — the only supported target. The windowing and rendering code
  is Win32 and Direct2D throughout.
- [Rust](https://rustup.rs) 1.80 or newer (see `rust-version` in `Cargo.toml`).
- A Windows linker for the toolchain you pick:
  - **GNU** (`x86_64-pc-windows-gnu`): `rustup target add x86_64-pc-windows-gnu`,
    plus MinGW's `gcc` and `windres` on `PATH`. This is what the maintainer develops
    against, and what `tools/verify.ps1` is normally pointed at.
  - **MSVC** (`x86_64-pc-windows-msvc`): Visual Studio Build Tools with the C++
    workload.

## Build, test and lint

```powershell
cargo build --release
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

CI (`.github/workflows/ci.yml`) runs exactly these four, in this order, on
`windows-latest`. **Clippy runs with `-D warnings`, so a single warning fails the
build** — run it before you push. `build.rs` treats the resource compiler as
optional precisely so that a missing Windows SDK cannot turn a lint run red.

`cargo build --release` produces `target/release/desktop-music-widget.exe` and
`target/release/diag.exe`.

## The end-to-end harness

```powershell
powershell -File tools\verify.ps1
```

Run it from a **full-language PowerShell**. The script builds real WAV data with
`System.IO.BinaryWriter` and `MemoryStream`, which constrained-language mode does
not expose.

It generates silent WAV fixtures under `%TEMP%\dmw-verify` and then checks:

1. **Decode and now-playing** — starts the release binary with `--play` and asserts
   that a track was published, that the fallback title came from the file stem, and
   that nothing failed to decode.
2. **First-run wizard** — starts it with an empty `APPDATA` and a fresh config
   directory, and asserts that the modal folder picker really opened.
3. **Settings window** — starts it with `--open-settings`, runs `diag.exe` against
   it, and asserts the card's Z order, that every control was created, and that the
   window shows the configured folder, config path and numeric/slider values.

It exits non-zero when any check fails.

Before launching the app by hand, know this: **the single-instance mutex is global by
design**, so a leftover copy of this build makes every later launch exit immediately
with nothing on screen and nothing obvious in the log. `tools/verify.ps1` handles
that for its own binary (`Clear-OurInstances`), and it deliberately only touches
processes whose image path is exactly the binary under test — the pre-rename build
shares the process name and must not be disturbed. If you started a copy manually,
kill it from Task Manager first.

## Architecture

```
src/lib.rs        the crate: module list, window class names, mutex name
src/main.rs       arguments, single-instance mutex, message loop
src/app.rs        window procedure, layout, Z order, tray, actions
src/menu.rs       the shared right-click menu (card + tray)
src/settings.rs   the settings window (standard Win32 controls)
src/dialogs.rs    folder picker, Explorer, message boxes
src/render.rs     Direct2D + DirectWrite onto a WIC bitmap, blitted with
                  UpdateLayeredWindow
src/art.rs        album-art decoding via WIC
src/theme.rs      colours and fonts, parsed from config
src/library.rs    background folder walk + preview counter
src/audio.rs      rodio player on its own thread
src/config.rs     schema, validation, v1 migration, atomic saves
src/autostart.rs  HKCU Run entry
src/hotkeys.rs    global hotkeys
src/i18n.rs       the string catalog (zh + en)
src/log.rs        rotating log file and a panic hook
src/bin/diag.rs   the diag.exe diagnostic tool
```

Four things worth knowing before you change something:

- **Z order** (`src/app.rs`, `app::desktop_anchor_kind`). The card is
  `WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE` and `WM_MOUSEACTIVATE`
  returns `MA_NOACTIVATE`, so it never takes focus. Whether it stacks above
  Wallpaper Engine or anchors to the Explorer desktop band (`Progman` / `WorkerW`)
  is decided there.
- **Layered-window alpha** (`src/render.rs`). `UpdateLayeredWindow` needs
  premultiplied BGRA, so the Direct2D target is created with
  `D2D1_ALPHA_MODE_PREMULTIPLIED` and the DIB is never touched by GDI afterwards.
  `SourceConstantAlpha` must stay 255 — the card's opacity is baked into the brush
  instead.
- **Starter-album fast start** (`src/library.rs`). Scanning a large library takes
  seconds, so the walk publishes a *starter* album first — one randomly chosen
  top-level folder, milliseconds of work — and the rest of the library afterwards.
- **Track hand-off** (`src/audio.rs`). Auto-advance is detected by comparing
  `Player::len()` against the accounting invariant `current + appended`, not by
  watching for the length to drop: the refill runs first and would mask that
  transition.

## Code style

- **Comments explain *why*, not *what*.** The existing code leans on this heavily:
  a non-obvious Win32 flag, a retry, or an ordering constraint should say what it
  is defending against.
- **Unit-test anything pure.** Config validation, the v1→v2 migration, scan scope
  (recursion, name- vs path-based exclusions), menu ids and radio-group
  contiguity, and i18n catalog parity all have tests and should keep them.
- **Config loading must never be able to fail.** A missing file is created, a
  corrupt file is backed up and replaced with defaults, and out-of-range values are
  clamped with a warning. Editing a text file should never make the widget
  unstartable.
- Keep the UI strings in `src/i18n.rs` — the catalog is one macro invocation, so a
  missing translation is a compile error and the two languages cannot drift.

## Reporting a bug

Open an issue with the bug report template filled in. Two attachments speed things
up enormously:

- `%APPDATA%\DesktopMusicWidget\widget.log` — the rotating log file, which includes
  the startup banner, config corrections and any decode or device failures.
- The output of **`diag.exe`** — it prints the Z order, which desktop anchor was
  chosen, whether the card is above or below the desktop band, and the monitor
  layout.

If the crash produced a message box, that dialog also names the log file: a release
build is `panic = "abort"`, so the panic hook is the only trace left behind.
