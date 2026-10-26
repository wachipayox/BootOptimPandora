//! Native startup presentation; frame capture is separate from normal launch policy.
mod audio;
mod geometry;
mod policy;
pub(crate) mod vector;

use crate::interface_config::InterfaceConfig;
use gpui::{prelude::*, *};
use policy::{INTRO, IntroKind, TRANSITION, intro_kind, may_reveal};
use std::time::Instant;

struct Session {
    existing_install: bool,
    consumed: bool,
    active: bool,
    process_started: Instant,
}
impl Global for Session {}

pub(crate) fn init(existing_install: bool, allow_startup: bool, process_started: Instant, cx: &mut App) {
    cx.set_global(Session {
        existing_install,
        consumed: !allow_startup,
        active: false,
        process_started,
    });
}
pub(crate) fn is_active(cx: &App) -> bool {
    cx.try_global::<Session>().is_some_and(|s| s.active)
}

pub(crate) struct Controller {
    opened: Instant,
    clock: Option<Instant>,
    kind: IntroKind,
    skipped: bool,
    ready_frames: u8,
    fade: Option<Instant>,
    first_frame: bool,
    audio: Option<audio::Audio>,
    audio_requested: bool,
    process_started: Instant,
    preload_reported: bool,
}
impl Controller {
    pub(crate) fn new(opened: Instant, cx: &mut App) -> Option<Self> {
        if std::env::var("WBOMBO_UI_CAPTURE_PAGE").is_ok_and(|page| page != "startup-live") {
            return None;
        }
        let session = cx.global_mut::<Session>();
        if session.consumed {
            return None;
        }
        session.consumed = true;
        session.active = true;
        let existing = session.existing_install;
        let process_started = session.process_started;
        let version = option_env!("PANDORA_RELEASE_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"));
        let kind = intro_kind(existing, InterfaceConfig::get(cx).startup_seen_version.as_deref(), version);
        let mut tracks: Vec<String> = crate::Assets::iter()
            .filter(|p| p.starts_with("splash-music/") && p.ends_with(".ogg"))
            .map(|p| p.to_string())
            .collect();
        tracks.sort();
        let selected = (kind != IntroKind::None)
            .then(|| {
                policy::choose_track(
                    &tracks,
                    InterfaceConfig::get(cx).startup_last_music.as_deref(),
                    rand::random::<u64>() as usize,
                )
            })
            .flatten();
        let audio = selected.and_then(|name| {
            crate::Assets::get(name).map(|asset| {
                InterfaceConfig::get_mut(cx).startup_last_music = Some(name.into());
                audio::Audio::prepare(asset.data.to_vec())
            })
        });
        InterfaceConfig::get_mut(cx).startup_seen_version = Some(version.into());
        Some(Self {
            opened,
            clock: None,
            kind,
            skipped: false,
            ready_frames: 0,
            fade: None,
            first_frame: true,
            audio,
            audio_requested: false,
            process_started,
            preload_reported: false,
        })
    }

    pub(crate) fn skip(&mut self) {
        self.skipped = true;
        if let Some(audio) = &self.audio {
            audio.fade();
        }
    }

    pub(crate) fn layer(&mut self, ready: bool, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
        let now = Instant::now();
        let elapsed = now.duration_since(self.opened);
        #[cfg(feature = "ui-capture")]
        let ready = ready
            && !(std::env::var("WBOMBO_UI_CAPTURE_PAGE").as_deref() == Ok("startup-live")
                && elapsed
                    < std::time::Duration::from_millis(
                        std::env::var("WBOMBO_UI_CAPTURE_STARTUP_DELAY_MS")
                            .ok()
                            .and_then(|value| value.parse::<u64>().ok())
                            .unwrap_or(0),
                    ));
        let first_paint_origin = self.first_frame.then_some((self.opened, self.process_started));
        if self.first_frame {
            self.first_frame = false;
        } else if self.clock.is_none() {
            if let Some(audio) = &self.audio {
                if !self.audio_requested {
                    audio.start();
                    self.audio_requested = true;
                }
                self.clock = audio.clock.lock().ok().and_then(|clock| *clock);
                if elapsed >= std::time::Duration::from_secs(1) && self.clock.is_none() {
                    // Device/decoder stalls must never hold the animation or UI.
                    self.audio = None;
                    self.clock = Some(now);
                }
            } else {
                self.clock = Some(now);
            }
        }
        self.ready_frames = if ready { self.ready_frames.saturating_add(1) } else { 0 };
        if self.ready_frames >= 2 && !self.preload_reported {
            self.preload_reported = true;
            log::info!(
                "startup-preload-ready pid={} native_window_to_ready_us={} main_to_ready_us={}",
                std::process::id(),
                elapsed.as_micros(),
                now.duration_since(self.process_started).as_micros()
            );
        }
        let animation_elapsed = self.clock.map(|clock| now.duration_since(clock)).unwrap_or_default();
        let intro_finished = self.kind == IntroKind::None || self.skipped || animation_elapsed >= INTRO;
        if animation_elapsed >= INTRO {
            if let Some(audio) = &self.audio {
                audio.fade();
            }
        }
        if self.fade.is_none() && intro_finished && may_reveal(elapsed, self.ready_frames >= 2, self.kind, self.skipped)
        {
            self.fade = Some(now);
            if !ready {
                crate::background::fade_late_completion(cx);
            }
            log::info!(
                "startup-home-ready pid={} window_open_to_ready_us={} capped={} main_to_reveal_us={}",
                std::process::id(),
                elapsed.as_micros(),
                !ready,
                now.duration_since(self.process_started).as_micros()
            );
        }
        let opacity = 1.
            - self
                .fade
                .map(|fade| now.duration_since(fade).as_secs_f32() / TRANSITION.as_secs_f32())
                .unwrap_or(0.);
        if opacity <= 0. {
            cx.global_mut::<Session>().active = false;
            return None;
        }
        window.request_animation_frame();
        let t = if self.kind == IntroKind::None || self.skipped || self.clock.is_none() {
            6.
        } else {
            animation_elapsed.as_secs_f64().min(6.)
        };
        let mut layer = div()
            .absolute()
            .inset_0()
            .size_full()
            .opacity(opacity)
            .child(vector::frame_timed(t, first_paint_origin));
        if self.kind != IntroKind::None && !self.skipped && self.fade.is_none() {
            layer = layer.child(
                div()
                    .absolute()
                    .left(px(24.))
                    .bottom(px(20.))
                    .font_family(crate::theme::SANS)
                    .text_size(px(12.))
                    .text_color(rgb(0xa9a9b2))
                    .child("Clic o Esc para saltar"),
            );
            if self.kind == IntroKind::Update {
                layer = layer.child(
                    div()
                        .absolute()
                        .right(px(24.))
                        .bottom(px(20.))
                        .font_family(crate::theme::SANS)
                        .text_size(px(12.))
                        .text_color(rgb(0xa9a9b2))
                        .child(format!(
                            "Actualizado a la versión {}",
                            option_env!("PANDORA_RELEASE_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
                        )),
                );
            }
        }
        Some(layer.into_any_element())
    }
}

#[cfg(feature = "ui-capture")]
pub(crate) fn capture_frame(window: &mut Window) -> Option<AnyElement> {
    let page = std::env::var("WBOMBO_UI_CAPTURE_PAGE").ok()?;
    let (route, time) = page.split_once(':').unwrap_or((&page, "6"));
    if !matches!(route, "startup-frame" | "startup-intro" | "startup-splash" | "startup-bench") {
        return None;
    }
    let t = time.parse::<f64>().ok()?.clamp(0., 6.);
    if route == "startup-bench" {
        window.request_animation_frame();
    }
    let mut frame = div().relative().size_full().child(vector::frame(t));
    if route == "startup-intro" {
        frame = frame.child(
            div()
                .absolute()
                .left(px(24.))
                .bottom(px(20.))
                .font_family(crate::theme::SANS)
                .text_size(px(12.))
                .text_color(rgb(0xa9a9b2))
                .child("Clic o Esc para saltar"),
        );
    }
    Some(frame.into_any_element())
}
