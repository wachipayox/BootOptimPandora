//! Startup decisions are independent of the renderer and of network availability.
use std::time::Duration;

pub(super) const INTRO: Duration = Duration::from_secs(6);
pub(super) const PRELOAD_LIMIT: Duration = Duration::from_secs(8);
pub(super) const TRANSITION: Duration = Duration::from_millis(300);
// A normal launch should finish its fade within two seconds of the window
// opening, even when a local asset is unusually slow or unavailable.
pub(super) const NORMAL_PRELOAD_LIMIT: Duration = Duration::from_millis(1600);
pub(super) const AUDIO_FADE: Duration = Duration::from_millis(150);
pub(super) const AUDIO_TAIL: Duration = Duration::from_millis(400);

pub(super) fn initial_size(scale: f32, available: [f32; 2], minimum: [f32; 2]) -> [f32; 2] {
    let scale = if scale.is_finite() && scale > 0. { scale } else { 1. };
    let (mut width, mut height) = (1600. / scale, 900. / scale);
    let grow = (minimum[0] / width).max(minimum[1] / height).max(1.);
    width *= grow;
    height *= grow;
    let fit = (available[0] / width).min(available[1] / height).min(1.);
    [
        (width * fit).min(available[0]).max(minimum[0]),
        (height * fit).min(available[1]).max(minimum[1]),
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum IntroKind {
    FirstInstall,
    Update,
    None,
}

pub(super) fn preload_limit(intro: IntroKind) -> Duration {
    if intro == IntroKind::None {
        NORMAL_PRELOAD_LIMIT
    } else {
        PRELOAD_LIMIT
    }
}

pub(super) fn audio_fade_due(animation_elapsed: Duration, elapsed: Duration, limit: Duration) -> bool {
    animation_elapsed >= INTRO + AUDIO_TAIL - AUDIO_FADE || elapsed >= limit
}

/// `OnUpdate` (the default) also covers the first launch on this device: no
/// version has been recorded yet. `Always` replays the intro on every launch but
/// only labels it as an update when the version actually changed.
pub(super) fn intro_kind(
    mode: super::IntroMode,
    existing_install: bool,
    last_version: Option<&str>,
    current: &str,
) -> IntroKind {
    let changed = match last_version {
        Some(last) if last == current => None,
        Some(_) => Some(IntroKind::Update),
        None if existing_install => Some(IntroKind::Update),
        None => Some(IntroKind::FirstInstall),
    };
    match mode {
        super::IntroMode::Never => IntroKind::None,
        super::IntroMode::OnUpdate => changed.unwrap_or(IntroKind::None),
        super::IntroMode::Always => changed.unwrap_or(IntroKind::FirstInstall),
    }
}

/// Only Esc or Space skip the intro; mouse clicks do not.
pub(crate) fn skips_intro(key: &str) -> bool {
    matches!(key, "escape" | "space")
}

pub(super) fn may_reveal(
    elapsed: Duration,
    animation_elapsed: Duration,
    ready: bool,
    intro: IntroKind,
    skipped: bool,
) -> bool {
    elapsed >= preload_limit(intro) || (ready && (intro == IntroKind::None || skipped || animation_elapsed >= INTRO))
}

/// Uniform selection over all eligible files, excluding the last track whenever
/// there is an alternative. No rejection loop, including one-track libraries.
pub(super) fn choose_track<'a>(tracks: &'a [String], last: Option<&str>, random: usize) -> Option<&'a str> {
    let eligible = tracks.iter().filter(|name| tracks.len() == 1 || Some(name.as_str()) != last);
    let count = eligible.clone().count();
    (count != 0).then(|| eligible.clone().nth(random % count).expect("eligible index").as_str())
}

#[cfg(test)]
mod tests {
    use super::super::IntroMode;
    use super::{IntroKind, audio_fade_due, choose_track, initial_size, intro_kind, may_reveal, skips_intro};
    use std::time::Duration;
    #[test]
    fn recommended_client_size_uses_physical_pixels_and_keeps_the_existing_minimum() {
        assert_eq!(initial_size(1., [1920., 1080.], [960., 600.]), [1600., 900.]);
        let hidpi = initial_size(1.25, [2048., 1104.], [960., 600.]);
        assert_eq!(hidpi, [1280., 720.]);
        assert_eq!(initial_size(1., [960., 600.], [960., 600.]), [960., 600.]);
    }

    #[test]
    fn intro_follows_mode_and_version() {
        assert_eq!(intro_kind(IntroMode::OnUpdate, false, None, "1"), IntroKind::FirstInstall);
        assert_eq!(intro_kind(IntroMode::OnUpdate, true, None, "1"), IntroKind::Update);
        assert_eq!(intro_kind(IntroMode::OnUpdate, true, Some("0"), "1"), IntroKind::Update);
        assert_eq!(intro_kind(IntroMode::OnUpdate, true, Some("1"), "1"), IntroKind::None);
        assert_eq!(intro_kind(IntroMode::Always, true, Some("1"), "1"), IntroKind::FirstInstall);
        assert_eq!(intro_kind(IntroMode::Always, true, Some("0"), "1"), IntroKind::Update);
        assert_eq!(intro_kind(IntroMode::Never, false, None, "1"), IntroKind::None);
        assert_eq!(intro_kind(IntroMode::Never, true, Some("0"), "1"), IntroKind::None);
        assert_eq!(IntroMode::default(), IntroMode::OnUpdate);
        assert!(skips_intro("escape") && skips_intro("space"));
        assert!(!skips_intro("enter") && !skips_intro("a"));
        // Earlier review builds already wrote this spelling to interface.json.
        assert_eq!(serde_json::to_string(&IntroMode::OnUpdate).unwrap(), "\"OnUpdate\"");
    }

    #[test]
    fn intro_and_preload_both_gate_reveal_but_never_beyond_eight_seconds() {
        assert!(!may_reveal(
            Duration::from_secs(1),
            Duration::from_secs(1),
            true,
            IntroKind::Update,
            false
        ));
        assert!(may_reveal(Duration::from_secs(6), Duration::from_secs(6), true, IntroKind::Update, false));
        assert!(!may_reveal(
            Duration::from_secs(7),
            Duration::from_secs(6),
            false,
            IntroKind::Update,
            false
        ));
        assert!(may_reveal(
            Duration::from_secs(8),
            Duration::from_secs(6),
            false,
            IntroKind::Update,
            false
        ));
        assert!(may_reveal(Duration::ZERO, Duration::ZERO, true, IntroKind::None, false));
        assert!(!may_reveal(Duration::from_millis(1599), Duration::ZERO, false, IntroKind::None, false));
        assert!(may_reveal(Duration::from_millis(1600), Duration::ZERO, false, IntroKind::None, false));
        assert!(may_reveal(Duration::from_secs(1), Duration::ZERO, true, IntroKind::Update, true));
        assert!(!may_reveal(Duration::from_secs(1), Duration::ZERO, false, IntroKind::Update, true));
        // A stalled audio/device/frame callback cannot extend the absolute cap.
        assert!(may_reveal(Duration::from_secs(8), Duration::ZERO, true, IntroKind::Update, false));
        assert!(may_reveal(Duration::from_secs(8), Duration::ZERO, false, IntroKind::Update, false));
    }

    #[test]
    fn intro_audio_tail_fades_after_the_video_but_respects_the_absolute_limit() {
        let limit = Duration::from_secs(8);
        assert!(!audio_fade_due(Duration::from_millis(6249), Duration::from_secs(6), limit));
        assert!(audio_fade_due(Duration::from_millis(6250), Duration::from_secs(6), limit));
        assert!(audio_fade_due(Duration::ZERO, limit, limit));
    }

    #[test]
    fn random_selection_never_repeats_and_reaches_every_other_track() {
        let tracks = vec!["a.ogg".into(), "b.ogg".into(), "c.ogg".into()];
        for last in &tracks {
            let selected: std::collections::BTreeSet<_> =
                (0..1000).map(|r| choose_track(&tracks, Some(last), r).unwrap()).collect();
            assert_eq!(selected.len(), 2);
            assert!(!selected.contains(last.as_str()));
        }
        assert_eq!(choose_track(&[], None, 0), None);
        assert_eq!(choose_track(&tracks[..1], Some("a.ogg"), usize::MAX), Some("a.ogg"));
        assert_eq!((0..3).map(|r| choose_track(&tracks, None, r).unwrap()).count(), 3);
    }
}
