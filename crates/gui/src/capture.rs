use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::{App, Window};

/// Captures this application's own rendered scene, not other windows or the desktop.
pub(crate) fn init(window: &Window, cx: &App) {
    let Some(path) = std::env::var_os("CLRS_GUI_CAPTURE") else {
        return;
    };
    let delay = std::env::var("CLRS_GUI_CAPTURE_DELAY")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(2);
    window
        .spawn(cx, async move |cx| {
            cx.background_executor()
                .timer(Duration::from_secs(delay))
                .await;
            let result = cx.update(|window, _| window.render_to_image());
            match result {
                Ok(Ok(image)) => {
                    let result = cx
                        .background_executor()
                        .spawn(async move { image.save(PathBuf::from(path)) })
                        .await;
                    if let Err(error) = result {
                        eprintln!("Could not save UI capture: {error}");
                    }
                }
                result => eprintln!("Could not render UI capture: {result:?}"),
            }
        })
        .detach();
}
