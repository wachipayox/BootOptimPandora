//! Startup decisions are independent of the renderer and of network availability.
use std::time::Duration;

pub(super) const INTRO: Duration = Duration::from_secs(6);
pub(super) const PRELOAD_LIMIT: Duration = Duration::from_secs(8);
pub(super) const TRANSITION: Duration = Duration::from_millis(300);
pub(super) const AUDIO_FADE: Duration = Duration::from_millis(150);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum IntroKind {
    FirstInstall,
    Update,
    None,
}

pub(super) fn intro_kind(existing_install: bool, last_version: Option<&str>, current: &str) -> IntroKind {
    if last_version == Some(current) {
        IntroKind::None
    } else if existing_install {
        IntroKind::Update
    } else {
        IntroKind::FirstInstall
    }
}

pub(super) fn may_reveal(
    elapsed: Duration,
    animation_elapsed: Duration,
    ready: bool,
    intro: IntroKind,
    skipped: bool,
) -> bool {
    elapsed >= PRELOAD_LIMIT || (ready && (intro == IntroKind::None || skipped || animation_elapsed >= INTRO))
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
    use super::{IntroKind, choose_track, intro_kind, may_reveal};
    use std::time::Duration;

    #[test]
    fn only_new_install_and_changed_versions_have_intro() {
        assert_eq!(intro_kind(false, None, "1"), IntroKind::FirstInstall);
        assert_eq!(intro_kind(true, None, "1"), IntroKind::Update);
        assert_eq!(intro_kind(true, Some("0"), "1"), IntroKind::Update);
        assert_eq!(intro_kind(true, Some("1"), "1"), IntroKind::None);
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
        assert!(may_reveal(Duration::from_secs(1), Duration::ZERO, true, IntroKind::Update, true));
        assert!(!may_reveal(Duration::from_secs(1), Duration::ZERO, false, IntroKind::Update, true));
        // A stalled audio/device/frame callback cannot extend the absolute cap.
        assert!(may_reveal(Duration::from_secs(8), Duration::ZERO, true, IntroKind::Update, false));
        assert!(may_reveal(Duration::from_secs(8), Duration::ZERO, false, IntroKind::Update, false));
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
