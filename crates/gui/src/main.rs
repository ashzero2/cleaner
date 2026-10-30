extern crate gpui_kit as gpui;

mod actions;
#[cfg(feature = "ui-capture")]
mod capture;
mod category;
mod explore;
mod format;
mod jobs;
mod panels;
mod recommendations;

mod search;
mod selection;
mod settings;
mod theme;
mod treemap;
mod ui;
mod workbench;

use std::borrow::Cow;

use gpui_kit::component::TitleBar;
use gpui_kit::{
    AppContext as _, AssetSource, Bounds, SharedString, WindowBounds, WindowOptions, point, px,
    size,
};

gpui_kit::assets::icon_assets!(
    ExtraIcons,
    [House, FolderPlus, Sparkles, Lock, Clock, Trash, X]
);

/// The kit's default assets plus the icons the window uses beyond them.
struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> gpui_kit::Result<Vec<SharedString>> {
        let mut paths = ExtraIcons.list(path)?;
        paths.extend(gpui_kit::assets::Assets.list(path)?);
        Ok(paths)
    }
}

fn main() {
    let loaded = settings::Settings::load();
    let jobs = match jobs::Jobs::new(jobs::default_cache_dir()) {
        Ok(jobs) => jobs,
        Err(error) => {
            eprintln!("Could not start the scan worker: {error}");
            std::process::exit(1);
        }
    };
    let root = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    gpui_kit::application().with_assets(Assets).run(move |cx| {
        gpui_kit::init(cx);
        theme::load_fonts(cx);
        theme::apply(loaded.settings.appearance, cx);
        actions::init(cx);
        let default = Bounds::centered(None, size(px(1280.), px(800.)), cx);
        let bounds = loaded
            .settings
            .window
            .map(|saved| {
                Bounds::new(
                    point(px(saved.x), px(saved.y)),
                    size(
                        px(saved.width.max(theme::MIN_WIDTH)),
                        px(saved.height.max(theme::MIN_HEIGHT)),
                    ),
                )
            })
            .filter(|bounds| {
                cx.displays().iter().any(|display| {
                    let screen = display.bounds();
                    bounds.origin.x >= screen.origin.x
                        && bounds.origin.y >= screen.origin.y
                        && bounds.right() <= screen.right()
                        && bounds.bottom() <= screen.bottom()
                })
            })
            .unwrap_or(default);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(theme::MIN_WIDTH), px(theme::MIN_HEIGHT))),
            titlebar: Some(gpui_kit::TitlebarOptions {
                title: Some("cl-rs".into()),
                traffic_light_position: Some(point(px(18.), px(21.))),
                ..TitleBar::title_bar_options()
            }),
            app_id: Some("dev.cl-rs.desktop".into()),
            ..Default::default()
        };
        if let Err(error) = gpui_kit::open_window(options, cx, |window, cx| {
            #[cfg(feature = "ui-capture")]
            capture::init(window, cx);
            cx.new(|cx| workbench::Workbench::new(loaded, jobs, root, window, cx))
        }) {
            eprintln!("Could not open cl-rs: {error:#}");
            cx.quit();
        }
        cx.activate(true);
    });
}
