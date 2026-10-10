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
            faint: c(0x6F746F),
            hairline: c(0xDDD8CC),
            hairline_strong: c(0xC6C0B0),
            hover: c(0xEBE7DD),
            accent: c(0xD84B33),
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
            on_accent: c(0x1A1E1C),
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
