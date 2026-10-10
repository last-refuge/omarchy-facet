//! Application state and behaviour. Rendering lives in `shell.rs`, `pages.rs` and `overlays.rs`.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use facet_core::{Category, Component, Inventory, Report, Value};
use gpui::{
    App, ClipboardItem, Context, Entity, FocusHandle, Focusable, KeyBinding, ScrollHandle, SharedString, Subscription,
    WeakEntity, Window, actions, prelude::*,
};
use serde::{Deserialize, Serialize};

use crate::input::{Changed, TextInput};
use crate::theme::{Mode, Theme};

actions!(
    facet,
    [OpenSearch, OpenExport, Refresh, GoBack, ToggleTheme, CloseOverlay, MoveUp, MoveDown, Confirm, ToggleSources, ToggleIdentifiers, ToggleLive]
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
        KeyBinding::new("ctrl-shift-m", ToggleLive, None),
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

/// Collectors whose numbers change from second to second. A live tick re-reads only these; devices
/// appearing or disappearing is hotplug's job, which does a full read.
const LIVE_COLLECTORS: [&str; 5] = ["processor", "memory", "graphics", "power", "sensors"];
/// The graphics collector (GPU power state) changes rarely and costs the most after sensors, so it joins
/// only every fifth tick (ten seconds).
const GRAPHICS_EVERY: u64 = 5;
/// The same set without graphics, for most ticks.
const WITHOUT_GRAPHICS: [&str; 4] = ["processor", "memory", "power", "sensors"];
/// While Facet's window is not focused, only every fifth tick (ten seconds) refreshes.
const BACKGROUND_EVERY: u64 = 5;

/// Samples kept per live reading: 90 at one every two seconds is three minutes.
const HISTORY_CAP: usize = 90;

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Hotplug {
    Off,
    Active,
    Unavailable(String),
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
    /// Live monitoring on/off; absent means "use the default" (on for real hardware).
    live: Option<bool>,
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
    /// Focus targets inside the dialog, in Tab order: close, text, json, identifiers, save, copy.
    pub handles: Vec<FocusHandle>,
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
    staging: BTreeMap<usize, Vec<Component>>,
    staging_done: usize,
    /// How many collector results this refresh is waiting for.
    expected: usize,
    /// A refresh is in flight; live ticks must not supersede it.
    refreshing: bool,
    /// Whether Facet's window has focus; live updates slow down when it does not.
    window_active: bool,
    round_failures: Vec<String>,
    last_updated: Option<Instant>,
    /// Live monitoring re-reads every two seconds and keeps a bounded history of live numbers.
    pub live: bool,
    samples: HashMap<String, VecDeque<f64>>,
    live_task: Option<gpui::Task<()>>,
    pub hotplug: Hotplug,
    hotplug_task: Option<gpui::Task<()>>,
    /// True once the first load has finished; only later arrivals count as "new".
    settled: bool,
    source: Source,
    epoch: u64,
    /// Set each frame from the window width; row builders adapt to it.
    pub narrow: bool,
    pub viewport_w: f32,
}

impl Facet {
    pub fn new(source: Source, initial: Option<Page>, theme: Option<ThemePref>, live: Option<bool>, window: &mut Window, cx: &mut Context<Self>) -> Self {
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
            staging: BTreeMap::new(),
            staging_done: 0,
            expected: 0,
            refreshing: false,
            window_active: true,
            round_failures: Vec::new(),
            last_updated: None,
            live: false,
            samples: HashMap::new(),
            live_task: None,
            hotplug: Hotplug::Off,
            hotplug_task: None,
            settled: false,
            source,
            epoch: 0,
            narrow: false,
            viewport_w: 1240.0,
        };
        cx.observe_window_activation(window, |this, window, cx| {
            let was_active = this.window_active;
            this.window_active = window.is_window_active();
            // Coming back to the window: catch up right away instead of waiting for the next tick.
            if this.window_active && !was_active && this.live && this.settled && !this.is_loading() && !this.refreshing {
                this.load_collectors(cx, Some(&WITHOUT_GRAPHICS));
            }
        })
        .detach();
        this.apply_theme(window.appearance(), cx);
        cx.observe_window_appearance(window, |this, window, cx| {
            this.apply_theme(window.appearance(), cx);
            cx.notify();
        })
        .detach();
        // Live monitoring is on by default for real hardware (demo data stays still unless asked).
        let is_live_source = matches!(this.source, Source::Live);
        let want_live = live.unwrap_or(is_live_source && saved.live.unwrap_or(true));
        this.start_load(cx);
        if is_live_source {
            this.start_hotplug(cx);
        }
        if want_live {
            this.set_live(true, cx);
        }
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
        let saved = Saved { page: Some(self.page.key()), theme: Some(self.theme_pref), live: Some(self.live) };
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

    /// A full read of every collector (startup, Ctrl+R, hotplug).
    pub fn start_load(&mut self, cx: &mut Context<Self>) {
        self.load_collectors(cx, None);
    }

    /// Read the collectors named in `only`, or all of them. Before the first load finishes everything is read.
    fn load_collectors(&mut self, cx: &mut Context<Self>, only: Option<&[&str]>) {
        self.epoch += 1;
        let epoch = self.epoch;
        self.staging.clear();
        self.staging_done = 0;
        self.round_failures.clear();
        match &self.source {
            Source::Fixture(inv) => {
                let inv = inv.clone();
                if self.settled {
                    self.commit_inventory(inv, cx);
                    self.last_updated = Some(Instant::now());
                } else {
                    self.inv = inv;
                }
                self.settled = true;
                self.load = Load::Ready;
            }
            Source::StuckLoading => self.load = Load::Loading { done: 2, total: 10 },
            Source::Broken(msg) => self.load = Load::Failed(msg.clone()),
            Source::Live => {
                let total = facet_collectors::COLLECTORS.len();
                if !self.settled {
                    // First load is progressive: collectors fill in as they finish.
                    self.load = Load::Loading { done: 0, total };
                    self.parts.clear();
                    self.failures.clear();
                }
                let only = if self.settled { only } else { None };
                let run: Vec<usize> = facet_collectors::COLLECTORS
                    .iter()
                    .enumerate()
                    .filter(|(_, (n, _))| only.is_none_or(|o| o.contains(n)))
                    .map(|(i, _)| i)
                    .collect();
                self.expected = run.len();
                self.refreshing = self.settled;
                for (i, (name, collect)) in facet_collectors::COLLECTORS.iter().enumerate().filter(|(i, _)| run.contains(i)) {
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

    fn concat_parts(parts: &BTreeMap<usize, Vec<Component>>) -> Inventory {
        Inventory { components: parts.values().flatten().cloned().collect() }
    }

    fn part_done(&mut self, epoch: u64, index: usize, name: &str, comps: Option<Vec<Component>>, cx: &mut Context<Self>) {
        if epoch != self.epoch {
            return; // a newer refresh superseded this one
        }
        let total = facet_collectors::COLLECTORS.len();
        if !self.settled {
            match comps {
                Some(c) => {
                    self.parts.insert(index, c);
                }
                None => self.failures.push(format!("The {name} collector failed unexpectedly.")),
            }
            let done = self.parts.len() + self.failures.len();
            self.inv = Self::concat_parts(&self.parts);
            self.load = if done >= total {
                crate::timing("all-collectors-done-ms");
                self.settled = true;
                self.last_updated = Some(Instant::now());
                if self.parts.is_empty() { Load::Failed("No hardware collector produced data.".into()) } else { Load::Ready }
            } else {
                Load::Loading { done, total }
            };
            cx.notify();
            return;
        }
        // Refreshes are staged and swapped in atomically, so nobody sees a half-updated machine and a
        // collector that fails keeps its last good reading (flagged) instead of vanishing or going stale silently.
        match comps {
            Some(c) => {
                self.staging.insert(index, c);
            }
            None => {
                if let Some(old) = self.parts.get(&index) {
                    self.staging.insert(index, old.clone());
                }
                self.round_failures.push(format!("The {name} collector failed; showing its last good reading."));
            }
        }
        self.staging_done += 1;
        if self.staging_done >= self.expected {
            // Merge: a live tick refreshes only some collectors and keeps the others' parts.
            for (i, part) in std::mem::take(&mut self.staging) {
                self.parts.insert(i, part);
            }
            self.refreshing = false;
            self.failures = std::mem::take(&mut self.round_failures);
            let inv = Self::concat_parts(&self.parts);
            self.commit_inventory(inv, cx);
            self.last_updated = Some(Instant::now());
            self.load = Load::Ready;
            cx.notify();
        }
    }

    /// Swap in a refreshed inventory, noting what appeared and disappeared.
    fn commit_inventory(&mut self, inv: Inventory, cx: &mut Context<Self>) {
        let old: HashMap<&str, &str> = self.inv.components.iter().map(|c| (c.id.as_str(), c.title.as_str())).collect();
        let new: HashMap<&str, &str> = inv.components.iter().map(|c| (c.id.as_str(), c.title.as_str())).collect();
        let mut added: Vec<&str> = inv.components.iter().filter(|c| !old.contains_key(c.id.as_str())).map(|c| c.title.as_str()).collect();
        let mut removed: Vec<&str> = self.inv.components.iter().filter(|c| !new.contains_key(c.id.as_str())).map(|c| c.title.as_str()).collect();
        added.truncate(2);
        removed.truncate(2);
        let fresh: Vec<String> = inv.components.iter().filter(|c| !old.contains_key(c.id.as_str())).map(|c| c.id.clone()).collect();
        let mut msg = Vec::new();
        if !added.is_empty() {
            msg.push(format!("New hardware: {}", added.join(", ")));
        }
        if !removed.is_empty() {
            msg.push(format!("Removed: {}", removed.join(", ")));
        }
        self.inv = inv;
        if !fresh.is_empty() {
            self.new_ids.extend(fresh);
            self.schedule_clear_new(cx);
        }
        if self.live {
            self.record_history();
        }
        if !msg.is_empty() {
            self.show_toast(msg.join("  ·  "), cx);
        }
    }

    // ---- live monitoring and history ------------------------------------------------------

    fn record_history(&mut self) {
        let mut keep = HashSet::new();
        for c in &self.inv.components {
            for sp in c.all_specs().filter(|sp| sp.live) {
                if let Some(v) = sp.value.value().and_then(Value::as_f64) {
                    let key = format!("{}|{}", c.id, sp.label);
                    let h = self.samples.entry(key.clone()).or_default();
                    h.push_back(v);
                    while h.len() > HISTORY_CAP {
                        h.pop_front();
                    }
                    keep.insert(key);
                }
            }
        }
        // History for devices that are gone must not linger or grow.
        self.samples.retain(|k, _| keep.contains(k));
    }

    pub fn history_for(&self, owner: &str, label: &str) -> Option<&VecDeque<f64>> {
        self.samples.get(&format!("{owner}|{label}")).filter(|h| h.len() >= 3)
    }

    pub fn toggle_live(&mut self, _: &ToggleLive, _: &mut Window, cx: &mut Context<Self>) {
        self.set_live_pref(!self.live, cx);
    }

    pub fn set_live(&mut self, on: bool, cx: &mut Context<Self>) {
        self.live = on;
        self.live_task = None;
        if on {
            self.record_history();
            self.live_task = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
                let mut tick: u64 = 0;
                loop {
                    cx.background_executor().timer(Duration::from_secs(2)).await;
                    let alive = this.update(cx, |this, cx| {
                        tick += 1;
                        if this.settled && !this.is_loading() && !this.refreshing {
                            if !this.window_active && !tick.is_multiple_of(BACKGROUND_EVERY) {
                                return;
                            }
                            let set: &[&str] = if tick.is_multiple_of(GRAPHICS_EVERY) { &LIVE_COLLECTORS } else { &WITHOUT_GRAPHICS };
                            this.load_collectors(cx, Some(set));
                        }
                    });
                    if alive.is_err() {
                        break;
                    }
                }
            }));
        } else {
            self.samples.clear();
        }
        cx.notify();
    }

    /// A user's choice: apply it and remember it.
    pub fn set_live_pref(&mut self, on: bool, cx: &mut Context<Self>) {
        self.set_live(on, cx);
        self.save();
    }

    /// Seconds since the last completed read, for the "updated" label.
    pub fn age_secs(&self) -> Option<u64> {
        self.last_updated.map(|t| t.elapsed().as_secs())
    }

    // ---- hotplug --------------------------------------------------------------------------

    pub fn start_hotplug(&mut self, cx: &mut Context<Self>) {
        use futures::StreamExt;
        let (tx, mut rx) = futures::channel::mpsc::unbounded::<Result<(), String>>();
        let spawned = std::thread::Builder::new().name("facet-hotplug".into()).spawn(move || {
            let events = tx.clone();
            let result = facet_collectors::hotplug::watch(move |_| {
                let _ = events.unbounded_send(Ok(()));
            });
            if let Err(e) = result {
                let _ = tx.unbounded_send(Err(e.to_string()));
            }
        });
        if let Err(e) = spawned {
            self.hotplug = Hotplug::Unavailable(e.to_string());
            return;
        }
        self.hotplug = Hotplug::Active;
        self.hotplug_task = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
            while let Some(msg) = rx.next().await {
                if let Err(e) = msg {
                    this.update(cx, |this, cx| {
                        this.hotplug = Hotplug::Unavailable(e);
                        cx.notify();
                    })
                    .ok();
                    break;
                }
                // Debounce a burst of events (one plug produces many) into a single re-read.
                cx.background_executor().timer(Duration::from_millis(600)).await;
                while rx.try_recv().is_ok() {}
                let alive = this.update(cx, |this, cx| {
                    if this.settled && !this.is_loading() {
                        this.start_load(cx);
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        }));
    }

    /// Design-review aid: after a few seconds, "plug in" a USB stick and "unplug" a keyboard.
    pub fn demo_hotplug(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            cx.background_executor().timer(Duration::from_secs(3)).await;
            this.update(cx, |this, cx| {
                if let Source::Fixture(inv) = &mut this.source {
                    inv.components.push(facet_core::fixtures::usb_stick());
                    inv.components.retain(|c| c.id != "usb:3-1.3");
                }
                this.start_load(cx);
            })
            .ok();
        })
        .detach();
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
        let input = cx.new(|cx| TextInput::new("Find a device or specification…", accent, cx));
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

    /// Tab inside a modal: cycle within the dialog (export) or hold still (palette, which uses arrows).
    /// Returns true when the key was handled.
    pub fn trap_tab(&mut self, shift: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if let Some(e) = &self.export {
            let n = e.handles.len();
            let next = match e.handles.iter().position(|h| h.is_focused(window)) {
                Some(i) if shift => (i + n - 1) % n,
                Some(i) => (i + 1) % n,
                None => 0,
            };
            window.focus(&e.handles[next], cx);
            return true;
        }
        self.palette.is_some()
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
        let handles: Vec<FocusHandle> = (0..6).map(|_| cx.focus_handle()).collect();
        window.focus(&handles[1], cx);
        self.export = Some(ExportState { format: ExportFormat::Text, include_identifiers: false, handles });
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
