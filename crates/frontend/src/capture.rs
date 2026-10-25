//! Optional native framebuffer evidence, paired with desktop captures by capture_ui.ps1.
//! This module and GPUI's readback support are absent from ordinary launcher builds.
use gpui::{App, Window};
use std::{path::PathBuf, time::Duration};

pub fn start(window: &mut Window, cx: &mut App) {
    let Some(directory) = std::env::var_os("WBOMBO_UI_CAPTURE_FRAMEBUFFER_DIR").map(PathBuf::from) else {
        return;
    };
    window
        .spawn(cx, async move |cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(100)).await;
                if !directory.join("request").is_file() {
                    continue;
                }
                let _ = std::fs::remove_file(directory.join("request"));
                // Copy a fresh framebuffer without replaying scene sprite IDs:
                // animated images can retire those between frame callbacks.
                let (send, receive) = tokio::sync::oneshot::channel();
                if cx
                    .update(|window, _| {
                        window.on_next_frame(move |window, _| {
                            let _ = send.send(window.capture_framebuffer());
                        });
                        window.refresh();
                    })
                    .is_err()
                {
                    break;
                }
                let Ok(result) = receive.await else { break };
                let directory = directory.clone();
                cx.background_executor()
                    .spawn(async move {
                        let result = result.and_then(|image| {
                            let pending = directory.join("frame.pending.png");
                            image.save(&pending)?;
                            std::fs::rename(pending, directory.join("frame.png"))?;
                            Ok(())
                        });
                        if let Err(error) = result {
                            log::warn!("Native framebuffer capture failed: {error}");
                            let _ = std::fs::write(directory.join("failed"), error.to_string());
                        }
                    })
                    .await;
            }
        })
        .detach();
}
