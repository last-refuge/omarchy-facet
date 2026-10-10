//! Window chrome: sidebar, toolbar, and the root layout.

use facet_core::Category;
use gpui::{
    AnyElement, ClickEvent, Context, FontWeight, InteractiveElement, IntoElement, ParentElement, Render, Role, SharedString,
    StatefulInteractiveElement, Styled, Window, div, prelude::*, px, svg, transparent_black,
};

use crate::app::{Facet, Load, Page};
use crate::theme::{ThemeExt, UI_FONT};
use crate::widgets::{category_icon, icon, pluralize};

pub const NARROW_BELOW: f32 = 880.0;

static FIRST_FRAME: std::sync::Once = std::sync::Once::new();

impl Render for Facet {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        FIRST_FRAME.call_once(|| crate::timing("first-render-ms"));
        let t = cx.theme().clone();
        let narrow = f32::from(window.viewport_size().width) < NARROW_BELOW;
        self.narrow = narrow;

        let content: AnyElement = self.page_content(narrow, window, cx);

        let mut root = div()
            .id("facet-root")
            .key_context(if self.export.is_some() { "Facet Overlay" } else { "Facet" })
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::open_search))
            .on_action(cx.listener(Self::open_export))
            .on_action(cx.listener(Self::refresh))
            .on_action(cx.listener(Self::go_back))
            .on_action(cx.listener(Self::toggle_theme))
            .on_action(cx.listener(Self::toggle_sources))
            .on_action(cx.listener(Self::toggle_identifiers))
            .on_action(cx.listener(Self::toggle_live))
            .size_full()
            .flex()
            .bg(t.colors.surface)
            .text_color(t.colors.text)
            .font_family(UI_FONT)
            .text_size(px(14.5))
            .child(self.sidebar(narrow, cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(self.toolbar(narrow, cx))
                    .child(div().flex_1().min_h_0().child(content)),
            );

        if let Some(overlay) = self.palette_overlay(window, cx) {
            root = root.child(overlay);
        }
        if let Some(overlay) = self.export_overlay(window, cx) {
            root = root.child(overlay);
        }
        if let Some(toast) = self.toast_view(cx) {
            root = root.child(toast);
        }
        root
    }
}

impl Facet {
    #[allow(clippy::too_many_arguments)]
    fn nav_item(
        &self,
        id: &'static str,
        icon_name: &'static str,
        label: &str,
        count: Option<usize>,
        active: bool,
        narrow: bool,
        page: Page,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = cx.theme().clone();
        let name = match count {
            Some(n) => format!("{label}, {}", pluralize(n, "item", "items")),
            None => label.to_owned(),
        };
        let fg = if active { t.colors.nav_text } else { t.colors.nav_muted };
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(name)
            .tab_index(0)
            .flex()
            .items_center()
            .gap(px(11.))
            .h(px(36.))
            .px(px(if narrow { 0. } else { 12. }))
            .when(narrow, |d| d.justify_center())
            .rounded(px(7.))
            .border_1()
            .border_color(transparent_black())
            .text_color(fg)
            .when(active, |d| d.bg(t.colors.nav_active).text_color(t.colors.nav_text).font_weight(FontWeight::SEMIBOLD))
            .cursor_pointer()
            .hover(|s| s.bg(t.colors.nav_active).text_color(t.colors.nav_text))
            .focus_visible(|s| s.border_color(t.colors.accent))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.go(page.clone(), window, cx)))
            .child(icon(icon_name, 17., if active { t.colors.accent } else { fg }))
            .when(!narrow, |d| {
                d.child(div().flex_1().text_size(px(14.)).child(label.to_owned())).when_some(count, |d, n| {
                    d.child(div().text_size(px(12.)).text_color(t.colors.nav_muted).child(n.to_string()))
                })
            })
    }

    fn sidebar(&self, narrow: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let machine = self.inv.in_category(Category::System).next();
        let mut nav = div()
            .id("sidebar")
            .role(Role::Navigation)
            .aria_label("Hardware categories")
            .flex_none()
            .w(px(if narrow { 60. } else { 248. }))
            .h_full()
            .flex()
            .flex_col()
            .gap(px(2.))
            .px(px(if narrow { 8. } else { 12. }))
            .py(px(16.))
            .bg(t.colors.nav)
            .text_color(t.colors.nav_text);

        // Wordmark.
        nav = nav.child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(if narrow { 0. } else { 8. }))
                .when(narrow, |d| d.justify_center())
                .h(px(40.))
                .child(svg().path("brand/mark.svg").size(px(24.)).flex_none().text_color(t.colors.accent))
                .when(!narrow, |d| {
                    d.child(div().text_size(px(21.)).font_weight(FontWeight::SEMIBOLD).text_color(t.colors.nav_text).child("facet"))
                }),
        );

        // Machine identity.
        if !narrow {
            let (title, sub) = match machine {
                Some(m) => (m.title.clone(), m.subtitle.clone().unwrap_or_default()),
                None if self.is_loading() => ("Reading this machine…".to_owned(), String::new()),
                None => ("This machine".to_owned(), String::new()),
            };
            nav = nav.child(
                div()
                    .px(px(8.))
                    .pt(px(6.))
                    .pb(px(14.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).text_color(t.colors.nav_text).child(title))
                    .when(!sub.is_empty(), |d| d.child(div().text_size(px(12.)).text_color(t.colors.nav_muted).child(sub))),
            );
        } else {
            nav = nav.child(div().h(px(12.)));
        }

        nav = nav.child(self.nav_item("nav-overview", "layout-dashboard", "Overview", None, self.page == Page::Overview, narrow, Page::Overview, cx));
        for c in Category::ALL {
            let n = self.count(c);
            let shown = if n == 0 && self.load != Load::Ready { None } else { Some(n) };
            let active = matches!(&self.page, Page::Category(x) if *x == c);
            if n == 0 && !self.is_loading() && c != Category::System {
                // Empty categories stay visible (and honest) rather than disappearing.
            }
            nav = nav.child(self.nav_item(category_nav_id(c), category_icon(c), c.label(), shown, active, narrow, Page::Category(c), cx));
        }

        nav = nav.child(div().flex_1()).child(div().h(px(1.)).my(px(8.)).bg(t.colors.nav_active));
        nav = nav
            .child(self.nav_item("nav-connections", "git-fork", "Device connections", None, self.page == Page::Connections, narrow, Page::Connections, cx))
            .child(self.nav_item("nav-preferences", "sliders-horizontal", "Preferences", None, self.page == Page::Preferences, narrow, Page::Preferences, cx));
        nav
    }

    fn breadcrumb(&self) -> Vec<(String, Option<Page>)> {
        let mut crumbs = vec![("Overview".to_owned(), Some(Page::Overview))];
        match &self.page {
            Page::Overview => return vec![("Overview".into(), None)],
            Page::Category(c) => crumbs.push((c.label().into(), None)),
            Page::Component(id) => match self.inv.get(id) {
                Some(c) => {
                    crumbs.push((c.category.label().into(), Some(Page::Category(c.category))));
                    crumbs.push((c.title.clone(), None));
                }
                None => crumbs.push(("Device".into(), None)),
            },
            Page::Connections => crumbs.push(("Device connections".into(), None)),
            Page::Preferences => crumbs.push(("Preferences".into(), None)),
        }
        crumbs
    }

    fn toolbar(&self, narrow: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let crumbs = self.breadcrumb();
        let last = crumbs.len() - 1;
        let mut trail = div().flex().items_center().gap(px(6.)).min_w_0().overflow_hidden();
        if self.can_go_back() {
            trail = trail.child(
                div()
                    .id("back")
                    .role(Role::Button)
                    .aria_label("Back")
                    .tab_index(0)
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(28.))
                    .mr(px(4.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(transparent_black())
                    .cursor_pointer()
                    .text_color(t.colors.muted)
                    .hover(|s| s.bg(t.colors.hover).text_color(t.colors.text))
                    .focus_visible(|s| s.border_color(t.colors.accent))
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.go_back(&crate::app::GoBack, window, cx)))
                    .child(icon("chevron-left", 16., t.colors.muted)),
            );
        }
        for (i, (label, page)) in crumbs.into_iter().enumerate() {
            if i > 0 {
                trail = trail.child(div().text_color(t.colors.faint).child("/"));
            }
            let is_last = i == last;
            let el: AnyElement = match page {
                Some(p) if !is_last => div()
                    .id(SharedString::from(format!("crumb-{i}")))
                    .role(Role::Link)
                    .aria_label(label.clone())
                    .tab_index(0)
                    .px(px(4.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(transparent_black())
                    .cursor_pointer()
                    .text_color(t.colors.muted)
                    .hover(|s| s.text_color(t.colors.accent_text))
                    .focus_visible(|s| s.border_color(t.colors.accent))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.go(p.clone(), window, cx)))
                    .child(label)
                    .into_any_element(),
                _ => div().text_color(t.colors.text).font_weight(FontWeight::SEMIBOLD).truncate().child(label).into_any_element(),
            };
            trail = trail.child(el);
        }

        let search = div()
            .id("open-search")
            .role(Role::Button)
            .aria_label("Search hardware")
            .aria_keyshortcuts("Control+K")
            .tab_index(0)
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(34.))
            .px(px(10.))
            .when(!narrow, |d| d.w(px(250.)))
            .rounded(px(8.))
            .border_1()
            .border_color(t.colors.hairline_strong)
            .text_color(t.colors.muted)
            .cursor_pointer()
            .hover(|s| s.bg(t.colors.hover))
            .focus_visible(|s| s.border_color(t.colors.accent))
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.open_search(&crate::app::OpenSearch, window, cx)))
            .child(icon("search", 15., t.colors.muted))
            .when(!narrow, |d| {
                d.child(div().flex_1().text_size(px(13.)).child("Search hardware"))
                    .child(div().text_size(px(11.5)).px(px(5.)).py(px(1.)).rounded(px(4.)).border_1().border_color(t.colors.hairline_strong).text_color(t.colors.faint).child("Ctrl K"))
            });

        let export = div()
            .id("open-export")
            .role(Role::Button)
            .aria_label("Export report")
            .aria_keyshortcuts("Control+E")
            .tab_index(0)
            .flex()
            .items_center()
            .gap(px(7.))
            .h(px(34.))
            .px(px(if narrow { 10. } else { 14. }))
            .rounded(px(8.))
            .border_1()
            .border_color(t.colors.accent)
            .bg(t.colors.accent)
            .text_color(t.colors.on_accent)
            .font_weight(FontWeight::SEMIBOLD)
            .text_size(px(13.))
            .cursor_pointer()
            .hover(|s| s.opacity(0.92))
            .focus_visible(|s| s.border_color(t.colors.text))
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.open_export(&crate::app::OpenExport, window, cx)))
            .child(icon("download", 15., t.colors.on_accent))
            .when(!narrow, |d| d.child("Export report"));

        let busy = matches!(self.load, Load::Loading { .. });
        div()
            .id("toolbar")
            .role(Role::Toolbar)
            .aria_label("Toolbar")
            .flex_none()
            .h(px(56.))
            .px(px(if narrow { 14. } else { 28. }))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(16.))
            .border_b_1()
            .border_color(t.colors.hairline)
            .child(trail)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .flex_none()
                    .when(busy, |d| d.child(div().text_size(px(12.)).text_color(t.colors.faint).child("Reading hardware…")))
                    .when(self.live && !busy, |d| {
                        d.child(
                            div()
                                .id("live-chip")
                                .role(Role::Status)
                                .aria_label("Live monitoring is on")
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .text_size(px(12.))
                                .text_color(t.colors.muted)
                                .child(div().size(px(7.)).rounded_full().bg(t.colors.sage))
                                .when(!narrow, |d| d.child("Live")),
                        )
                    })
                    .child(search)
                    .child(export),
            )
    }
}

fn category_nav_id(c: Category) -> &'static str {
    match c {
        Category::System => "nav-system",
        Category::Processor => "nav-processor",
        Category::Memory => "nav-memory",
        Category::Graphics => "nav-graphics",
        Category::Storage => "nav-storage",
        Category::Network => "nav-network",
        Category::Displays => "nav-displays",
        Category::Peripherals => "nav-peripherals",
        Category::Power => "nav-power",
    }
}
