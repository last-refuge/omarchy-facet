//! Application state and behaviour. Rendering lives in `shell.rs`, `pages.rs` and `overlays.rs`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use facet_core::{Category, Component, Inventory, Report};
use gpui::{
    App, ClipboardItem, Context, Entity, FocusHandle, Focusable, KeyBinding, ScrollHandle, SharedString, Subscription,
    WeakEntity, Window, actions, prelude::*,
};
use serde::{Deserialize, Serialize};

use crate::input::{Changed, TextInput};
use crate::theme::{Mode, Theme};

actions!(
    facet,
    [OpenSearch, OpenExport, Refresh, GoBack, ToggleTheme, CloseOverlay, MoveUp, MoveDown, Confirm, ToggleSources, ToggleIdentifiers]
);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-k", OpenSearch, None),
        KeyBinding::new("ctrl-e", OpenExport, None),
        KeyBinding::new("ctrl-r", Refresh, None),
        KeyBinding::new("alt-left", GoBack, None),
        KeyBinding::new("ctrl-shift-l", ToggleTheme, None),
        KeyBinding::new("ctrl-shift-s", ToggleSources, None),
        KeyBinding::new("ctrl-shift-i", ToggleIdentifiers, None),
        KeyBinding::new("escape", CloseOverlay, Some("Overlay")),
        KeyBinding::new("up", MoveUp, Some("Palette")),
        KeyBinding::new("down", MoveDown, Some("Palette")),
        KeyBinding::new("enter", Confirm, Some("Palette")),
    ]);
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Page {
    Overview,
    Category(Category),
    Component(String),
    Connections,
    Preferences,
}

impl Page {
    pub fn key(&self) -> String {
        match self {
            Page::Overview => "overview".into(),
            Page::Category(c) => format!("category:{c:?}").to_lowercase(),
            Page::Component(id) => format!("component:{id}"),
            Page::Connections => "connections".into(),
            Page::Preferences => "preferences".into(),
        }
    }

    pub fn parse(s: &str) -> Option<Page> {
        match s {
            "overview" => Some(Page::Overview),
            "connections" => Some(Page::Connections),
            "preferences" => Some(Page::Preferences),
            _ => {
                if let Some(rest) = s.strip_prefix("category:") {
                    Category::ALL.into_iter().find(|c| format!("{c:?}").to_lowercase() == rest).map(Page::Category)
                } else {
                    s.strip_prefix("component:").map(|id| Page::Component(id.to_owned()))
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Load {
    Loading { done: usize, total: usize },
    Ready,
    Failed(String),
}

pub enum Source {
    Live,
    Fixture(Inventory),
    /// Demo states for design review: never finishes loading / always fails.
    StuckLoading,
    Broken(String),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePref {
    System,
    Light,
    Dark,
}

#[derive(Serialize, Deserialize, Default)]
struct Saved {
    page: Option<String>,
    theme: Option<ThemePref>,
}

fn state_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(base.join("facet/state.json"))
}

fn load_saved() -> Saved {
    state_path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Text,
    Json,
}

pub struct ExportState {
    pub format: ExportFormat,
    pub include_identifiers: bool,
}

pub struct PaletteState {
    pub input: Entity<TextInput>,
    pub selected: usize,
    pub results: Vec<String>,
    _sub: Subscription,
}

pub struct Facet {
    pub inv: Inventory,
    parts: BTreeMap<usize, Vec<Component>>,
    pub failures: Vec<String>,
    pub load: Load,
    pub page: Page,
    history: Vec<Page>,
    pub theme_pref: ThemePref,
    pub palette: Option<PaletteState>,
    pub export: Option<ExportState>,
    pub toast: Option<SharedString>,
    toast_epoch: u64,
    pub show_sources: bool,
    /// Serial numbers, UUIDs and MAC addresses stay hidden until the user asks to see them.
    pub show_identifiers: bool,
    pub tech_open: HashSet<String>,
    scrolls: HashMap<String, ScrollHandle>,
    pub focus: FocusHandle,
    pub new_ids: HashSet<String>,
    seen: HashSet<String>,
    /// True once the first load has finished; only later arrivals count as "new".
    settled: bool,
    source: Source,
    epoch: u64,
    /// Set each frame from the window width; row builders adapt to it.
    pub narrow: bool,
}

impl Facet {
    pub fn new(source: Source, initial: Option<Page>, theme: Option<ThemePref>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let saved = load_saved();
        let page = initial.or_else(|| saved.page.as_deref().and_then(Page::parse)).unwrap_or(Page::Overview);
        let theme_pref = theme.or(saved.theme).unwrap_or(ThemePref::System);
        let mut this = Facet {
            inv: Inventory::default(),
            parts: BTreeMap::new(),
            failures: Vec::new(),
            load: Load::Loading { done: 0, total: 0 },
            page,
            history: Vec::new(),
            theme_pref,
            palette: None,
            export: None,
            toast: None,
            toast_epoch: 0,
            show_sources: false,
            show_identifiers: false,
            tech_open: HashSet::new(),
            scrolls: HashMap::new(),
            focus: cx.focus_handle(),
            new_ids: HashSet::new(),
            seen: HashSet::new(),
            settled: false,
            source,
            epoch: 0,
            narrow: false,
        };
        this.apply_theme(window.appearance(), cx);
        cx.observe_window_appearance(window, |this, window, cx| {
            this.apply_theme(window.appearance(), cx);
            cx.notify();
        })
        .detach();
        this.start_load(cx);
        window.focus(&this.focus, cx);
        this
    }

    // ---- theme ----------------------------------------------------------------------------

    fn apply_theme(&mut self, appearance: gpui::WindowAppearance, cx: &mut Context<Self>) {
        let mode = match self.theme_pref {
            ThemePref::Light => Mode::Light,
            ThemePref::Dark => Mode::Dark,
            ThemePref::System => match appearance {
                gpui::WindowAppearance::Dark | gpui::WindowAppearance::VibrantDark => Mode::Dark,
                _ => Mode::Light,
            },
        };
        cx.set_global(Theme::new(mode));
    }

    pub fn set_theme_pref(&mut self, pref: ThemePref, window: &mut Window, cx: &mut Context<Self>) {
        self.theme_pref = pref;
        self.apply_theme(window.appearance(), cx);
        self.save();
        cx.notify();
    }

    pub fn toggle_theme(&mut self, _: &ToggleTheme, window: &mut Window, cx: &mut Context<Self>) {
        let dark_now = cx.global::<Theme>().mode == Mode::Dark;
        self.set_theme_pref(if dark_now { ThemePref::Light } else { ThemePref::Dark }, window, cx);
    }

    // ---- persistence ----------------------------------------------------------------------

    fn save(&self) {
        let Some(path) = state_path() else { return };
        // Only remember pages that survive a restart (a component page for a vanished device is fine too).
        let saved = Saved { page: Some(self.page.key()), theme: Some(self.theme_pref) };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string(&saved) {
            let _ = std::fs::write(path, json);
        }
    }

    // ---- navigation -----------------------------------------------------------------------

    pub fn go(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        if self.page != page {
            let prev = std::mem::replace(&mut self.page, page);
            self.history.push(prev);
            self.history.truncate(64);
        }
        self.palette = None;
        self.export = None;
        self.save();
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub fn go_back(&mut self, _: &GoBack, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(prev) = self.history.pop() {
            self.page = prev;
            self.save();
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    pub fn can_go_back(&self) -> bool {
        !self.history.is_empty()
    }

    /// One scroll handle per page so position survives refresh and navigation.
    pub fn scroll_for(&mut self, key: &str) -> ScrollHandle {
        self.scrolls.entry(key.to_owned()).or_default().clone()
    }

    // ---- loading --------------------------------------------------------------------------

    pub fn refresh(&mut self, _: &Refresh, _window: &mut Window, cx: &mut Context<Self>) {
        self.start_load(cx);
        cx.notify();
    }

    pub fn start_load(&mut self, cx: &mut Context<Self>) {
        self.epoch += 1;
        let epoch = self.epoch;
        self.failures.clear();
        match &self.source {
            Source::Fixture(inv) => {
                let inv = inv.clone();
                self.set_inventory(inv, cx);
                self.settled = true;
                self.load = Load::Ready;
            }
            Source::StuckLoading => self.load = Load::Loading { done: 2, total: 10 },
            Source::Broken(msg) => self.load = Load::Failed(msg.clone()),
            Source::Live => {
                let total = facet_collectors::COLLECTORS.len();
                self.load = Load::Loading { done: 0, total };
                for (i, (name, collect)) in facet_collectors::COLLECTORS.iter().enumerate() {
                    let collect = *collect;
                    let name = *name;
                    cx.spawn(async move |this: WeakEntity<Self>, cx| {
                        let result = cx.background_spawn(async move { std::panic::catch_unwind(collect) }).await;
                        this.update(cx, |this, cx| this.part_done(epoch, i, name, result.ok(), cx)).ok();
                    })
                    .detach();
                }
            }
        }
    }

    fn part_done(&mut self, epoch: u64, index: usize, name: &str, comps: Option<Vec<Component>>, cx: &mut Context<Self>) {
        if epoch != self.epoch {
            return; // a newer refresh superseded this one
        }
        match comps {
            Some(c) => {
                self.parts.insert(index, c);
            }
            None => {
                self.parts.remove(&index);
                self.failures.push(format!("The {name} collector failed unexpectedly."));
            }
        }
        let total = facet_collectors::COLLECTORS.len();
        let done = self.parts.len() + self.failures.len();
        let inv = Inventory { components: self.parts.values().flatten().cloned().collect() };
        self.set_inventory(inv, cx);
        self.load = if done >= total {
            self.settled = true;
            if self.parts.is_empty() { Load::Failed("No hardware collector produced data.".into()) } else { Load::Ready }
        } else {
            Load::Loading { done, total }
        };
        cx.notify();
    }

    fn set_inventory(&mut self, inv: Inventory, cx: &mut Context<Self>) {
        let first = !self.settled;
        let ids: HashSet<String> = inv.components.iter().map(|c| c.id.clone()).collect();
        if !first {
            let fresh: HashSet<String> = ids.difference(&self.seen).cloned().collect();
            if !fresh.is_empty() {
                self.new_ids.extend(fresh);
                self.schedule_clear_new(cx);
            }
        }
        self.seen.extend(ids);
        self.inv = inv;
    }

    fn schedule_clear_new(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            cx.background_executor().timer(Duration::from_secs(6)).await;
            this.update(cx, |this, cx| {
                this.new_ids.clear();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn is_loading(&self) -> bool {
        matches!(self.load, Load::Loading { .. })
    }

    // ---- copy / toast ---------------------------------------------------------------------

    pub fn copy(&mut self, text: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
        let shown: String = if text.chars().count() > 48 { format!("{}…", text.chars().take(47).collect::<String>()) } else { text };
        self.show_toast(format!("Copied {shown}"), cx);
    }

    pub fn show_toast(&mut self, msg: String, cx: &mut Context<Self>) {
        self.toast = Some(msg.into());
        self.toast_epoch += 1;
        let epoch = self.toast_epoch;
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            cx.background_executor().timer(Duration::from_millis(2200)).await;
            this.update(cx, |this, cx| {
                if this.toast_epoch == epoch {
                    this.toast = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub fn toggle_identifiers(&mut self, _: &ToggleIdentifiers, _: &mut Window, cx: &mut Context<Self>) {
        self.show_identifiers = !self.show_identifiers;
        cx.notify();
    }

    /// Text for a spec in the UI: identifiers are hidden unless the user chose to show them.
    pub fn spec_text(&self, s: &facet_core::Spec) -> (String, bool) {
        if s.identifier && !self.show_identifiers && s.is_available() {
            ("Hidden".to_owned(), true)
        } else {
            (s.display(), crate::widgets::is_absent(s))
        }
    }

    pub fn toggle_sources(&mut self, _: &ToggleSources, _: &mut Window, cx: &mut Context<Self>) {
        self.show_sources = !self.show_sources;
        cx.notify();
    }

    // ---- palette --------------------------------------------------------------------------

    pub fn open_search(&mut self, _: &OpenSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.export = None;
        let accent = cx.global::<Theme>().colors.accent_text;
        let input = cx.new(|cx| TextInput::new("Search hardware, models, properties…", accent, cx));
        let sub = cx.subscribe(&input, |this, input, _: &Changed, cx| {
            let q = input.read(cx).text().to_owned();
            this.update_results(&q);
            cx.notify();
        });
        window.focus(&input.focus_handle(cx), cx);
        self.palette = Some(PaletteState { input, selected: 0, results: Vec::new(), _sub: sub });
        self.update_results("");
        cx.notify();
    }

    /// Open the palette pre-filled (used by `--overlay search --query ...` for design review).
    pub fn open_search_with(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.open_search(&OpenSearch, window, cx);
        if let Some(p) = &self.palette {
            let input = p.input.clone();
            input.update(cx, |i, cx| i.set_text(query, cx));
        }
    }

    fn update_results(&mut self, query: &str) {
        let ids: Vec<String> = self.inv.search(query).into_iter().take(40).map(|c| c.id.clone()).collect();
        if let Some(p) = &mut self.palette {
            p.results = ids;
            p.selected = 0;
        }
    }

    pub fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(p) = &mut self.palette {
            if p.results.is_empty() {
                return;
            }
            let n = p.results.len() as isize;
            p.selected = (p.selected as isize + delta).rem_euclid(n) as usize;
            cx.notify();
        }
    }

    pub fn move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(-1, cx);
    }

    pub fn move_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(1, cx);
    }

    pub fn confirm(&mut self, _: &Confirm, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.palette.as_ref().and_then(|p| p.results.get(p.selected).cloned());
        if let Some(id) = target {
            self.go(Page::Component(id), window, cx);
        }
    }

    pub fn close_overlay(&mut self, _: &CloseOverlay, window: &mut Window, cx: &mut Context<Self>) {
        self.palette = None;
        self.export = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    // ---- export ---------------------------------------------------------------------------

    pub fn open_export(&mut self, _: &OpenExport, window: &mut Window, cx: &mut Context<Self>) {
        self.palette = None;
        self.export = Some(ExportState { format: ExportFormat::Text, include_identifiers: false });
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub fn export_text(&self) -> String {
        let Some(e) = &self.export else { return String::new() };
        let report = Report::new(self.inv.clone(), e.include_identifiers);
        match e.format {
            ExportFormat::Text => report.to_text(),
            ExportFormat::Json => serde_json::to_string_pretty(&report).unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}")),
        }
    }

    pub fn save_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(e) = &self.export else { return };
        let ext = if e.format == ExportFormat::Json { "json" } else { "txt" };
        let contents = self.export_text();
        let dir = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
        let receiver = cx.prompt_for_new_path(&dir, Some(&format!("facet-report.{ext}")));
        let _ = window;
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let Ok(Ok(Some(path))) = receiver.await else { return };
            let result = std::fs::write(&path, contents);
            this.update(cx, |this, cx| match result {
                Ok(()) => this.show_toast(format!("Saved {}", path.display()), cx),
                Err(e) => this.show_toast(format!("Could not save: {e}"), cx),
            })
            .ok();
        })
        .detach();
    }

    // ---- derived data ---------------------------------------------------------------------

    pub fn count(&self, c: Category) -> usize {
        self.inv.in_category(c).count()
    }
}
