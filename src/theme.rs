//! Card colours and fonts.
//!
//! These used to be literals inside the renderer. They are now a serialisable
//! theme whose defaults reproduce the original palette exactly, so the card
//! looks identical until a user chooses to change it.

use serde::{Deserialize, Serialize};

/// Colours as `#rrggbb` or `#rrggbbaa` strings (leading `#` optional).
/// Strings are used rather than numbers so a hand-edited config stays readable.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Theme {
    /// Card background behind everything else.
    pub background: String,
    /// Placeholder square shown while a track has no cover art.
    pub art_bg: String,
    /// Track title, and also the transport glyphs.
    pub title: String,
    /// Artist line.
    pub subtitle: String,
    /// Unplayed portion of the progress bar.
    pub bar_bg: String,
    /// Played portion of the progress bar.
    pub bar_fg: String,
}

impl Default for Theme {
    fn default() -> Self {
        // These hex values are the round-trip of the original hardcoded floats.
        Self {
            background: "#121217".into(), // (0.07, 0.07, 0.09, 1.0)
            art_bg: "#ffffff1a".into(),   // (1, 1, 1, 0.10)
            title: "#fffffff5".into(),    // (1, 1, 1, 0.96)
            subtitle: "#ffffff99".into(), // (1, 1, 1, 0.60)
            bar_bg: "#ffffff29".into(),   // (1, 1, 1, 0.16)
            bar_fg: "#ffffffd9".into(),   // (1, 1, 1, 0.85)
        }
    }
}

/// A colour in Direct2D's 0.0-1.0 float space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }
}

fn hex_nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Parses `#rgb`, `#rrggbb` or `#rrggbbaa`. Returns `None` for anything else so
/// callers can fall back to the default instead of showing a wrong colour.
pub fn parse_color(s: &str) -> Option<Rgba> {
    let s = s.trim().trim_start_matches('#');
    let bytes = s.as_bytes();
    let nib = |i: usize| hex_nibble(bytes[i]);

    let (r, g, b, a) = match bytes.len() {
        3 => (nib(0)? * 17, nib(1)? * 17, nib(2)? * 17, 255),
        6 => (
            nib(0)? * 16 + nib(1)?,
            nib(2)? * 16 + nib(3)?,
            nib(4)? * 16 + nib(5)?,
            255,
        ),
        8 => (
            nib(0)? * 16 + nib(1)?,
            nib(2)? * 16 + nib(3)?,
            nib(4)? * 16 + nib(5)?,
            nib(6)? * 16 + nib(7)?,
        ),
        _ => return None,
    };

    Some(Rgba::new(
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        a as f32 / 255.0,
    ))
}

/// A theme with every colour already parsed, ready for the renderer. Invalid
/// entries fall back to the default palette, so a typo degrades to a working
/// card rather than an invisible one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub background: Rgba,
    pub art_bg: Rgba,
    pub title: Rgba,
    pub subtitle: Rgba,
    pub bar_bg: Rgba,
    pub bar_fg: Rgba,
}

impl Palette {
    pub fn resolve(theme: &Theme) -> Self {
        let d = Theme::default();
        let pick = |v: &str, fallback: &str| {
            parse_color(v)
                .or_else(|| parse_color(fallback))
                .unwrap_or(Rgba::new(1.0, 1.0, 1.0, 1.0))
        };
        Self {
            background: pick(&theme.background, &d.background),
            art_bg: pick(&theme.art_bg, &d.art_bg),
            title: pick(&theme.title, &d.title),
            subtitle: pick(&theme.subtitle, &d.subtitle),
            bar_bg: pick(&theme.bar_bg, &d.bar_bg),
            bar_fg: pick(&theme.bar_fg, &d.bar_fg),
        }
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::resolve(&Theme::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_six_digit_hex() {
        let c = parse_color("#ff8000").unwrap();
        assert!((c.r - 1.0).abs() < 0.001);
        assert!((c.g - 128.0 / 255.0).abs() < 0.001);
        assert!((c.b - 0.0).abs() < 0.001);
        assert!((c.a - 1.0).abs() < 0.001, "alpha defaults to opaque");
    }

    #[test]
    fn parses_eight_digit_hex_with_alpha() {
        let c = parse_color("#00000000").unwrap();
        assert_eq!(c.a, 0.0);
        let c = parse_color("ffffff80").unwrap(); // '#' is optional
        assert!((c.a - 128.0 / 255.0).abs() < 0.001);
    }

    #[test]
    fn parses_short_form_by_doubling() {
        let c = parse_color("#f0a").unwrap();
        assert!((c.r - 1.0).abs() < 0.001);
        assert!((c.g - 0.0).abs() < 0.001);
        assert!((c.b - 170.0 / 255.0).abs() < 0.001);
    }

    #[test]
    fn rejects_nonsense() {
        for bad in ["", "#", "#ff", "#ffff", "#gggggg", "red", "#12345"] {
            assert!(parse_color(bad).is_none(), "{bad} should not parse");
        }
    }

    #[test]
    fn default_theme_round_trips_through_hex() {
        // Guards the promise that the shipped default looks like the original.
        let p = Palette::default();
        assert!((p.background.r - 18.0 / 255.0).abs() < 0.001);
        assert!((p.background.b - 23.0 / 255.0).abs() < 0.001);
        assert!((p.subtitle.a - 153.0 / 255.0).abs() < 0.001);
    }

    #[test]
    fn broken_colour_falls_back_to_default() {
        let theme = Theme {
            background: "not-a-color".into(),
            ..Theme::default()
        };
        let p = Palette::resolve(&theme);
        assert_eq!(p.background, Palette::default().background);
    }
}
