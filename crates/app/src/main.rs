use facet_core::{Availability, Field, StorageDevice};
use gpui::{
    App, ClipboardItem, Context, FontWeight, IntoElement, MouseButton, ParentElement,
    Render, SharedString, Styled, Window, WindowBackgroundAppearance, WindowOptions, div, prelude::*, px, rgb,
};

const IVORY: u32 = 0xF4F1EA;
const GRAPHITE: u32 = 0x252726;
const VERMILION: u32 = 0xD84B33;
const MUTED: u32 = 0x6B6F6C;

struct Facet {
    devices: Vec<StorageDevice>,
    copied: Option<SharedString>,
}

impl Facet {
    fn spec(&self, label: &str, text: String, cx: &mut Context<Self>) -> impl IntoElement {
        let copy = text.clone();
        let label = label.to_owned();
        div()
            .id(SharedString::from(label.clone()))
            .flex()
            .gap_4()
            .py_2()
            .border_b_1()
            .border_color(rgb(0xDDD8CC))
            .cursor_pointer()
            .hover(|s| s.text_color(rgb(VERMILION)))
            .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
                this.copied = Some(format!("Copied {copy}").into());
                cx.notify();
            }))
            .child(div().w(px(120.)).text_color(rgb(MUTED)).child(label))
            .child(div().child(text))
    }
}

fn show<T>(f: &Field<T>, fmt: impl Fn(&T) -> String) -> String {
    match &f.availability {
        Availability::Available { value } => fmt(value),
        Availability::NotReported => "Not reported".into(),
        Availability::PermissionRequired => "Permission required".into(),
        Availability::Disconnected => "Disconnected".into(),
        Availability::Redacted => "Hidden".into(),
        Availability::Failed { reason } => format!("Unavailable ({reason})"),
    }
}

impl Render for Facet {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut page = div()
            .size_full()
            .bg(rgb(IVORY))
            .text_color(rgb(GRAPHITE))
            .p_8()
            .flex()
            .flex_col()
            .child(div().text_3xl().font_weight(FontWeight::SEMIBOLD).child("A closer look."))
            .child(div().text_color(rgb(MUTED)).mb_6().child("Every component. One clear picture."));

        let devices = self.devices.clone();
        for d in &devices {
            let title = show(&d.model, |v| v.clone());
            page = page
                .child(div().text_xl().mt_4().child(format!("{title}  ·  {}", d.id)))
                .child(self.spec("Firmware", show(&d.firmware, |v| v.clone()), cx))
                .child(self.spec("Capacity", show(&d.capacity_bytes, |b| facet_core::format_bytes(*b)), cx))
                .child(self.spec("Transport", show(&d.transport, |t| format!("{t:?}")), cx))
                .child(self.spec("Serial", show(&d.serial, |v| v.clone()), cx));
        }
        if devices.is_empty() {
            page = page.child("No storage devices found.");
        }
        if let Some(msg) = &self.copied {
            page = page.child(div().mt_4().text_color(rgb(VERMILION)).child(msg.clone()));
        }
        page
    }
}

fn main() {
    env_logger::init();
    gpui_platform::application().run(|cx: &mut App| {
        let mut report = facet_core::Report::new(facet_collectors::storage::collect());
        report.redact_identifiers();
        let options = WindowOptions {
            window_background: WindowBackgroundAppearance::Opaque,
            app_id: Some("dev.facet.Facet".into()),
            ..Default::default()
        };
        cx.open_window(options, |_, cx| {
            cx.new(|_| Facet { devices: report.storage, copied: None })
        })
        .expect("open window");
        cx.activate(true);
    });
}
