//! Page bodies: overview, category lists, component detail, connections, preferences, and the
//! loading / error states.

use facet_core::{Category, Component, Inventory, Spec, Value};
use gpui::{
    AnyElement, Context, Div, FontWeight, InteractiveElement, IntoElement, ParentElement, Role,
    SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::*, px, transparent_black,
};

use crate::app::{Facet, Load, Page, ThemePref};
use crate::theme::{MONO_FONT, Mode, ThemeExt};
use crate::widgets::{Activate, absence_reason, category_icon, component_icon, eyebrow, icon, line_chart, mono, pluralize, sparkline, VERSION};

/// Short human phrase for a summary spec, or `None` when it adds nothing ("No").
/// Widest the page content grows. The overview's two-column maths uses the same number.
const CONTENT_MAX: f32 = 1180.0;

fn fact(s: &Spec, first: bool) -> Option<String> {
    let text = s.display();
    // Drop explanatory parentheses ("Integrated (on the root bus; heuristic)" -> "Integrated").
    let short = text.split(" (").next().unwrap_or(&text).to_owned();
    match s.value.value() {
        Some(Value::Bool(true)) => Some(s.label.to_lowercase()),
        Some(Value::Bool(false)) => None,
        Some(Value::Count(_) | Value::Percent(_)) => Some(format!("{short} {}", s.label.to_lowercase())),
        Some(Value::Hertz(_)) if s.rated && s.label.starts_with("Max") => Some(format!("up to {short}")),
        Some(Value::Bytes(_) | Value::BinaryBytes(_)) if !first => {
            let label = s.label.to_lowercase();
            let label = label.trim_end_matches(" memory").trim_end_matches(" total");
            Some(format!("{short} {label}"))
        }
        _ => Some(short),
    }
}

fn facts(c: &Component, n: usize) -> String {
    let mut out = Vec::new();
    for (i, s) in c.summary.iter().filter(|s| s.is_available() && !s.identifier).enumerate() {
        if let Some(f) = fact(s, i == 0) {
            out.push(f);
        }
        if out.len() == n {
            break;
        }
    }
    out.join("  ·  ")
}

impl Facet {
    pub fn page_content(&mut self, narrow: bool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let key = self.page.key();
        let scroll = self.scroll_for(&key);
        let body: AnyElement = match self.page.clone() {
            Page::Overview => self.overview(narrow, cx),
            Page::Category(c) => self.category_page(c, narrow, cx),
            Page::Component(id) => self.detail_page(&id, narrow, cx),
            Page::Connections => self.connections_page(narrow, cx),
            Page::Preferences => self.preferences_page(window, cx),
        };
        let pad = if narrow { 18. } else { 40. };
        div()
            .id(SharedString::from(format!("page-{key}")))
            .role(Role::Main)
            .aria_label(self.page_label())
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .child(div().w_full().max_w(px(CONTENT_MAX)).mx_auto().px(px(pad)).pt(px(if narrow { 20. } else { 34. })).pb(px(56.)).child(body))
            .into_any_element()
    }

    fn page_label(&self) -> String {
        match &self.page {
            Page::Overview => "Overview".into(),
            Page::Category(c) => c.label().into(),
            Page::Component(id) => self.inv.get(id).map(|c| c.title.clone()).unwrap_or_else(|| "Device".into()),
            Page::Connections => "Device connections".into(),
            Page::Preferences => "Preferences".into(),
        }
    }

    // ---- shared pieces --------------------------------------------------------------------

    fn heading(&self, title: impl Into<SharedString>, sub: impl Into<SharedString>, cx: &Context<Self>) -> Div {
        let t = cx.theme();
        let title: SharedString = title.into();
        div()
            .mb(px(26.))
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(div().id("page-title").role(Role::Heading).aria_label(title.clone()).text_size(px(34.)).line_height(px(40.)).font_weight(FontWeight::SEMIBOLD).child(title))
            .child(div().text_size(px(16.)).text_color(t.colors.muted).child(sub.into()))
    }

    fn section(&self, title: &str, cx: &Context<Self>) -> gpui::Stateful<Div> {
        let t = cx.theme();
        div()
            .id(SharedString::from(format!("section-{title}")))
            .role(Role::Heading)
            .aria_label(title.to_owned())
            .mt(px(34.))
            .mb(px(8.))
            .pb(px(8.))
            .border_b_1()
            .border_color(t.colors.hairline_strong)
            .child(eyebrow(title.to_owned(), t))
    }

    fn skeleton_rows(&self, n: usize, cx: &Context<Self>) -> gpui::Stateful<Div> {
        let t = cx.theme();
        let mut d = div().id("loading").flex().flex_col().role(Role::Status).aria_label("Reading hardware");
        for i in 0..n {
            d = d.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .py(px(16.))
                    .border_b_1()
                    .border_color(t.colors.hairline)
                    .child(div().size(px(20.)).rounded(px(5.)).bg(t.colors.hairline))
                    .child(div().w(px(110.)).h(px(10.)).rounded(px(4.)).bg(t.colors.hairline))
                    .child(div().w(px(220. + (i as f32 * 37.) % 120.)).h(px(14.)).rounded(px(4.)).bg(t.colors.hairline_strong)),
            );
        }
        d
    }

    fn error_panel(&self, msg: &str, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        div()
            .id("error-panel")
            .role(Role::Alert)
            .aria_label("Facet could not read this machine")
            .mt(px(40.))
            .p(px(28.))
            .rounded(px(12.))
            .border_1()
            .border_color(t.colors.hairline_strong)
            .bg(t.colors.raised)
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(div().flex().items_center().gap(px(10.)).child(icon("triangle-alert", 20., t.colors.accent_text)).child(div().text_size(px(20.)).font_weight(FontWeight::SEMIBOLD).child("Facet couldn't read this machine")))
            .child(div().text_color(t.colors.muted).child(msg.to_owned()))
            .child(div().text_color(t.colors.muted).child("Nothing was changed. Your hardware is untouched; only reading failed."))
            .child(self.button("retry", "Try again", true, cx, |this, window, cx| this.refresh(&crate::app::Refresh, window, cx)))
            .into_any_element()
    }

    pub fn button(
        &self,
        id: &'static str,
        label: &str,
        primary: bool,
        cx: &mut Context<Self>,
        f: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> impl IntoElement {
        self.button_focus(id, label, primary, None, cx, f)
    }

    /// A button that can also be a target of a dialog's own Tab cycle.
    pub fn button_focus(
        &self,
        id: &'static str,
        label: &str,
        primary: bool,
        focus: Option<gpui::FocusHandle>,
        cx: &mut Context<Self>,
        f: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> impl IntoElement {
        let t = cx.theme().clone();
        div()
            .id(id)
            .when_some(focus, |d, h| d.track_focus(&h))
            .role(Role::Button)
            .aria_label(label.to_owned())
            .tab_index(0)
            .self_start()
            .h(px(34.))
            .px(px(14.))
            .flex()
            .items_center()
            .rounded(px(8.))
            .border_1()
            .font_weight(FontWeight::SEMIBOLD)
            .text_size(px(13.))
            .cursor_pointer()
            .when(primary, |d| d.bg(t.colors.accent).border_color(t.colors.accent).text_color(t.colors.on_accent).hover(|s| s.opacity(0.92)))
            .when(!primary, |d| d.border_color(t.colors.hairline_strong).hover(|s| s.bg(t.colors.hover)))
            .focus_visible(|s| s.border_color(t.colors.text))
            .on_activate(cx, move |this, window, cx| f(this, window, cx))
            .child(label.to_owned())
    }

    fn notice(&self, cx: &Context<Self>) -> Option<AnyElement> {
        if self.failures.is_empty() {
            return None;
        }
        let t = cx.theme();
        Some(
            div()
                .id("collector-notice")
                .role(Role::Status)
                .mb(px(18.))
                .px(px(14.))
                .py(px(10.))
                .rounded(px(8.))
                .border_1()
                .border_color(t.colors.hairline_strong)
                .text_size(px(13.))
                .text_color(t.colors.muted)
                .child(format!("Some hardware could not be read. {}", self.failures.join(" ")))
                .into_any_element(),
        )
    }

    // ---- overview -------------------------------------------------------------------------

    fn machine_row(&self, label: &str, c: &Component, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let narrow = self.narrow;
        let id = c.id.clone();
        let is_new = self.new_ids.contains(&c.id);
        div()
            .id(SharedString::from(format!("machine-{}", c.id)))
            .role(Role::Link)
            .aria_label(format!("{label}: {}. {}", c.title, facts(c, 3)))
            .tab_index(0)
            .flex()
            .items_center()
            .gap(px(16.))
            .py(px(15.))
            .px(px(4.))
            .border_b_1()
            .border_color(t.colors.hairline)
            .cursor_pointer()
            .hover(|s| s.bg(t.colors.hover))
            .focus_visible(|s| s.border_color(t.colors.accent))
            .on_activate(cx, move |this, window, cx| this.go(Page::Component(id.clone()), window, cx))
            .relative()
            .when(is_new, |d| d.child(div().absolute().left_0().top(px(8.)).bottom(px(8.)).w(px(3.)).rounded(px(2.)).bg(t.colors.accent)))
            .child(icon(component_icon(c), 19., t.colors.muted))
            .when(!narrow, |d| {
                d.child(div().w(px(104.)).flex_none().child(eyebrow(label.to_owned(), &t)))
                    .child(div().flex_1().min_w_0().truncate().text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).child(c.title.clone()))
                    .child(div().flex_none().text_size(px(13.)).text_color(t.colors.muted).child(facts(c, 3)))
            })
            .when(narrow, |d| {
                d.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(eyebrow(label.to_owned(), &t))
                        .child(div().text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).child(c.title.clone()))
                        .child(div().text_size(px(12.5)).text_color(t.colors.muted).child(facts(c, 3))),
                )
            })
            .child(icon("chevron-right", 15., t.colors.faint))
            .into_any_element()
    }

    /// One row of the "This machine" table: small-caps label, the component, and its key facts beneath.
    fn table_row(&self, label: &str, c: &Component, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let id = c.id.clone();
        let is_new = self.new_ids.contains(&c.id);
        div()
            .id(SharedString::from(format!("machine-{}", c.id)))
            .role(Role::Link)
            .aria_label(format!("{label}: {}. {}", c.title, facts(c, 3)))
            .tab_index(0)
            .relative()
            .flex()
            .gap(px(22.))
            .py(px(10.))
            .px(px(2.))
            .border_b_1()
            .border_color(t.colors.hairline)
            .cursor_pointer()
            .hover(|s| s.bg(t.colors.hover))
            .focus_visible(|s| s.border_color(t.colors.accent).bg(t.colors.hover))
            .on_activate(cx, move |this, window, cx| this.go(Page::Component(id.clone()), window, cx))
            .when(is_new, |d| d.child(div().absolute().left_0().top(px(8.)).bottom(px(8.)).w(px(3.)).rounded(px(2.)).bg(t.colors.accent)))
            .child(div().w(px(104.)).flex_none().pt(px(4.)).child(eyebrow(label.to_owned(), &t)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .child(div().text_size(px(17.)).font_weight(FontWeight::SEMIBOLD).child(c.title.clone()))
                    .child(div().text_size(px(13.)).text_color(t.colors.muted).child(facts(c, 3).replace("  ·  ", "  /  "))),
            )
            .into_any_element()
    }

    /// Several drives collapse into one row ("3 drives, 7.0 TB") that opens the Storage page, so a
    /// machine with many disks doesn't push the live readings off the screen.
    fn storage_summary_row(&self, drives: &[&Component], cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let total: u64 = drives
            .iter()
            .filter_map(|c| c.all_specs().find(|s| s.label == "Capacity").and_then(|s| match s.value.value() {
                Some(Value::Bytes(b)) => Some(*b),
                _ => None,
            }))
            .sum();
        let names = drives.iter().map(|c| c.title.as_str()).collect::<Vec<_>>().join(", ");
        div()
            .id("machine-storage-all")
            .role(Role::Link)
            .aria_label(format!("Storage: {} drives, {}. {names}", drives.len(), facet_core::format_bytes(total)))
            .tab_index(0)
            .flex()
            .gap(px(22.))
            .py(px(10.))
            .px(px(2.))
            .border_b_1()
            .border_color(t.colors.hairline)
            .cursor_pointer()
            .hover(|s| s.bg(t.colors.hover))
            .focus_visible(|s| s.border_color(t.colors.accent).bg(t.colors.hover))
            .on_activate(cx, |this, window, cx| this.go(Page::Category(Category::Storage), window, cx))
            .child(div().w(px(104.)).flex_none().pt(px(4.)).child(eyebrow("Storage", &t)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .child(div().text_size(px(17.)).font_weight(FontWeight::SEMIBOLD).child(format!("{} drives, {} in total", drives.len(), facet_core::format_bytes(total))))
                    .child(div().truncate().text_size(px(13.)).text_color(t.colors.muted).child(names)),
            )
            .into_any_element()
    }

    /// The exploded-machine drawing with callouts for the parts this machine has. Callout labels are
    /// drawn only when there is room; the drawing alone is always a way into the connections view.
    fn illustration(&self, inv: &Inventory, w: f32, labels: bool, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let h = w * 430.0 / 640.0;
        let mut art = div().relative().w(px(w)).h(px(h)).child(gpui::svg().path("brand/machine.svg").w(px(w)).h(px(h)).text_color(t.colors.muted.opacity(0.85)));
        if labels {
            let dimms: Vec<&Component> = inv.components.iter().filter(|c| c.id.starts_with("memory:dimm")).collect();
            let memory_sub = dimms.first().and_then(|d| d.all_specs().find(|s| s.label == "Size")).map(|s| format!("{} × {}", dimms.len(), s.display()));
            let has_nvme = inv.in_category(Category::Storage).any(|c| c.all_specs().any(|s| s.label == "Interface" && s.display() == "NVMe"));
            // (label, sub-label, anchor x, anchor y, label on the left?)  Anchors come from scripts/gen-illustration.py.
            let callouts: [(&str, Option<String>, f32, f32, bool); 4] = [
                ("CPU", None, 0.479, 0.109, true),
                ("Memory", memory_sub, 0.752, 0.368, false),
                ("Graphics", None, 0.458, 0.834, false),
                (if has_nvme { "M.2 SSD" } else { "Storage" }, None, 0.257, 0.375, true),
            ];
            for (label, sub, ax, ay, left) in callouts {
                let (x, y) = (ax * w, ay * h);
                let (line_from, line_to) = if left { (0.15 * w, x) } else { (x, 0.80 * w) };
                art = art
                    .child(div().absolute().left(px(line_from.min(line_to))).top(px(y)).w(px((line_to - line_from).abs())).h(px(1.)).bg(t.colors.accent.opacity(0.55)))
                    .child(div().absolute().left(px(x - 3.5)).top(px(y - 3.5)).size(px(7.)).rounded_full().bg(t.colors.accent))
                    .child(
                        div()
                            .absolute()
                            .top(px(y - 9.))
                            .when(left, |d| d.left_0().w(px(0.14 * w)).items_end().text_right())
                            .when(!left, |d| d.left(px(0.81 * w)).w(px(0.19 * w)))
                            .flex()
                            .flex_col()
                            .text_size(px(12.))
                            .text_color(t.colors.muted)
                            .child(label.to_owned())
                            .when_some(sub, |d, s| d.child(div().text_size(px(11.)).text_color(t.colors.faint).child(s))),
                    );
            }
        }
        div()
            .id("explore-machine")
            .role(Role::Button)
            .aria_label("Explore how this machine is connected")
            .tab_index(0)
            .rounded(px(10.))
            .border_1()
            .border_color(transparent_black())
            .cursor_pointer()
            .focus_visible(|s| s.border_color(t.colors.accent))
            .on_activate(cx, |this, window, cx| this.go(Page::Connections, window, cx))
            .child(art)
            .into_any_element()
    }

    /// The full-width bar of live readings: CPU and GPU temperature with trend lines, memory with a bar.
    fn live_strip(&mut self, inv: &Inventory, narrow: bool, cx: &mut Context<Self>) -> Option<AnyElement> {
        let t = cx.theme().clone();
        let wanted = ["cpu_temperature", "memory_in_use", "gpu_temperature"];
        let readings: Vec<facet_core::Reading> = facet_core::headline_readings(inv).into_iter().filter(|r| wanted.contains(&r.id.as_str())).collect();
        if readings.is_empty() {
            return None;
        }
        let live = self.live;
        let status = if live {
            div().flex().items_center().gap(px(8.)).child(div().size(px(7.)).rounded_full().bg(t.colors.sage)).child(div().text_size(px(12.5)).text_color(t.colors.muted).child("Refreshing every 2 seconds"))
        } else {
            div().flex().items_center().gap(px(8.)).child(div().text_size(px(12.5)).text_color(t.colors.muted).child("Snapshot")).child(
                div()
                    .id("live-on")
                    .role(Role::Button)
                    .aria_label("Turn on live monitoring")
                    .tab_index(0)
                    .px(px(6.))
                    .rounded(px(5.))
                    .border_1()
                    .border_color(transparent_black())
                    .cursor_pointer()
                    .text_size(px(12.5))
                    .text_color(t.colors.accent_text)
                    .font_weight(FontWeight::SEMIBOLD)
                    .hover(|s| s.bg(t.colors.hover))
                    .focus_visible(|s| s.border_color(t.colors.accent))
                    .on_activate(cx, |this, _, cx| this.set_live_pref(true, cx))
                    .child("Turn on live monitoring"),
            )
        };

        let mut strip = div().flex().when(narrow, |d| d.flex_col()).border_t_1().border_b_1().border_color(t.colors.hairline_strong);
        let n = readings.len();
        for (i, r) in readings.into_iter().enumerate() {
            let history = r.source.as_ref().and_then(|(owner, label)| self.history_for(owner, label)).map(|h| h.iter().copied().collect::<Vec<f64>>());
            let is_memory = r.id == "memory_in_use";
            let (big, small) = match (is_memory, r.detail.split_once(" / ")) {
                (true, Some((used, total))) => (used.to_owned(), format!(" / {total}")),
                _ => (r.display.clone(), String::new()),
            };
            let chart_color = if r.id == "gpu_temperature" { t.colors.sage } else { t.colors.accent_text };
            let mut value_row = div()
                .flex()
                .items_end()
                .gap(px(14.))
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .child(div().text_size(px(38.)).line_height(px(44.)).font_weight(FontWeight::SEMIBOLD).text_color(if r.available { t.colors.text } else { t.colors.faint }).child(big))
                        .when(!small.is_empty(), |d| d.child(div().text_size(px(20.)).text_color(t.colors.muted).child(small))),
                );
            if is_memory {
                let frac = (r.value.unwrap_or(0.0) / 100.0).clamp(0.0, 1.0) as f32;
                value_row = value_row.child(
                    div().mb(px(10.)).w(px(110.)).h(px(8.)).rounded_full().bg(t.colors.hairline).child(div().h_full().w(px(110. * frac)).rounded_full().bg(t.colors.text)),
                );
            } else if let Some(h) = history {
                value_row = value_row.child(div().mb(px(6.)).child(line_chart(h, chart_color, 120., 34.)));
            }
            strip = strip.child(
                div()
                    .id(SharedString::from(format!("live-{}", r.id)))
                    .role(Role::GenericContainer)
                    .aria_label(format!("{}: {}. {}", r.label, r.display, r.note))
                    .flex_1()
                    .min_w_0()
                    .px(px(22.))
                    .py(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .when(i == 0 && !narrow, |d| d.pl(px(2.)))
                    .when(i > 0 && !narrow, |d| d.border_l_1().border_color(t.colors.hairline))
                    .when(i > 0 && narrow, |d| d.border_t_1().border_color(t.colors.hairline))
                    .child(div().text_size(px(15.)).text_color(t.colors.muted).child(r.label.clone()))
                    .child(value_row)
                    .when(!r.available, |d| d.child(div().text_size(px(12.5)).text_color(t.colors.muted).child(r.note.clone()))),
            );
            let _ = n;
        }
        Some(
            div()
                .mt(px(30.))
                .child(div().flex().items_center().justify_between().mb(px(10.)).child(eyebrow("Live readings", &t)).child(status))
                .child(strip)
                .into_any_element(),
        )
    }

    fn device_row(&self, c: &Component, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let id = c.id.clone();
        let is_new = self.new_ids.contains(&c.id);
        let sub = c.subtitle.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| facts(c, 3));
        div()
            .id(SharedString::from(format!("device-{}", c.id)))
            .role(Role::Link)
            .aria_label(format!("{}. {}", c.title, sub))
            .tab_index(0)
            .relative()
            .flex()
            .items_center()
            .gap(px(20.))
            .py(px(14.))
            .px(px(4.))
            .border_b_1()
            .border_color(t.colors.hairline)
            .cursor_pointer()
            .hover(|s| s.bg(t.colors.hover))
            .focus_visible(|s| s.border_color(t.colors.accent).bg(t.colors.hover))
            .on_activate(cx, move |this, window, cx| this.go(Page::Component(id.clone()), window, cx))
            .when(is_new, |d| d.child(div().absolute().left_0().top(px(8.)).bottom(px(8.)).w(px(3.)).rounded(px(2.)).bg(t.colors.accent)))
            .child(div().w(px(36.)).flex_none().flex().justify_center().child(icon(component_icon(c), 26., t.colors.text)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .child(div().text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).child(c.title.clone()))
                    .child(div().text_size(px(13.)).text_color(t.colors.muted).child(sub)),
            )
            .child(icon("chevron-right", 16., t.colors.faint))
            .into_any_element()
    }

    fn overview(&mut self, narrow: bool, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let inv = self.inv.clone();
        // Room for the content: the window minus the sidebar and page padding.
        let avail = (self.viewport_w - if narrow { 60. + 36. } else { 248. + 80. }).min(CONTENT_MAX - 80.);
        let two_col = !narrow && avail >= 760.;
        let labels = avail >= 960.;
        let left_w = if avail >= 960. { 440. } else { 390. };

        if let Load::Failed(msg) = self.load.clone() {
            return div().flex().flex_col().child(self.heading("A closer look.", "Every component. One clear picture.", cx)).child(self.error_panel(&msg, cx)).into_any_element();
        }

        // ---- left: headline, the machine, and its main parts
        let mut left = div().flex().flex_col().when(two_col, |d| d.w(px(left_w)).flex_none()).when(!two_col, |d| d.w_full());
        left = left.child(self.heading("A closer look.", "Every component. One clear picture.", cx));
        if let Some(n) = self.notice(cx) {
            left = left.child(n);
        }
        let connected = connected_devices(&inv);
        if let Some(sys) = inv.in_category(Category::System).next() {
            let kind = sys.subtitle.clone().unwrap_or_default();
            let line = if kind.is_empty() { pluralize(connected.len(), "connected device", "connected devices") } else { format!("{kind} · {}", pluralize(connected.len(), "connected device", "connected devices")) };
            left = left.child(
                div()
                    .id("machine-link")
                    .role(Role::Heading)
                    .aria_label(format!("{}, {line}", sys.title))
                    .mt(px(6.))
                    .mb(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(div().text_size(px(22.)).line_height(px(28.)).font_weight(FontWeight::SEMIBOLD).child(sys.title.clone()))
                    .child(div().text_size(px(14.)).text_color(t.colors.muted).child(line)),
            );
        }
        let mut table = div().flex().flex_col().border_t_1().border_color(t.colors.hairline_strong);
        let mut any = false;
        if let Some(c) = inv.in_category(Category::Processor).next() {
            table = table.child(self.table_row("Processor", c, cx));
            any = true;
        }
        if let Some(c) = inv.get("memory:system").or_else(|| inv.in_category(Category::Memory).next()) {
            table = table.child(self.table_row("Memory", c, cx));
            any = true;
        }
        for c in inv.in_category(Category::Graphics) {
            table = table.child(self.table_row("Graphics", c, cx));
            any = true;
        }
        let drives: Vec<&Component> = inv.in_category(Category::Storage).collect();
        match drives.len() {
            0 => {}
            1 => {
                table = table.child(self.table_row("Storage", drives[0], cx));
                any = true;
            }
            _ => {
                table = table.child(self.storage_summary_row(&drives, cx));
                any = true;
            }
        }
        if !any {
            table = table.child(if self.is_loading() {
                self.skeleton_rows(4, cx).into_any_element()
            } else {
                div().py(px(18.)).text_color(t.colors.muted).child("No hardware was reported.").into_any_element()
            });
        } else if self.is_loading() {
            table = table.child(self.skeleton_rows(1, cx));
        }
        left = left.child(table);

        // ---- right: the exploded machine
        let age = match self.age_secs() {
            Some(0..=4) | None => "Updated just now".to_owned(),
            Some(n) if n < 90 => format!("Updated {n} s ago"),
            Some(n) => format!("Updated {} min ago", n / 60),
        };
        let top = if two_col {
            let right_w = (avail - left_w - 36.).max(260.);
            let art_w = right_w.min(600.);
            div().flex().items_start().gap(px(36.)).child(left).child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .items_end()
                    .gap(px(6.))
                    .child(div().text_size(px(12.)).text_color(t.colors.faint).child(age))
                    .child(self.illustration(&inv, art_w, labels, cx))
                    .child(
                        div()
                            .id("explore-link")
                            .role(Role::Link)
                            .aria_label("Explore device connections")
                            .tab_index(0)
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .px(px(6.))
                            .py(px(2.))
                            .rounded(px(6.))
                            .border_1()
                            .border_color(transparent_black())
                            .cursor_pointer()
                            .text_size(px(13.))
                            .text_color(t.colors.muted)
                            .hover(|s| s.text_color(t.colors.accent_text))
                            .focus_visible(|s| s.border_color(t.colors.accent))
                            .on_activate(cx, |this, window, cx| this.go(Page::Connections, window, cx))
                            .child("Explore device connections")
                            .child(icon("arrow-right", 14., t.colors.muted)),
                    ),
            )
        } else {
            let art_w = avail.clamp(240., 520.);
            div().flex().flex_col().gap(px(20.)).child(left).child(div().flex().justify_center().child(self.illustration(&inv, art_w, false, cx)))
        };

        let mut page = div().flex().flex_col().child(top);
        if let Some(strip) = self.live_strip(&inv, narrow, cx) {
            page = page.child(strip);
        }

        // ---- connected devices
        if !connected.is_empty() {
            let total = connected.len();
            page = page.child(
                div()
                    .mt(px(34.))
                    .mb(px(4.))
                    .flex()
                    .items_baseline()
                    .justify_between()
                    .child(div().id("connected-title").role(Role::Heading).aria_label("Connected and ready").text_size(px(22.)).font_weight(FontWeight::SEMIBOLD).child("Connected & ready"))
                    .child(
                        div()
                            .id("view-all")
                            .role(Role::Link)
                            .aria_label(format!("View all {total} connected devices"))
                            .tab_index(0)
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .px(px(6.))
                            .py(px(2.))
                            .rounded(px(6.))
                            .border_1()
                            .border_color(transparent_black())
                            .cursor_pointer()
                            .text_size(px(13.))
                            .text_color(t.colors.muted)
                            .hover(|s| s.text_color(t.colors.accent_text))
                            .focus_visible(|s| s.border_color(t.colors.accent))
                            .on_activate(cx, |this, window, cx| this.go(Page::Connections, window, cx))
                            .child(format!("View all {total}"))
                            .child(icon("arrow-right", 14., t.colors.muted)),
                    ),
            );
            let mut list = div().flex().flex_col().border_t_1().border_color(t.colors.hairline_strong);
            for c in connected.iter().take(4) {
                list = list.child(self.device_row(c, cx));
            }
            page = page.child(list);
        }
        page.into_any_element()
    }

    // ---- category -------------------------------------------------------------------------

    fn component_row(&self, c: &Component, depth: usize, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let id = c.id.clone();
        let is_new = self.new_ids.contains(&c.id);
        div()
            .id(SharedString::from(format!("row-{}", c.id)))
            .role(Role::Link)
            .aria_label(format!("{}. {}", c.title, c.subtitle.clone().unwrap_or_default()))
            .tab_index(0)
            .flex()
            .items_center()
            .gap(px(14.))
            .py(px(13.))
            .pl(px(4. + depth as f32 * 26.))
            .pr(px(4.))
            .border_b_1()
            .border_color(t.colors.hairline)
            .cursor_pointer()
            .hover(|s| s.bg(t.colors.hover))
            .focus_visible(|s| s.border_color(t.colors.accent))
            .on_activate(cx, move |this, window, cx| this.go(Page::Component(id.clone()), window, cx))
            .relative()
            .when(is_new, |d| d.child(div().absolute().left_0().top(px(8.)).bottom(px(8.)).w(px(3.)).rounded(px(2.)).bg(t.colors.accent)))
            .child(icon(component_icon(c), 18., t.colors.muted))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().truncate().font_weight(FontWeight::SEMIBOLD).child(c.title.clone()))
                    .when_some(c.subtitle.clone(), |d, s| d.child(div().truncate().text_size(px(12.5)).text_color(t.colors.muted).child(s)))
                    .when(self.narrow, |d| d.child(div().text_size(px(12.5)).text_color(t.colors.muted).child(facts(c, 2)))),
            )
            .when(!self.narrow, |d| d.child(div().flex_none().text_size(px(12.5)).text_color(t.colors.muted).child(facts(c, 2))))
            .when(is_new, |d| d.child(div().text_size(px(11.)).font_weight(FontWeight::SEMIBOLD).text_color(t.colors.accent_text).child("NEW")))
            .child(icon("chevron-right", 14., t.colors.faint))
            .into_any_element()
    }

    fn tree_rows(&self, items: &[Component], parent: Option<&str>, depth: usize, out: &mut Vec<(Component, usize)>) {
        for c in items.iter().filter(|c| c.parent.as_deref() == parent) {
            out.push((c.clone(), depth));
            self.tree_rows(items, Some(&c.id), depth + 1, out);
        }
    }

    fn category_page(&mut self, cat: Category, _narrow: bool, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let items: Vec<Component> = self.inv.in_category(cat).cloned().collect();
        let sub = match items.len() {
            0 if self.is_loading() => "Reading…".to_owned(),
            0 => "Nothing reported".to_owned(),
            n => pluralize(n, "item", "items"),
        };
        let mut page = div().flex().flex_col().child(self.heading(cat.label(), sub, cx));
        if let Some(n) = self.notice(cx) {
            page = page.child(n);
        }
        if items.is_empty() {
            return page
                .child(if self.is_loading() {
                    self.skeleton_rows(3, cx).into_any_element()
                } else {
                    div()
                        .py(px(24.))
                        .flex()
                        .gap(px(12.))
                        .items_start()
                        .child(icon(category_icon(cat), 20., t.colors.faint))
                        .child(div().flex().flex_col().gap(px(4.)).child(div().font_weight(FontWeight::SEMIBOLD).child(format!("No {} found", cat.label().to_lowercase()))).child(div().text_color(t.colors.muted).child("Either this machine has none, or the system doesn't expose it to unprivileged programs.")))
                        .into_any_element()
                })
                .into_any_element();
        }
        // Peripherals: real devices first, internal PCI controllers in their own quieter section.
        // Each group is a forest of its own, so a USB tree isn't swallowed by its PCI controller.
        let (controllers, devices): (Vec<Component>, Vec<Component>) =
            if cat == Category::Peripherals { items.into_iter().partition(|c| c.id.starts_with("pci:")) } else { (Vec::new(), items) };
        let mut page = page;
        for (title, group) in [(None, devices), (Some("Internal controllers"), controllers)] {
            if group.is_empty() {
                continue;
            }
            let ids: std::collections::HashSet<&str> = group.iter().map(|c| c.id.as_str()).collect();
            let roots: Vec<Component> = group.iter().filter(|c| c.parent.as_deref().is_none_or(|p| !ids.contains(p))).cloned().collect();
            let mut ordered = Vec::new();
            for r in &roots {
                ordered.push((r.clone(), 0));
                self.tree_rows(&group, Some(&r.id), 1, &mut ordered);
            }
            if let Some(title) = title {
                page = page.child(self.section(title, cx));
            }
            let mut list = div().flex().flex_col().border_t_1().border_color(t.colors.hairline_strong);
            for (c, depth) in ordered {
                list = list.child(self.component_row(&c, depth, cx));
            }
            page = page.child(list);
        }
        page.into_any_element()
    }

    // ---- detail ---------------------------------------------------------------------------

    fn spec_row(&self, owner: &str, s: &Spec, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let (text, absent) = self.spec_text(s);
        let copy = text.clone();
        let trend = self.history_for(owner, &s.label).map(|h| {
            // Samples are stored in Celsius; say the range in the unit the page shows.
            let unit = facet_core::temperature_unit();
            let shown = |v: f64| if matches!(s.value.value(), Some(Value::Celsius(_))) { unit.from_celsius(v) } else { v };
            let (lo, hi) = h.iter().map(|v| shown(*v)).fold((f64::MAX, f64::MIN), |(l, x), v| (l.min(v), x.max(v)));
            (h.clone(), format!(". Trend over the last {} readings: {:.0} to {:.0}", h.len(), lo, hi))
        });
        let name = format!("{}: {}{}", s.label, text, trend.as_ref().map(|t| t.1.as_str()).unwrap_or(""));
        let gid = SharedString::from(format!("grp-{owner}-{}", s.label));
        div()
            .id(SharedString::from(format!("spec-{owner}-{}", s.label)))
            .group(gid.clone())
            .role(if absent { Role::Row } else { Role::Button })
            .aria_label(if absent { name.clone() } else { format!("{name}. Activate to copy.") })
            .when(!absent, |d| d.tab_index(0).cursor_pointer())
            .flex()
            .gap(px(18.))
            .py(px(10.))
            .px(px(4.))
            .border_b_1()
            .border_color(t.colors.hairline)
            .when(!absent, |d| {
                d.hover(|s| s.bg(t.colors.hover))
                    .focus_visible(|s| s.border_color(t.colors.accent).bg(t.colors.hover))
                    .on_activate(cx, move |this, _, cx| this.copy(copy.clone(), cx))
            })
            .child(
                div()
                    .w(px(if self.narrow { 140. } else { 270. }))
                    .flex_none()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_x(px(7.))
                    .text_color(t.colors.muted)
                    .child(s.label.clone())
                    .when(s.live, |d| d.child(div().size(px(6.)).rounded_full().bg(t.colors.sage)))
                    .when(s.rated, |d| d.child(div().text_size(px(10.5)).px(px(5.)).rounded(px(3.)).border_1().border_color(t.colors.hairline_strong).text_color(t.colors.faint).child("rated"))),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(
                        div()
                            .when(absent, |d| d.italic().text_color(t.colors.faint))
                            .when(!absent && matches!(s.value.value(), Some(Value::Text(x)) if x.len() > 28 && x.chars().all(|c| !c.is_whitespace() || c == ' ')) && s.identifier, |d| d.font_family(MONO_FONT))
                            .child(text),
                    )
                    .when(self.show_sources, |d| {
                        d.child(mono(format!("{}{}", s.value.source, absence_reason(s).map(|r| format!("  ·  {r}")).unwrap_or_default())).text_color(t.colors.faint).text_size(px(11.5)))
                    }),
            )
            .when_some(trend, |d, (h, _)| d.child(sparkline(&h, t.colors.sage)))
            .child(div().flex_none().invisible().group_hover(gid, |s| s.visible()).when(absent, |d| d.hidden()).child(icon("copy", 14., t.colors.faint)))
            .into_any_element()
    }

    fn detail_page(&mut self, id: &str, narrow: bool, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let Some(c) = self.inv.get(id).cloned() else {
            if self.is_loading() {
                return div().child(self.heading("Reading…", "", cx)).child(self.skeleton_rows(4, cx)).into_any_element();
            }
            return div()
                .child(self.heading("No longer connected", "This device isn't reported by the system any more.", cx))
                .child(div().text_color(t.colors.muted).mb(px(18.)).child(format!("It may have been unplugged or removed ({id}). Facet doesn't show stale readings as current.")))
                .child(self.button("to-overview", "Back to overview", false, cx, |this, window, cx| this.go(Page::Overview, window, cx)))
                .into_any_element();
        };

        let mut page = div().flex().flex_col();
        // Header.
        page = page.child(
            div()
                .mb(px(22.))
                .flex()
                .items_start()
                .gap(px(16.))
                .child(div().mt(px(6.)).size(px(44.)).flex_none().flex().items_center().justify_center().rounded(px(10.)).border_1().border_color(t.colors.hairline_strong).child(icon(component_icon(&c), 22., t.colors.accent_text)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(div().id("detail-title").role(Role::Heading).aria_label(c.title.clone()).text_size(px(if narrow { 26. } else { 32. })).line_height(px(38.)).font_weight(FontWeight::SEMIBOLD).child(c.title.clone()))
                        .when_some(c.subtitle.clone(), |d, s| d.child(div().text_size(px(15.)).text_color(t.colors.muted).child(s))),
                )
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .gap(px(8.))
                        .when(c.all_specs().any(|s| s.identifier), |d| {
                            d.child(self.button("toggle-identifiers", if self.show_identifiers { "Hide identifiers" } else { "Show identifiers" }, false, cx, |this, window, cx| this.toggle_identifiers(&crate::app::ToggleIdentifiers, window, cx)))
                        })
                        .child(self.button("toggle-sources", if self.show_sources { "Hide sources" } else { "Show sources" }, false, cx, |this, window, cx| this.toggle_sources(&crate::app::ToggleSources, window, cx))),
                ),
        );
        if let Some(n) = self.notice(cx) {
            page = page.child(n);
        }

        // Summary strip: the facts first.
        if !c.summary.is_empty() {
            let mut strip = div().flex().flex_wrap().gap_x(px(if narrow { 24. } else { 48. })).gap_y(px(14.)).pb(px(8.));
            for s in &c.summary {
                let (text, absent) = self.spec_text(s);
                let copy = text.clone();
                strip = strip.child(
                    div()
                        .id(SharedString::from(format!("sum-{}-{}", c.id, s.label)))
                        .role(if absent { Role::GenericContainer } else { Role::Button })
                        .aria_label(format!("{}: {}", s.label, text))
                        .when(!absent, |d| d.tab_index(0).cursor_pointer().hover(|x| x.text_color(t.colors.accent_text)).focus_visible(|x| x.text_color(t.colors.accent_text)).on_activate(cx, move |this, _, cx| this.copy(copy.clone(), cx)))
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(eyebrow(s.label.clone(), &t))
                        .child(div().text_size(px(22.)).line_height(px(28.)).font_weight(FontWeight::SEMIBOLD).when(absent, |d| d.font_weight(FontWeight::NORMAL).italic().text_color(t.colors.faint)).child(text)),
                );
            }
            page = page.child(strip);
        }

        // Spec groups.
        let tech_open = self.tech_open.contains(&c.id);
        for g in c.groups.iter().filter(|g| !g.technical) {
            page = page.child(self.section(&g.title, cx));
            let mut rows = div().flex().flex_col();
            for s in &g.specs {
                rows = rows.child(self.spec_row(&c.id, s, cx));
            }
            page = page.child(rows);
        }
        let tech: Vec<_> = c.groups.iter().filter(|g| g.technical).collect();
        if !tech.is_empty() {
            let cid = c.id.clone();
            page = page.child(
                div()
                    .id("tech-toggle")
                    .role(Role::Button)
                    .aria_label(if tech_open { "Hide technical details" } else { "Show technical details" })
                    .tab_index(0)
                    .mt(px(34.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .self_start()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(transparent_black())
                    .px(px(4.))
                    .py(px(2.))
                    .cursor_pointer()
                    .text_color(t.colors.accent_text)
                    .font_weight(FontWeight::SEMIBOLD)
                    .focus_visible(|s| s.border_color(t.colors.accent))
                    .on_activate(cx, move |this, _, cx| {
                        if !this.tech_open.remove(&cid) {
                            this.tech_open.insert(cid.clone());
                        }
                        cx.notify();
                    })
                    .child(icon(if tech_open { "chevron-down" } else { "chevron-right" }, 15., t.colors.accent_text))
                    .child("Technical details"),
            );
            if tech_open {
                for g in tech {
                    page = page.child(self.section(&g.title, cx));
                    let mut rows = div().flex().flex_col();
                    for s in &g.specs {
                        rows = rows.child(self.spec_row(&c.id, s, cx));
                    }
                    page = page.child(rows);
                }
            }
        }

        // Connections.
        let parent = c.parent.as_deref().and_then(|p| self.inv.get(p)).cloned();
        let children: Vec<Component> = self.inv.children_of(&c.id).cloned().collect();
        if parent.is_some() || !children.is_empty() {
            page = page.child(self.section("Connections", cx));
            let mut list = div().flex().flex_col();
            if let Some(p) = &parent {
                list = list.child(self.machine_row("Connected via", p, cx));
            }
            for ch in &children {
                list = list.child(self.machine_row("Connected", ch, cx));
            }
            page = page.child(list);
        }
        page.into_any_element()
    }

    // ---- connections ----------------------------------------------------------------------

    fn connections_page(&mut self, _narrow: bool, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let all = self.inv.components.clone();
        let ids: std::collections::HashSet<&str> = all.iter().map(|c| c.id.as_str()).collect();
        let has_children = |id: &str| all.iter().any(|c| c.parent.as_deref() == Some(id));
        let roots: Vec<Component> =
            all.iter().filter(|c| c.parent.as_deref().is_none_or(|p| !ids.contains(p)) && has_children(&c.id)).cloned().collect();
        let lonely = all.iter().filter(|c| c.parent.as_deref().is_none_or(|p| !ids.contains(p)) && !has_children(&c.id)).count();
        let mut page = div().flex().flex_col().child(self.heading("Device connections", "What is plugged into what: devices behind hubs and docks, displays behind graphics, adapters on the PCI bus.", cx));
        if roots.is_empty() {
            return page
                .child(div().text_color(t.colors.muted).child(if self.is_loading() { "Reading…" } else { "No connected hierarchy was reported on this machine." }))
                .into_any_element();
        }
        for r in &roots {
            let mut ordered = vec![(r.clone(), 0usize)];
            self.tree_rows(&all, Some(&r.id), 1, &mut ordered);
            let mut block = div().flex().flex_col().mb(px(18.)).border_t_1().border_color(t.colors.hairline_strong);
            for (c, depth) in ordered {
                block = block.child(self.component_row(&c, depth, cx));
            }
            page = page.child(block);
        }
        if lonely > 0 {
            page = page.child(div().text_size(px(13.)).text_color(t.colors.muted).child(format!("{} more with no connected hierarchy are listed in their categories.", pluralize(lonely, "component", "components"))));
        }
        page.into_any_element()
    }

    // ---- preferences ----------------------------------------------------------------------

    fn preferences_page(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let current = self.theme_pref;
        let mut seg = div().flex().gap(px(0.)).self_start().rounded(px(8.)).border_1().border_color(t.colors.hairline_strong).overflow_hidden();
        for (id, label, pref) in [("theme-system", "System", ThemePref::System), ("theme-light", "Light", ThemePref::Light), ("theme-dark", "Dark", ThemePref::Dark)] {
            let active = current == pref;
            seg = seg.child(
                div()
                    .id(id)
                    .role(Role::Button)
                    .aria_label(format!("{label} appearance"))
                    .tab_index(0)
                    .px(px(16.))
                    .h(px(34.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .when(active, |d| d.bg(t.colors.accent).text_color(t.colors.on_accent).font_weight(FontWeight::SEMIBOLD))
                    .when(!active, |d| d.hover(|s| s.bg(t.colors.hover)))
                    .on_activate(cx, move |this, window, cx| this.set_theme_pref(pref, window, cx))
                    .child(label),
            );
        }
        let dark = t.mode == Mode::Dark;
        let temp_unit = facet_core::temperature_unit();
        let mut temp_seg = div().flex().self_start().rounded(px(8.)).border_1().border_color(t.colors.hairline_strong).overflow_hidden();
        for (id, unit) in [("temp-f", facet_core::TemperatureUnit::Fahrenheit), ("temp-c", facet_core::TemperatureUnit::Celsius)] {
            let active = temp_unit == unit;
            temp_seg = temp_seg.child(
                div()
                    .id(id)
                    .role(Role::Button)
                    .aria_label(format!("{} ({})", unit.label(), unit.symbol()))
                    .tab_index(0)
                    .px(px(16.))
                    .h(px(34.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .border_1()
                    .border_color(transparent_black())
                    .when(active, |d| d.bg(t.colors.accent).text_color(t.colors.on_accent).font_weight(FontWeight::SEMIBOLD))
                    .when(!active, |d| d.hover(|s| s.bg(t.colors.hover)))
                    .focus_visible(|s| s.border_color(t.colors.text))
                    .on_activate(cx, move |this, _, cx| this.set_temperature_pref(unit, cx))
                    .child(format!("{} {}", unit.label(), unit.symbol())),
            );
        }
        let live = self.live;
        let hotplug_line = match &self.hotplug {
            crate::app::Hotplug::Active => "Hotplug detection is on: plugging or unplugging hardware updates this window automatically.".to_owned(),
            crate::app::Hotplug::Off => "Hotplug detection is off for this demo data.".to_owned(),
            crate::app::Hotplug::Unavailable(why) => format!("Hotplug detection isn't available here ({why}). Press Ctrl R to re-read the hardware."),
        };
        let shortcut = |keys: &str, what: &str| {
            div().flex().items_center().gap(px(16.)).py(px(8.)).border_b_1().border_color(t.colors.hairline).child(div().w(px(140.)).child(mono(keys.to_owned()))).child(div().text_color(t.colors.muted).child(what.to_owned()))
        };
        div()
            .flex()
            .flex_col()
            .child(self.heading("Preferences", "Facet only reads. It never changes your hardware.", cx))
            .child(self.section("Appearance", cx))
            .child(div().py(px(10.)).flex().flex_col().gap(px(8.)).child(seg).child(div().text_size(px(13.)).text_color(t.colors.muted).child(if dark { "Currently showing the dark variant." } else { "Currently showing the light variant." })))
            .child(self.section("Temperature", cx))
            .child(
                div()
                    .py(px(10.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(temp_seg)
                    .child(div().max_w(px(640.)).text_size(px(13.)).text_color(t.colors.muted).child("Used everywhere in Facet, in reports, in the facet command and in the Omarchy bar widget. Exported JSON always keeps degrees Celsius so it stays unambiguous.")),
            )
            .child(self.section("Live updates", cx))
            .child(
                div()
                    .py(px(10.))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(
                        div()
                            .id("live-toggle")
                            .role(Role::CheckBox)
                            .aria_label("Live monitoring")
                            .tab_index(0)
                            .self_start()
                            .flex()
                            .items_center()
                            .gap(px(9.))
                            .cursor_pointer()
                            .rounded(px(5.))
                            .border_1()
                            .border_color(transparent_black())
                            .focus_visible(|s| s.border_color(t.colors.accent))
                            .on_activate(cx, move |this, _, cx| this.set_live(!this.live, cx))
                            .child(
                                div()
                                    .size(px(18.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(4.))
                                    .border_1()
                                    .border_color(if live { t.colors.accent } else { t.colors.hairline_strong })
                                    .when(live, |d| d.bg(t.colors.accent).child(icon("check", 13., t.colors.on_accent))),
                            )
                            .child(div().font_weight(FontWeight::SEMIBOLD).child("Live monitoring")),
                    )
                    .child(div().max_w(px(640.)).text_size(px(13.)).text_color(t.colors.muted).child("Re-reads the numbers that change (temperatures, memory, battery, GPU state) every two seconds and keeps the last three minutes as small trend charts. On by default; turn it off if you want Facet completely idle."))
                    .child(div().text_size(px(13.)).text_color(t.colors.muted).child(hotplug_line))
                    .child(div().text_size(px(13.)).text_color(t.colors.muted).child(match self.age_secs() {
                        Some(0) => "Hardware was last read just now.".to_owned(),
                        Some(n) => format!("Hardware was last read {n} s ago."),
                        None => "Hardware has not been read yet.".to_owned(),
                    })),
            )
            .child(self.section("Keyboard", cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(shortcut("Ctrl K", "Search all hardware"))
                    .child(shortcut("Ctrl E", "Export a report"))
                    .child(shortcut("Ctrl R", "Re-read the hardware"))
                    .child(shortcut("Alt ←", "Go back"))
                    .child(shortcut("Ctrl Shift S", "Show or hide data sources"))
                    .child(shortcut("Ctrl Shift I", "Show or hide serial numbers and addresses"))
                    .child(shortcut("Ctrl Shift M", "Turn live monitoring on or off"))
                    .child(shortcut("Ctrl Shift L", "Toggle light and dark"))
                    .child(shortcut("Tab", "Move between controls")),
            )
            .child(self.section("Privacy", cx))
            .child(div().py(px(10.)).text_color(t.colors.muted).max_w(px(640.)).child("Facet reads hardware information on this computer and sends nothing anywhere. Serial numbers, UUIDs and MAC addresses are hidden by default, including in exported reports, until you choose to include them."))
            .child(self.section("About", cx))
            .child(
                div()
                    .id("about")
                    .role(Role::GenericContainer)
                    .aria_label(format!("About Facet, version {VERSION}"))
                    .py(px(10.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(div().flex().gap(px(16.)).child(div().w(px(140.)).text_color(t.colors.muted).child("Version")).child(mono(VERSION.to_owned())))
                    .child(div().flex().gap(px(16.)).child(div().w(px(140.)).text_color(t.colors.muted).child("What it does")).child("Reads hardware information. It never changes your hardware."))
                    .child(div().flex().gap(px(16.)).child(div().w(px(140.)).text_color(t.colors.muted).child("Licence")).child("MIT or Apache-2.0"))
                    .child(div().flex().gap(px(16.)).child(div().w(px(140.)).text_color(t.colors.muted).child("Project")).child(mono("github.com/last-refuge/omarchy-facet".to_owned()))),
            )
            .into_any_element()
    }
}

/// What is plugged into or paired with this machine: displays, USB devices (not the root hubs),
/// Bluetooth adapters and cameras. Internal PCI parts and input nodes belong to the machine, not to it.
fn connected_devices(inv: &Inventory) -> Vec<Component> {
    let mut out: Vec<Component> = inv.in_category(Category::Displays).cloned().collect();
    out.extend(
        inv.in_category(Category::Peripherals)
            .filter(|c| (c.id.starts_with("usb:") && !c.id.starts_with("usb:usb")) || c.id.starts_with("bluetooth:") || c.id.starts_with("camera:"))
            .cloned(),
    );
    out
}
