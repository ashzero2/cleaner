extern crate gpui_kit as gpui;

mod actions;
#[cfg(feature = "ui-capture")]
mod capture;
mod explore;
mod format;
mod jobs;
mod panels;
mod selection;
mod settings;
mod theme;
mod workbench;

use gpui_kit::{
    AppContext as _, Bounds, TitlebarOptions, WindowBounds, WindowOptions, point, px, size,
};

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
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
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
                titlebar: Some(TitlebarOptions {
                    title: Some("cl-rs".into()),
                    ..Default::default()
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
