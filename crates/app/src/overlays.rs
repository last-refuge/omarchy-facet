//! Modal surfaces: the search palette, the export dialog, and the copy toast.

use gpui::{
    AnyElement, ClickEvent, Context, FontWeight, InteractiveElement, IntoElement, MouseButton, ParentElement, Role, SharedString,
    StatefulInteractiveElement, Styled, Window, div, prelude::*, px, transparent_black,
};

use crate::app::{ExportFormat, Facet, Page};
use crate::theme::{MONO_FONT, ThemeExt};
use crate::widgets::{component_icon, icon, pluralize};

impl Facet {
    fn scrim(&self, cx: &Context<Self>) -> gpui::Stateful<gpui::Div> {
        let t = cx.theme();
        div()
            .id("scrim")
            .absolute()
            .inset_0()
            .bg(t.colors.scrim)
            .flex()
            .flex_col()
            .items_center()
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| this.close_overlay(&crate::app::CloseOverlay, window, cx)))
    }

    pub fn palette_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let t = cx.theme().clone();
        let state = self.palette.as_ref()?;
        let input = state.input.clone();
        let query = input.read(cx).text().to_owned();
        let selected = state.selected;
        let results: Vec<_> = state.results.iter().filter_map(|id| self.inv.get(id).cloned()).collect();
        let width = f32::from(window.viewport_size().width);
        let panel_w = (width - 32.).clamp(300., 640.);

        let mut list = div()
            .id("palette-results")
            .role(Role::ListBox)
            .aria_label("Search results")
            .max_h(px(380.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .py(px(6.));
        if results.is_empty() {
            list = list.child(
                div()
                    .px(px(18.))
                    .py(px(22.))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(if self.is_loading() && self.inv.components.is_empty() { "Still reading hardware…" } else { "No matching hardware" }))
                    .child(div().text_size(px(13.)).text_color(t.colors.muted).child("Try a model, manufacturer, driver, or a plain description like “nvme” or “usb 3”. Serial numbers are not searchable.")),
            );
        }
        for (i, c) in results.iter().enumerate() {
            let id = c.id.clone();
            let active = i == selected;
            list = list.child(
                div()
                    .id(SharedString::from(format!("result-{}", c.id)))
                    .role(Role::ListBoxOption)
                    .aria_label(format!("{}, {}", c.title, c.category.label()))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .mx(px(6.))
                    .px(px(12.))
                    .py(px(9.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .when(active, |d| d.bg(t.colors.hover))
                    .hover(|s| s.bg(t.colors.hover))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.go(Page::Component(id.clone()), window, cx)))
                    .child(icon(component_icon(c), 17., if active { t.colors.accent_text } else { t.colors.muted }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().truncate().font_weight(FontWeight::SEMIBOLD).child(c.title.clone()))
                            .when_some(c.subtitle.clone(), |d, s| d.child(div().truncate().text_size(px(12.)).text_color(t.colors.muted).child(s))),
                    )
                    .child(div().flex_none().text_size(px(12.)).text_color(t.colors.faint).child(c.category.label())),
            );
        }

        let panel = div()
            .id("palette")
            .key_context("Overlay Palette")
            .role(Role::Dialog)
            .aria_label("Search hardware")
            .on_action(cx.listener(Self::close_overlay))
            .on_action(cx.listener(Self::move_up))
            .on_action(cx.listener(Self::move_down))
            .on_action(cx.listener(Self::confirm))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .mt(px(72.))
            .w(px(panel_w))
            .rounded(px(14.))
            .border_1()
            .border_color(t.colors.hairline_strong)
            .bg(t.colors.raised)
            .shadow_lg()
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .px(px(18.))
                    .h(px(54.))
                    .border_b_1()
                    .border_color(t.colors.hairline)
                    .text_size(px(16.))
                    .child(icon("search", 18., t.colors.muted))
                    .child(div().id("palette-input").role(Role::SearchInput).aria_label("Search hardware").flex_1().child(input)),
            )
            .child(list)
            .child(
                div()
                    .flex()
                    .justify_between()
                    .px(px(18.))
                    .h(px(36.))
                    .items_center()
                    .border_t_1()
                    .border_color(t.colors.hairline)
                    .text_size(px(12.))
                    .text_color(t.colors.faint)
                    .child(if query.trim().is_empty() { pluralize(self.inv.components.len(), "component", "components") } else { pluralize(results.len(), "match", "matches") })
                    .child("↑↓ move   ↵ open   esc close"),
            );
        Some(self.scrim(cx).child(panel).into_any_element())
    }

    pub fn export_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let t = cx.theme().clone();
        let state = self.export.as_ref()?;
        let format = state.format;
        let include = state.include_identifiers;
        let preview = self.export_text();
        let lines = preview.lines().count();
        let width = f32::from(window.viewport_size().width);
        let panel_w = (width - 32.).clamp(300., 780.);
        let height = f32::from(window.viewport_size().height);

        let seg = |id: &'static str, label: &'static str, f: ExportFormat, cx: &mut Context<Self>| {
            let active = format == f;
            div()
                .id(id)
                .role(Role::Button)
                .aria_label(format!("{label} format"))
                .tab_index(0)
                .px(px(14.))
                .h(px(32.))
                .flex()
                .items_center()
                .cursor_pointer()
                .when(active, |d| d.bg(t.colors.accent).text_color(t.colors.on_accent).font_weight(FontWeight::SEMIBOLD))
                .when(!active, |d| d.hover(|s| s.bg(t.colors.hover)))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if let Some(e) = this.export.as_mut() {
                        e.format = f;
                    }
                    cx.notify();
                }))
                .child(label)
        };

        let checkbox = div()
            .id("include-identifiers")
            .role(Role::CheckBox)
            .aria_label("Include serial numbers, UUIDs and MAC addresses")
            .tab_index(0)
            .flex()
            .items_center()
            .gap(px(9.))
            .cursor_pointer()
            .rounded(px(5.))
            .border_1()
            .border_color(transparent_black())
            .focus_visible(|s| s.border_color(t.colors.accent))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                if let Some(e) = this.export.as_mut() {
                    e.include_identifiers = !e.include_identifiers;
                }
                cx.notify();
            }))
            .child(
                div()
                    .size(px(18.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.))
                    .border_1()
                    .border_color(if include { t.colors.accent } else { t.colors.hairline_strong })
                    .when(include, |d| d.bg(t.colors.accent))
                    .when(include, |d| d.child(icon("check", 13., t.colors.on_accent))),
            )
            .child(div().text_size(px(13.5)).child("Include serial numbers, UUIDs and MAC addresses"));

        let copy_text = preview.clone();
        let panel = div()
            .id("export")
            .key_context("Overlay")
            .role(Role::Dialog)
            .aria_label("Export report")
            .on_action(cx.listener(Self::close_overlay))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .mt(px(48.))
            .w(px(panel_w))
            .max_h(px((height - 72.).max(240.)))
            .flex()
            .flex_col()
            .rounded(px(14.))
            .border_1()
            .border_color(t.colors.hairline_strong)
            .bg(t.colors.raised)
            .shadow_lg()
            .overflow_hidden()
            .child(
                div()
                    .px(px(22.))
                    .pt(px(18.))
                    .pb(px(14.))
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .child(div().flex().justify_between().items_center().child(div().text_size(px(20.)).font_weight(FontWeight::SEMIBOLD).child("Export report")).child(
                        div()
                            .id("export-close")
                            .role(Role::Button)
                            .aria_label("Close")
                            .tab_index(0)
                            .size(px(30.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(6.))
                            .cursor_pointer()
                            .hover(|s| s.bg(t.colors.hover))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.close_overlay(&crate::app::CloseOverlay, window, cx)))
                            .child(icon("x", 16., t.colors.muted)),
                    ))
                    .child(div().text_size(px(13.)).text_color(t.colors.muted).child("Preview exactly what will be copied or saved. Identifying values are hidden unless you include them."))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_x(px(24.))
                            .gap_y(px(10.))
                            .child(div().flex().rounded(px(8.)).border_1().border_color(t.colors.hairline_strong).overflow_hidden().child(seg("fmt-text", "Readable text", ExportFormat::Text, cx)).child(seg("fmt-json", "JSON", ExportFormat::Json, cx)))
                            .child(checkbox),
                    ),
            )
            .child(
                div()
                    .id("export-preview")
                    .role(Role::Document)
                    .aria_label(format!("Report preview, {lines} lines"))
                    .flex_1()
                    .min_h(px(120.))
                    .mx(px(22.))
                    .p(px(14.))
                    .overflow_y_scroll()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(t.colors.hairline)
                    .bg(t.colors.surface)
                    .font_family(MONO_FONT)
                    .text_size(px(12.))
                    .line_height(px(17.))
                    .child(preview),
            )
            .child(
                div()
                    .px(px(22.))
                    .py(px(16.))
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(div().text_size(px(12.5)).text_color(t.colors.faint).child(pluralize(lines, "line", "lines")))
                    .child(
                        div()
                            .flex()
                            .gap(px(10.))
                            .child(self.button("export-save", "Save…", false, cx, |this, window, cx| this.save_export(window, cx)))
                            .child(self.button("export-copy", "Copy report", true, cx, move |this, _, cx| {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy_text.clone()));
                                this.show_toast("Report copied to the clipboard".into(), cx);
                            })),
                    ),
            );
        Some(self.scrim(cx).child(panel).into_any_element())
    }

    pub fn toast_view(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let t = cx.theme();
        let msg = self.toast.clone()?;
        Some(
            div()
                .absolute()
                .bottom(px(26.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    div()
                        .id("toast")
                        .role(Role::Status)
                        .aria_label(msg.clone())
                        .px(px(16.))
                        .py(px(10.))
                        .rounded(px(10.))
                        .bg(t.colors.nav)
                        .text_color(t.colors.nav_text)
                        .text_size(px(13.))
                        .shadow_lg()
                        .child(msg),
                )
                .into_any_element(),
        )
    }
}
