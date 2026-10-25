//! Optional native framebuffer evidence, paired with desktop captures by capture_ui.ps1.
//! This module and GPUI's readback support are absent from ordinary launcher builds.
use gpui::{App, Window};
use std::{path::PathBuf, time::Duration};

/// Creates an owned local UI fixture through the ordinary backend creation
/// message. It needs no Distribution credentials or published test profile.
pub fn prepare_local_fixture(data: &crate::entity::DataEntities, cx: &App) {
    let Ok(name) = std::env::var("WBOMBO_UI_CAPTURE_LOCAL_INSTANCE") else {
        return;
    };
    if name != "agente-274-window-drag" || std::env::var_os("WBOMBO_UI_CAPTURE_PAGE").is_none() {
        log::warn!("Refusing unrecognized local capture fixture");
        return;
    }
    if data
        .instances
        .read(cx)
        .entries
        .values()
        .any(|instance| instance.read(cx).name.as_ref() == name)
    {
        return;
    }
    data.backend_handle.send(bridge::message::MessageToBackend::CreateInstance {
        name: name.as_str().into(),
        version: "1.21.1".into(),
        loader: schema::loader::Loader::Vanilla,
        icon: None,
    });
    log::info!("capture-local-fixture-requested pid={}", std::process::id());
}

pub fn local_fixture() -> bool {
    std::env::var("WBOMBO_UI_CAPTURE_LOCAL_INSTANCE").is_ok_and(|name| name == "agente-274-window-drag")
}

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
                        if let Err(error) = window.request_framebuffer_capture() {
                            let _ = send.send(Err(error));
                            return;
                        }
                        window.on_next_frame(move |window, _| {
                            // Frame callbacks run before drawing. The following
                            // callback retrieves the copy saved by this draw.
                            window.on_next_frame(move |window, _| {
                                let _ = send.send(window.capture_framebuffer().map(|image| {
                                    (image, window.capture_window_hit_regions())
                                }));
                            });
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
                        let result = result.and_then(|(image, regions)| {
                            let regions: Vec<_> = regions.into_iter().map(|(label, area, bounds, points)| {
                                serde_json::json!({
                                    "label": label, "area": area,
                                    "bounds": {"x": f32::from(bounds.origin.x), "y": f32::from(bounds.origin.y),
                                        "width": f32::from(bounds.size.width), "height": f32::from(bounds.size.height)},
                                    "points": points.into_iter().map(|p| [f32::from(p.x), f32::from(p.y)]).collect::<Vec<_>>()
                                })
                            }).collect();
                            std::fs::write(directory.join("regions.pending.json"), serde_json::to_vec(&regions)?)?;
                            std::fs::rename(directory.join("regions.pending.json"), directory.join("regions.json"))?;
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
