//! Embeds the application manifest, the icon and the version resource.
//!
//! The `.rc` is **generated** rather than committed so the version numbers can
//! never drift from `Cargo.toml`. Resource paths are written as absolute
//! forward-slash paths because the generated file lives in `OUT_DIR`.
//!
//! A missing or broken resource compiler is reported as a build warning instead
//! of an error: the widget runs perfectly well without an icon, and refusing to
//! build over a cosmetic resource would be a poor trade for anyone compiling
//! from source.

use std::path::{Path, PathBuf};
use std::process::Command;

/// `{...}` placeholders are substituted by hand (not with `format!`) so the
/// literal `\0` terminators the VERSIONINFO block needs survive untouched.
const RC_TEMPLATE: &str = r#"1 ICON "{ICON}"
1 24 "{MANIFEST}"

1 VERSIONINFO
FILEVERSION {VER}
PRODUCTVERSION {VER}
FILEOS 0x40004
FILETYPE 0x1
{
  BLOCK "StringFileInfo"
  {
    BLOCK "040904b0"
    {
      VALUE "FileDescription", "DesktopMusicWidget\0"
      VALUE "FileVersion", "{VERSION}\0"
      VALUE "InternalName", "desktop-music-widget\0"
      VALUE "LegalCopyright", "MIT License\0"
      VALUE "OriginalFilename", "desktop-music-widget.exe\0"
      VALUE "ProductName", "DesktopMusicWidget\0"
      VALUE "ProductVersion", "{VERSION}\0"
    }
  }
  BLOCK "VarFileInfo"
  {
    VALUE "Translation", 0x409, 1200
  }
}
"#;

/// `1.2.3` → `(1, 2, 3)`; anything unparseable degrades to zeros.
fn version_quad(version: &str) -> (u32, u32, u32) {
    let mut parts = version.split('.').map(|p| {
        p.split(['-', '+'])
            .next()
            .unwrap_or("0")
            .parse::<u32>()
            .unwrap_or(0)
    });
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    )
}

fn forward_slashes(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Locates an executable on `PATH`. `env_var` is checked first so a user can
/// point at a specific toolchain.
fn find_tool(env_var: &str, names: &[&str]) -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os(env_var) {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return Some(path);
        }
    }
    let paths = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&paths) {
        for name in names {
            for candidate in [dir.join(name), dir.join(format!("{name}.exe"))] {
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

/// GNU targets: compile the `.rc` straight to a COFF object with `windres` and
/// hand that object to the linker.
///
/// This deliberately does not go through `embed-resource`. On the CI runner that
/// crate chains `windres` into `ar` to build an archive, and the archive came out
/// as "file format not recognized" — the `ar` it picked up was not the
/// target-matching one. A plain COFF object needs only `windres`, needs no
/// archive step, and is accepted by `ld` directly.
fn compile_gnu(rc_path: &Path, out_dir: &Path) -> Result<PathBuf, String> {
    let windres = find_tool("WINDRES", &["x86_64-w64-mingw32-windres", "windres"])
        .ok_or("windres was not found on PATH (install MinGW-w64 binutils)")?;

    let object = out_dir.join("desktop-music-widget-res.o");
    // `-O coff` alone is deliberate: windres has no `--output-arch` option (it
    // rejects it outright), and the target-specific executable name looked up
    // above is what decides the object's architecture.
    let output = Command::new(&windres)
        .arg("-i")
        .arg(rc_path)
        .arg("-o")
        .arg(&object)
        .arg("-O")
        .arg("coff")
        .output()
        .map_err(|e| format!("cannot run {}: {e}", windres.display()))?;

    if !output.status.success() {
        return Err(format!(
            "windres failed ({}):\n{}{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    if !object.is_file() {
        return Err("windres reported success but produced no object file".into());
    }
    Ok(object)
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/app.manifest");
    println!("cargo:rerun-if-changed=assets/icon.ico");

    // The resources are Windows-only; other targets simply skip them so the
    // crate stays cross-compilable for checks.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let manifest_dir = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is always set by cargo"),
    );
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR is always set by cargo"));
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());

    let icon = manifest_dir.join("assets").join("icon.ico");
    let manifest = manifest_dir.join("assets").join("app.manifest");
    if !icon.exists() {
        println!(
            "cargo:warning=assets/icon.ico is missing; run assets/make_icon.ps1 to regenerate it"
        );
    }

    let (major, minor, patch) = version_quad(&version);
    let rc = RC_TEMPLATE
        .replace("{ICON}", &forward_slashes(&icon))
        .replace("{MANIFEST}", &forward_slashes(&manifest))
        .replace("{VER}", &format!("{major},{minor},{patch},0"))
        .replace("{VERSION}", &version);

    let rc_path = out_dir.join("desktop-music-widget.rc");
    if let Err(e) = std::fs::write(&rc_path, rc) {
        println!("cargo:warning=cannot write the generated .rc file: {e}");
        return;
    }

    let is_gnu = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu");
    let outcome = if is_gnu {
        compile_gnu(&rc_path, &out_dir).map(|object| {
            println!("cargo:rustc-link-arg={}", object.display());
        })
    } else {
        // MSVC: the crate locates the Windows SDK's rc.exe, compiles the .rc and
        // also arranges for the manifest to be the one the linker uses.
        // `manifest_optional` keeps a missing resource compiler from failing the
        // build outright.
        embed_resource::compile(&rc_path, embed_resource::NONE)
            .manifest_optional()
            .map_err(|e| e.to_string())
    };

    if let Err(e) = outcome {
        println!(
            "cargo:warning=could not embed the Windows resources ({e}); \
             the build will continue without an app icon or themed controls"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_parse_into_a_quad() {
        assert_eq!(version_quad("1.2.3"), (1, 2, 3));
        assert_eq!(version_quad("1.0.0"), (1, 0, 0));
        // Pre-release and build metadata must not confuse the parse.
        assert_eq!(version_quad("2.5.0-beta.1"), (2, 5, 0));
        assert_eq!(version_quad("2.5.0+build7"), (2, 5, 0));
    }

    #[test]
    fn unparseable_versions_degrade_to_zero() {
        assert_eq!(version_quad(""), (0, 0, 0));
        assert_eq!(version_quad("not-a-version"), (0, 0, 0));
        assert_eq!(version_quad("1"), (1, 0, 0));
    }

    #[test]
    fn rc_paths_use_forward_slashes() {
        // Backslashes are escape characters to some resource compilers.
        assert_eq!(
            forward_slashes(std::path::Path::new(r"D:\a\b\icon.ico")),
            "D:/a/b/icon.ico"
        );
    }

    #[test]
    fn template_placeholders_are_all_substituted() {
        let rendered = RC_TEMPLATE
            .replace("{ICON}", "i")
            .replace("{MANIFEST}", "m")
            .replace("{VER}", "1,0,0,0")
            .replace("{VERSION}", "1.0.0");
        assert!(!rendered.contains('{'), "a placeholder was left behind");
        assert!(rendered.contains("VALUE \"FileVersion\", \"1.0.0\\0\""));
        assert!(
            rendered.contains("1 24 \"m\""),
            "manifest id 24 is required"
        );
    }

    #[test]
    fn an_absent_tool_is_reported_as_missing() {
        // The lookup must not panic and must not invent a path.
        assert!(find_tool(
            "DESKTOP_MUSIC_WIDGET_NO_SUCH_TOOL",
            &["definitely-not-a-real-tool-9f3a2b"]
        )
        .is_none());
    }

    #[test]
    fn tool_lookup_finds_a_tool_that_is_definitely_present() {
        // `cmd.exe` exists on every Windows install, so this exercises the PATH
        // walk for real rather than the fallback.
        let found = find_tool("DESKTOP_MUSIC_WIDGET_NO_SUCH_TOOL", &["cmd"]);
        assert!(found.is_some(), "cmd.exe should be found on PATH");
    }
}
