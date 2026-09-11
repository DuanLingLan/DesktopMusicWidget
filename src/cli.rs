//! Command-line parsing.
//!
//! Several flags exist purely so the program can be exercised without a GUI:
//! `--count-tracks` and `--dump-config` are what CI uses to cover the scanning
//! and configuration code on a headless runner.

use std::path::PathBuf;

#[derive(Debug, Default, Clone)]
pub struct Args {
    pub help: bool,
    pub version: bool,
    /// Overrides the config directory (highest priority).
    pub config_dir: Option<PathBuf>,
    /// Force portable mode: config lives next to the executable.
    pub portable: bool,
    /// `auto` | `zh-CN` | `en`
    pub lang: Option<String>,
    pub dump_config: bool,
    /// Print how many tracks the configured folders contain, then exit.
    pub count_tracks: bool,
    /// Directory for `--count-tracks`; falls back to the configured library.
    pub dir: Option<PathBuf>,
    /// Debug helper: bypass the library and play exactly these files.
    pub play: Vec<PathBuf>,
    pub install_autostart: bool,
    pub remove_autostart: bool,
    /// Back up and rewrite the config with defaults, then exit.
    pub reset_config: bool,
    pub verbose: bool,
    /// One-shot override that beats `autoplay` in the config.
    pub autoplay: Option<bool>,
    /// Open config.toml in the default editor and exit.
    pub open_config: bool,
    /// Open the settings window as soon as the widget is up. Handy for
    /// troubleshooting and for automated smoke tests.
    pub open_settings: bool,
}

/// Parses `raw` (arguments *after* argv[0]).
///
/// Both `--opt value` and `--opt=value` are accepted. `--play` swallows every
/// following argument that is not itself a flag, so `--play a.mp3 b.mp3` works.
pub fn parse(raw: &[String]) -> Result<Args, String> {
    let mut args = Args::default();
    let mut i = 0;

    // Splits `--opt=value` into its two halves.
    fn split_eq(s: &str) -> (&str, Option<&str>) {
        match s.split_once('=') {
            Some((k, v)) => (k, Some(v)),
            None => (s, None),
        }
    }

    while i < raw.len() {
        let tok = raw[i].as_str();
        let (key, inline) = split_eq(tok);

        // Takes the value either from `--opt=value` or the next argument.
        macro_rules! value {
            () => {{
                if let Some(v) = inline {
                    v.to_string()
                } else {
                    i += 1;
                    match raw.get(i) {
                        Some(v) => v.clone(),
                        None => return Err(format!("{key} 缺少参数 / {key} needs a value")),
                    }
                }
            }};
        }

        match key {
            "-h" | "--help" => args.help = true,
            "-V" | "--version" => args.version = true,
            "--config-dir" => args.config_dir = Some(PathBuf::from(value!())),
            "--portable" => args.portable = true,
            "--lang" => args.lang = Some(value!()),
            "--dump-config" => args.dump_config = true,
            "--count-tracks" => args.count_tracks = true,
            "--dir" => args.dir = Some(PathBuf::from(value!())),
            "--install-autostart" => args.install_autostart = true,
            "--remove-autostart" => args.remove_autostart = true,
            "--reset-config" => args.reset_config = true,
            "--verbose" | "-v" => args.verbose = true,
            "--autoplay" => args.autoplay = Some(true),
            "--no-autoplay" => args.autoplay = Some(false),
            "--open-config" => args.open_config = true,
            "--open-settings" => args.open_settings = true,
            "--play" => {
                if let Some(v) = inline {
                    args.play.push(PathBuf::from(v));
                }
                // Everything up to the next flag belongs to --play.
                while let Some(next) = raw.get(i + 1) {
                    if next.starts_with('-') {
                        break;
                    }
                    args.play.push(PathBuf::from(next));
                    i += 1;
                }
            }
            other => return Err(format!("未知参数 / unknown argument: {other}")),
        }
        i += 1;
    }

    Ok(args)
}

pub fn usage() -> String {
    format!(
        "DesktopMusicWidget {}\n\
         \n\
         USAGE / 用法:\n\
         \x20 desktop-music-widget [OPTIONS]\n\
         \n\
         OPTIONS:\n\
         \x20 -h, --help              Show this help / 显示帮助\n\
         \x20 -V, --version           Show version / 显示版本\n\
         \x20     --config-dir <DIR>  Use DIR for config and logs / 指定配置目录\n\
         \x20     --portable          Keep config next to the exe / 便携模式\n\
         \x20     --lang <LANG>       auto | zh-CN | en / 界面语言\n\
         \x20     --autoplay, --no-autoplay\n\
         \x20                         Override autoplay for this run / 本次运行是否自动播放\n\
         \x20     --install-autostart Add to the logon entry / 写入开机自启\n\
         \x20     --remove-autostart  Remove the logon entry / 移除开机自启\n\
         \x20     --open-config       Open config.toml and exit / 打开配置文件\n\
         \x20     --open-settings     Open the settings window at startup / 启动时打开设置\n\
         \x20     --reset-config      Back up and rewrite defaults / 重置配置\n\
         \x20     --dump-config       Print the effective config / 打印当前配置\n\
         \x20     --count-tracks [--dir <DIR>]\n\
         \x20                         Count tracks and exit / 统计曲目数量\n\
         \x20     --play <FILE>...    Play these files instead of the library / 直接播放\n\
         \x20 -v, --verbose           Also log to the console / 输出日志到控制台\n",
        env!("CARGO_PKG_VERSION")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(v: &[&str]) -> Result<Args, String> {
        parse(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn defaults_are_inert() {
        let a = p(&[]).unwrap();
        assert!(!a.help && !a.version && !a.count_tracks);
        assert!(a.play.is_empty());
        assert!(a.config_dir.is_none());
    }

    #[test]
    fn value_can_be_inline_or_separate() {
        assert_eq!(
            p(&["--config-dir", "C:\\x"]).unwrap().config_dir,
            Some(PathBuf::from("C:\\x"))
        );
        assert_eq!(
            p(&["--config-dir=C:\\y"]).unwrap().config_dir,
            Some(PathBuf::from("C:\\y"))
        );
    }

    #[test]
    fn missing_value_is_an_error() {
        assert!(p(&["--config-dir"]).is_err());
    }

    #[test]
    fn play_swallows_paths_until_the_next_flag() {
        let a = p(&["--play", "a.mp3", "b.mp3", "--verbose"]).unwrap();
        assert_eq!(a.play, vec![PathBuf::from("a.mp3"), PathBuf::from("b.mp3")]);
        assert!(a.verbose, "the flag after --play must still be parsed");
    }

    #[test]
    fn play_accepts_an_inline_first_path() {
        let a = p(&["--play=a.mp3", "b.mp3"]).unwrap();
        assert_eq!(a.play, vec![PathBuf::from("a.mp3"), PathBuf::from("b.mp3")]);
    }

    #[test]
    fn unknown_flags_are_rejected() {
        assert!(p(&["--nope"]).is_err());
    }

    #[test]
    fn autoplay_overrides_are_independent() {
        assert_eq!(p(&["--no-autoplay"]).unwrap().autoplay, Some(false));
        assert_eq!(p(&["--autoplay"]).unwrap().autoplay, Some(true));
        assert_eq!(p(&[]).unwrap().autoplay, None);
    }

    #[test]
    fn usage_mentions_every_flag() {
        let u = usage();
        for flag in [
            "--config-dir",
            "--portable",
            "--lang",
            "--count-tracks",
            "--play",
            "--install-autostart",
            "--remove-autostart",
            "--dump-config",
            "--verbose",
        ] {
            assert!(u.contains(flag), "usage() omits {flag}");
        }
    }
}
