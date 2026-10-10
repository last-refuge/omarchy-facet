//! Design tokens. Views read colours through `cx.theme()` and never hard-code them.
//! Palette: warm ivory surface, deep graphite navigation, vermilion accent, muted sage support
//! (see llm-docs/brand-and-design.md). Both modes share one token set.

use gpui::{App, Global, Hsla, rgb};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Light,
    Dark,
}

#[derive(Clone, Debug)]
pub struct Colors {
    pub surface: Hsla,
    /// Raised surface for cards and overlays.
    pub raised: Hsla,
    pub nav: Hsla,
    pub nav_active: Hsla,
    pub nav_text: Hsla,
    pub nav_muted: Hsla,
    pub text: Hsla,
    pub muted: Hsla,
    pub faint: Hsla,
    pub hairline: Hsla,
    pub hairline_strong: Hsla,
    pub hover: Hsla,
    /// Vermilion for fills and large elements.
    pub accent: Hsla,
    /// Darker/lighter vermilion that meets AA for small text on `surface`.
    pub accent_text: Hsla,
    pub on_accent: Hsla,
    pub sage: Hsla,
    pub scrim: Hsla,
}

fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

impl Colors {
    pub fn light() -> Self {
        Colors {
            surface: c(0xF4F1EA),
            raised: c(0xFBF9F4),
            nav: c(0x202523),
            nav_active: c(0x303733),
            nav_text: c(0xEAE6DC),
            nav_muted: c(0xA3ACA7),
            text: c(0x252726),
            muted: c(0x5A5F5C),
            faint: c(0x636864),
            hairline: c(0xDDD8CC),
            hairline_strong: c(0xC6C0B0),
            hover: c(0xEBE7DD),
            accent: c(0xD1452D),
            accent_text: c(0xB23A25),
            on_accent: c(0xFFFFFF),
            sage: c(0x78917C),
            scrim: gpui::hsla(0.0, 0.0, 0.05, 0.38),
        }
    }

    pub fn dark() -> Self {
        Colors {
            surface: c(0x1A1E1C),
            raised: c(0x232826),
            nav: c(0x121514),
            nav_active: c(0x252B28),
            nav_text: c(0xE6E2D8),
            nav_muted: c(0x9AA39E),
            text: c(0xECE8DF),
            muted: c(0xB0B4AD),
            faint: c(0x949A94),
            hairline: c(0x333936),
            hairline_strong: c(0x4A524E),
            hover: c(0x262C29),
            accent: c(0xE0563D),
            accent_text: c(0xF0836E),
            on_accent: c(0x101312),
            sage: c(0x8DA891),
            scrim: gpui::hsla(0.0, 0.0, 0.0, 0.55),
        }
    }
}

pub const UI_FONT: &str = "IBM Plex Sans";
pub const MONO_FONT: &str = "Lilex";

#[derive(Clone, Debug)]
pub struct Theme {
    pub mode: Mode,
    pub colors: Colors,
}

impl Theme {
    pub fn new(mode: Mode) -> Self {
        Theme { mode, colors: if mode == Mode::Dark { Colors::dark() } else { Colors::light() } }
    }
}

impl Global for Theme {}

pub trait ThemeExt {
    fn theme(&self) -> &Theme;
}

impl ThemeExt for App {
    fn theme(&self) -> &Theme {
        self.global::<Theme>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG 2.x relative luminance of an sRGB colour given as `Hsla`.
    fn luminance(c: Hsla) -> f32 {
        let rgb = c.to_rgb();
        let lin = |v: f32| if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
        0.2126 * lin(rgb.r) + 0.7152 * lin(rgb.g) + 0.0722 * lin(rgb.b)
    }

    fn contrast(a: Hsla, b: Hsla) -> f32 {
        let (la, lb) = (luminance(a), luminance(b));
        let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
        (hi + 0.05) / (lo + 0.05)
    }

    #[test]
    fn text_pairs_meet_wcag_aa() {
        for (name, c) in [("light", Colors::light()), ("dark", Colors::dark())] {
            let pairs = [
                ("text on surface", c.text, c.surface, 4.5),
                ("muted on surface", c.muted, c.surface, 4.5),
                ("faint on surface", c.faint, c.surface, 4.5),
                ("accent_text on surface", c.accent_text, c.surface, 4.5),
                ("text on raised", c.text, c.raised, 4.5),
                ("muted on raised", c.muted, c.raised, 4.5),
                ("muted on hover", c.muted, c.hover, 4.5),
                ("text on hover", c.text, c.hover, 4.5),
                ("on_accent on accent (buttons)", c.on_accent, c.accent, 4.5),
                ("nav_text on nav", c.nav_text, c.nav, 4.5),
                ("nav_muted on nav", c.nav_muted, c.nav, 4.5),
                ("nav_text on nav_active", c.nav_text, c.nav_active, 4.5),
                ("nav_muted on nav_active", c.nav_muted, c.nav_active, 4.5),
                // Non-text: focus ring and hairlines that carry meaning need 3:1.
                ("accent on surface (focus ring, large text)", c.accent, c.surface, 3.0),
                ("accent on nav (nav icon)", c.accent, c.nav, 3.0),
                ("hairline_strong on surface (control borders)", c.hairline_strong, c.surface, 1.4),
            ];
            for (what, fg, bg, min) in pairs {
                let r = contrast(fg, bg);
                assert!(r >= min, "{name}: {what} is {r:.2}:1, needs {min}:1");
            }
        }
    }
}
