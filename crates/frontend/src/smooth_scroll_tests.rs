use gpui::WheelMotion;
use std::time::{Duration, Instant};

#[test]
fn discrete_wheel_progresses_monotonically_and_finishes_exactly() {
    let now = Instant::now();
    let mut motion = WheelMotion::new([0., 0.], [0., -120.], [0., 1000.], now);
    let mut offset = [0., 0.];
    for ms in [0, 16, 32, 80, 159] {
        let (next, active) = motion.sample(offset, now + Duration::from_millis(ms)).unwrap();
        assert!(active);
        assert!(next[1] <= offset[1] && next[1] > -120.);
        offset = next;
    }
    assert_eq!(motion.sample(offset, now + Duration::from_millis(160)), Some(([0., -120.], false)));
}

#[test]
fn burst_accumulates_but_reversal_discards_pending_distance() {
    let now = Instant::now();
    let mut motion = WheelMotion::new([0., 0.], [0., -120.], [0., 1000.], now);
    motion.push([0., 0.], [0., -120.], [0., 1000.], now);
    let (offset, _) = motion.sample([0., 0.], now + Duration::from_millis(40)).unwrap();
    motion.push(offset, [0., 30.], [0., 1000.], now + Duration::from_millis(40));
    let (next, active) = motion.sample(offset, now + Duration::from_millis(200)).unwrap();
    assert!(!active);
    assert!((next[1] - (offset[1] + 30.)).abs() < 0.001);
}

#[test]
fn both_axes_are_clamped_and_late_frames_finish_without_replay() {
    let now = Instant::now();
    let mut motion = WheelMotion::new([-10., -10.], [-1000., 1000.], [50., 100.], now);
    assert_eq!(motion.sample([-10., -10.], now + Duration::from_secs(3)), Some(([-50., 0.], false)));
}

#[test]
fn external_scroll_or_resize_cancels_pending_travel() {
    let now = Instant::now();
    let mut motion = WheelMotion::new([-10., -100.], [0., -120.], [50., 1000.], now);
    assert!(motion.sample([-10., -40.], now + Duration::from_millis(16)).is_none());
}
