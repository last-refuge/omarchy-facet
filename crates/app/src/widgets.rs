//! Small shared building blocks.

use facet_core::{Availability, Category, Component, Spec};
use gpui::{Div, Hsla, IntoElement, ParentElement, Styled, Svg, div, px, svg};

use crate::theme::{MONO_FONT, Theme};

pub fn icon(name: &str, size: f32, color: Hsla) -> Svg {
    svg().path(format!("icons/{name}.svg")).size(px(size)).flex_none().text_color(color)
}

pub fn category_icon(c: Category) -> &'static str {
    match c {
        Category::System => "laptop",
        Category::Processor => "cpu",
        Category::Memory => "memory-stick",
        Category::Graphics => "circuit-board",
        Category::Storage => "hard-drive",
        Category::Network => "network",
        Category::Displays => "monitor",
        Category::Peripherals => "usb",
        Category::Power => "activity",
    }
}

pub fn component_icon(c: &Component) -> &'static str {
    let id = c.id.as_str();
    if id.starts_with("network:") {
        return if c.subtitle.as_deref() == Some("Wi-Fi") { "wifi" } else { "network" };
    }
    if id.starts_with("audio:") {
        return "volume-2";
    }
    if id.starts_with("camera:") {
        return "camera";
    }
    if id.starts_with("input:") {
        return "keyboard";
    }
    if id.starts_with("power:AC") {
        return "plug";
    }
    if id.starts_with("power:BAT") {
        return "battery-full";
    }
    if id.starts_with("sensors:fan") {
        return "fan";
    }
    if id.starts_with("sensors:") {
        return if c.title.to_lowercase().contains("temperature") { "thermometer" } else { "gauge" };
    }
    if id.starts_with("bluetooth:") {
        return "bluetooth";
    }
    if id.starts_with("pci:") {
        return "circuit-board";
    }
    if id.starts_with("usb:") || id.starts_with("input:") {
        let kind = c.subtitle.as_deref().unwrap_or("").to_lowercase();
        for (needle, icon) in [("keyboard", "keyboard"), ("mouse", "mouse"), ("pointing", "mouse"), ("touchpad", "mouse"), ("camera", "camera"), ("hub", "git-fork"), ("audio", "volume-2")] {
            if kind.contains(needle) {
                return icon;
            }
        }
        return "usb";
    }
    if id.starts_with("sensors:") && c.title.to_lowercase().contains("temperature") {
        return "thermometer";
    }
    category_icon(c.category)
}

/// Small caps label used above sections.
pub fn eyebrow(text: impl Into<gpui::SharedString>, t: &Theme) -> Div {
    div().text_size(px(11.5)).font_weight(gpui::FontWeight::SEMIBOLD).text_color(t.colors.faint).child(text.into().to_uppercase())
}

/// Monospace chip for ids and raw measurements.
pub fn mono(text: impl Into<gpui::SharedString>) -> Div {
    div().font_family(MONO_FONT).text_size(px(12.5)).child(text.into())
}

/// Whether a spec is a real value or an explained absence.
pub fn is_absent(s: &Spec) -> bool {
    !matches!(s.value.availability, Availability::Available { .. })
}

/// Why a value is absent, in plain language.
pub fn absence_reason(s: &Spec) -> Option<String> {
    match &s.value.availability {
        Availability::Available { .. } => None,
        Availability::NotReported => Some(format!("This device or driver does not report it. Source: {}", s.value.source)),
        Availability::PermissionRequired => Some("Reading this needs administrator access, which Facet does not use for browsing.".into()),
        Availability::Disconnected => Some("The device is no longer connected.".into()),
        Availability::Redacted => Some("Hidden by default. Include identifiers when exporting to show it.".into()),
        Availability::Failed { reason } => Some(format!("Reading failed: {reason}")),
    }
}

pub fn pluralize(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

#[allow(dead_code)]
pub fn into_el(e: impl IntoElement) -> gpui::AnyElement {
    e.into_any_element()
}
