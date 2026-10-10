//! Compile-time bundled icons and fonts (provenance and licences: assets/icons/LICENSE, assets/fonts/*).

use std::borrow::Cow;

use anyhow::Result;
use gpui::{AssetSource, SharedString};

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        const ICONS: &[(&str, &[u8])] = &[
            $((concat!("icons/", $name, ".svg"),
               include_bytes!(concat!("../../../assets/icons/", $name, ".svg")) as &[u8])),*
        ];
    };
}

icons!(
    "activity", "arrow-right", "battery-full", "bluetooth", "cable", "camera", "check", "chevron-down", "chevron-left", "chevron-right", "circle-help",
    "circuit-board", "clock", "copy", "cpu", "download", "fan", "file-text", "gauge", "git-fork", "hard-drive",
    "info", "keyboard", "laptop", "layout-dashboard", "lock", "memory-stick", "microchip", "monitor", "mouse",
    "network", "panel-left", "plug", "refresh-cw", "search", "sliders-horizontal", "thermometer", "triangle-alert", "usb",
    "volume-2", "wifi", "x",
);

const BRAND: &[(&str, &[u8])] = &[
    ("brand/mark.svg", include_bytes!("../../../assets/brand/mark.svg") as &[u8]),
    ("brand/machine.svg", include_bytes!("../../../assets/brand/machine.svg") as &[u8]),
];

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(ICONS.iter().chain(BRAND).find(|(p, _)| *p == path).map(|(_, b)| Cow::Borrowed(*b)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(ICONS.iter().chain(BRAND).filter(|(p, _)| p.starts_with(path)).map(|(p, _)| SharedString::from(*p)).collect())
    }
}

macro_rules! font {
    ($file:literal) => {
        Cow::Borrowed(include_bytes!(concat!("../../../assets/fonts/", $file)) as &[u8])
    };
}

pub fn load_fonts(cx: &gpui::App) -> Result<()> {
    cx.text_system().add_fonts(vec![
        font!("IBMPlexSans-Regular.ttf"),
        font!("IBMPlexSans-SemiBold.ttf"),
        font!("Lilex-Regular.ttf"),
        font!("Lilex-Bold.ttf"),
    ])
}
