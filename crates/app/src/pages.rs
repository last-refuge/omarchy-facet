//! Page bodies: overview, category lists, component detail, connections, preferences, and the
//! loading / error states.

use facet_core::{Category, Component, Spec, Value};
use gpui::{
    AnyElement, ClickEvent, Context, Div, FontWeight, InteractiveElement, IntoElement, ParentElement, Role,
    SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::*, px, transparent_black,
};

use crate::app::{Facet, Load, Page, ThemePref};
use crate::theme::{MONO_FONT, Mode, ThemeExt};
use crate::widgets::{absence_reason, category_icon, component_icon, eyebrow, icon, mono, pluralize, sparkline};

/// Short human phrase for a summary spec, or `None` when it adds nothing ("No").
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
            .child(div().w_full().max_w(px(1040.)).mx_auto().px(px(pad)).pt(px(if narrow { 20. } else { 34. })).pb(px(56.)).child(body))
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
        let t = cx.theme().clone();
        div()
            .id(id)
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
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| f(this, window, cx)))
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
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.go(Page::Component(id.clone()), window, cx)))
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

    fn overview(&mut self, narrow: bool, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let mut page = div().flex().flex_col();
        page = page.child(self.heading("A closer look.", "Every component. One clear picture.", cx));

        if let Load::Failed(msg) = self.load.clone() {
            return page.child(self.error_panel(&msg, cx)).into_any_element();
        }
        if let Some(n) = self.notice(cx) {
            page = page.child(n);
        }

        // This machine.
        page = page.child(self.section("This machine", cx));
        let mut rows = div().flex().flex_col();
        let mut any = false;
        let inv = self.inv.clone();
        if let Some(c) = inv.in_category(Category::Processor).next() {
            rows = rows.child(self.machine_row("Processor", c, cx));
            any = true;
        }
        if let Some(c) = inv.get("memory:system").or_else(|| inv.in_category(Category::Memory).next()) {
            rows = rows.child(self.machine_row("Memory", c, cx));
            any = true;
        }
        for c in inv.in_category(Category::Graphics) {
            rows = rows.child(self.machine_row("Graphics", c, cx));
            any = true;
        }
        for c in inv.in_category(Category::Storage) {
            rows = rows.child(self.machine_row("Storage", c, cx));
            any = true;
        }
        if !any {
            rows = rows.child(if self.is_loading() {
                self.skeleton_rows(4, cx).into_any_element()
            } else {
                div().py(px(18.)).text_color(t.colors.muted).child("No hardware was reported.").into_any_element()
            });
        } else if self.is_loading() {
            rows = rows.child(self.skeleton_rows(1, cx));
        }
        page = page.child(rows);

        // Live now.
        let readings = live_readings(&inv);
        if !readings.is_empty() {
            page = page.child(self.section("Live now", cx));
            let mut strip = div().flex().gap(px(if narrow { 20. } else { 56. })).when(narrow, |d| d.flex_col()).py(px(8.));
            for (label, value, note) in readings {
                strip = strip.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(eyebrow(label.clone(), &t))
                        .child(div().text_size(px(40.)).line_height(px(46.)).font_weight(FontWeight::SEMIBOLD).text_color(if value.starts_with('—') { t.colors.faint } else { t.colors.text }).child(value))
                        .child(div().text_size(px(12.5)).text_color(t.colors.muted).child(note)),
                );
            }
            page = page.child(strip);
        }

        // Connected devices.
        let connected: Vec<Component> = inv
            .components
            .iter()
            .filter(|c| matches!(c.category, Category::Displays | Category::Peripherals | Category::Network) && !c.id.starts_with("pci:") && !c.id.starts_with("usb:usb"))
            .take(7)
            .cloned()
            .collect();
        if !connected.is_empty() {
            page = page.child(self.section("Connected devices", cx));
            let mut list = div().flex().flex_col();
            for c in &connected {
                list = list.child(self.machine_row(c.category.label(), c, cx));
            }
            page = page.child(list).child(
                div().mt(px(14.)).child(self.button("see-connections", "Explore connections", false, cx, |this, window, cx| this.go(Page::Connections, window, cx))),
            );
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
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.go(Page::Component(id.clone()), window, cx)))
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
            let (lo, hi) = h.iter().fold((f64::MAX, f64::MIN), |(l, x), v| (l.min(*v), x.max(*v)));
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
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.copy(copy.clone(), cx)))
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
                        .when(!absent, |d| d.tab_index(0).cursor_pointer().hover(|x| x.text_color(t.colors.accent_text)).focus_visible(|x| x.text_color(t.colors.accent_text)).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.copy(copy.clone(), cx))))
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
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        if !this.tech_open.remove(&cid) {
                            this.tech_open.insert(cid.clone());
                        }
                        cx.notify();
                    }))
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
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.set_theme_pref(pref, window, cx)))
                    .child(label),
            );
        }
        let dark = t.mode == Mode::Dark;
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
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.set_live(!this.live, cx)))
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
                    .child(div().max_w(px(640.)).text_size(px(13.)).text_color(t.colors.muted).child("Re-reads the hardware every two seconds and keeps the last three minutes of readings as small trend charts. Off by default so Facet stays idle when you are not looking."))
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
            .into_any_element()
    }
}

/// Up to three headline readings for the overview. Absent values are explained, never zero.
fn live_readings(inv: &facet_core::Inventory) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let find = |pred: &dyn Fn(&Component, &Spec) -> bool| -> Option<(&Component, &Spec)> {
        inv.components.iter().find_map(|c| c.all_specs().find(|s| s.live && pred(c, s)).map(|s| (c, s)))
    };
    let celsius = |s: &Spec| matches!(s.value.value(), Some(Value::Celsius(_)));

    if let Some((_, s)) = find(&|c, s| c.id.starts_with("sensors:") && celsius(s) && (s.label.contains("Package") || s.label.contains("Tctl") || s.label.contains("Tdie"))) {
        out.push(("CPU temperature".to_owned(), s.display(), "Package sensor".to_owned()));
    } else if let Some((c, s)) = find(&|c, s| c.category == Category::Processor && celsius(s)) {
        out.push(("CPU temperature".to_owned(), s.display(), c.title.clone()));
    }
    if let Some((_, s)) = find(&|c, s| c.id == "memory:system" && s.label == "In use") {
        out.push(("Memory in use".to_owned(), s.display(), "Of installed memory".to_owned()));
    } else if let Some(mem) = inv.get("memory:system") {
        let bytes = |label: &str| mem.all_specs().find(|s| s.label == label).and_then(|s| match s.value.value() {
            Some(Value::BinaryBytes(b) | Value::Bytes(b)) => Some(*b),
            _ => None,
        });
        if let (Some(total), Some(avail)) = (bytes("Usable memory"), bytes("Available memory"))
            && total > 0
            && avail <= total
        {
            let pct = 100.0 * (total - avail) as f64 / total as f64;
            out.push(("Memory in use".to_owned(), Value::Percent(pct).display(), "Of usable memory".to_owned()));
        }
    }
    let gpus: Vec<(&Component, &Spec)> = inv
        .in_category(Category::Graphics)
        .filter_map(|g| g.all_specs().find(|s| s.label == "Temperature").map(|s| (g, s)))
        .collect();
    if let Some((g, s)) = gpus.iter().find(|(_, s)| s.is_available()) {
        out.push(("GPU temperature".to_owned(), s.display(), g.title.clone()));
    } else if let Some((g, s)) = gpus.iter().find(|(_, s)| s.value.source.contains("suspended")).or(gpus.first()) {
        let asleep = s.value.source.contains("suspended");
        let note = if asleep { format!("{} is asleep; not queried so it stays asleep", g.title) } else { format!("{}: the driver doesn't report it", g.title) };
        out.push(("GPU temperature".to_owned(), "— °C".to_owned(), note));
    }
    out
}
