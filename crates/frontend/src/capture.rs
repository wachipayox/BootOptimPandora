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

pub fn open_window_dialog(window: &mut Window, cx: &mut App) {
    let action = bridge::modal_action::ModalAction::default();
    action.set_finished_with_error("No se ha podido actualizar el launcher.".into());
    crate::modals::generic::show_modal(
        window,
        cx,
        "Actualizando".into(),
        "No se ha podido actualizar el launcher.".into(),
        action,
    );
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
                let request = std::fs::read(directory.join("request")).unwrap_or_default();
                let resize = serde_json::from_slice::<serde_json::Value>(&request).ok().and_then(|value| {
                    let pair = value.get("resize")?.as_array()?;
                    Some([pair.first()?.as_f64()? as f32, pair.get(1)?.as_f64()? as f32])
                });
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
                        // Arm before resize: retain the FIRST fresh draw, rather
                        // than a settled frame after asynchronous preparation.
                        if let Some([width, height]) = resize {
                            window.resize(gpui::size(gpui::px(width), gpui::px(height)));
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

/// Arms the very first native draw, before the window factory returns its root.
/// Ordinary capture routes still bypass startup; startup-live uses real policy.
pub fn startup_first_frame(window: &mut Window, _cx: &mut App) {
    if std::env::var("WBOMBO_UI_CAPTURE_STARTUP_FIRST_FRAME").as_deref() != Ok("1") {
        return;
    }
    let Some(directory) = std::env::var_os("WBOMBO_UI_CAPTURE_FRAMEBUFFER_DIR").map(PathBuf::from) else {
        return;
    };
    if let Err(error) = window.request_framebuffer_capture() {
        log::warn!("Could not arm first startup frame: {error}");
        return;
    }
    window.on_next_frame(move |window, _| {
        window.on_next_frame(move |window, cx| match window.capture_framebuffer() {
            Ok(image) => cx
                .background_executor()
                .spawn(async move {
                    if let Err(error) = image.save(directory.join("startup-first.png")) {
                        log::warn!("Could not save first startup frame: {error}");
                    }
                })
                .detach(),
            Err(error) => log::warn!("Could not read first startup frame: {error}"),
        });
        window.refresh();
    });
}
