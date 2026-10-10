mod app;
mod assets;
mod gpu_guard;
mod input;
mod overlays;
mod pages;
mod shell;
mod theme;
mod widgets;

use gpui::{App, AppContext, Bounds, TitlebarOptions, WindowBackgroundAppearance, WindowBounds, WindowOptions, px, size};

use app::{Facet, Page, Source, ThemePref};

static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

const USAGE: &str = "usage: facet-app [--fixture laptop|desktop|vm|loading|error] [--page <overview|category:storage|component:<id>|connections|preferences>] [--theme system|light|dark] [--size WxH] [--overlay search|export] [--query <text>]";

struct Args {
    source: Source,
    page: Option<Page>,
    theme: Option<ThemePref>,
    size: Option<(f32, f32)>,
    overlay: Option<String>,
    query: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args { source: Source::Live, page: None, theme: None, size: None, overlay: None, query: None };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        let mut value = |name: &str| it.next().ok_or_else(|| format!("{name} needs a value"));
        match a.as_str() {
            "--fixture" => {
                let v = value("--fixture")?;
                args.source = match v.as_str() {
                    "loading" => Source::StuckLoading,
                    "error" => Source::Broken("The hardware collectors stopped unexpectedly.".into()),
                    name => Source::Fixture(facet_core::fixtures::by_name(name).ok_or_else(|| format!("unknown fixture {name:?}"))?),
                };
            }
            "--page" => {
                let v = value("--page")?;
                args.page = Some(Page::parse(&v).ok_or_else(|| format!("unknown page {v:?}"))?);
            }
            "--theme" => {
                args.theme = Some(match value("--theme")?.as_str() {
                    "system" => ThemePref::System,
                    "light" => ThemePref::Light,
                    "dark" => ThemePref::Dark,
                    other => return Err(format!("unknown theme {other:?}")),
                });
            }
            "--size" => {
                let v = value("--size")?;
                let (w, h) = v.split_once('x').ok_or("--size expects WxH")?;
                args.size = Some((w.parse().map_err(|_| "bad width")?, h.parse().map_err(|_| "bad height")?));
            }
            "--overlay" => args.overlay = Some(value("--overlay")?),
            "--query" => args.query = Some(value("--query")?),
            "--help" | "-h" => return Err(String::new()),
            other => return Err(format!("unknown option {other:?}")),
        }
    }
    Ok(args)
}

fn main() {
    START.get_or_init(std::time::Instant::now);
    let args = match parse_args() {
        Ok(a) => a,
        Err(msg) => {
            if !msg.is_empty() {
                eprintln!("facet-app: {msg}");
            }
            eprintln!("{USAGE}");
            std::process::exit(if msg.is_empty() { 0 } else { 2 });
        }
    };
    // Must run before any thread exists and before GPUI touches Vulkan.
    gpu_guard::apply();
    env_logger::init();

    gpui_platform::application().with_assets(assets::Assets).run(move |cx: &mut App| {
        if let Err(e) = assets::load_fonts(cx) {
            eprintln!("facet-app: could not load bundled fonts: {e}");
        }
        input::TextInput::bind_keys(cx);
        app::bind_keys(cx);

        let (w, h) = args.size.unwrap_or((1240., 800.));
        let bounds = Bounds::centered(None, size(px(w), px(h)), cx);
        let Args { source, page, theme, overlay, query, .. } = args;
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_background: WindowBackgroundAppearance::Opaque,
                app_id: Some("dev.facet.Facet".into()),
                titlebar: Some(TitlebarOptions { title: Some("Facet".into()), ..Default::default() }),
                window_min_size: Some(size(px(360.), px(420.))),
                ..Default::default()
            },
            move |window, cx| {
                if let (Some(_), Some(t)) = (std::env::var_os("FACET_TIMING"), START.get()) {
                    eprintln!("window-created-ms {}", t.elapsed().as_millis());
                }
                let view = cx.new(|cx| Facet::new(source, page, theme, window, cx));
                view.update(cx, |this, cx| match overlay.as_deref() {
                    Some("search") => this.open_search_with(query.as_deref().unwrap_or(""), window, cx),
                    Some("export") => this.open_export(&app::OpenExport, window, cx),
                    _ => {}
                });
                view
            },
        )
        .expect("open window");
        cx.activate(true);
    });
}
