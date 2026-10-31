//! Optional animation of discrete mouse-wheel scrolling. Precise touchpad input
//! and direct scrollbar/keyboard positioning retain their original behavior.
use crate::{App, EntityId, Global, Pixels, Point, Window, point, px};
use std::{cell::RefCell, rc::Rc, time::Instant};

/// Application opt-in; disabled unless explicitly installed by the application.
#[derive(Default)]
pub struct SmoothScrollSettings(pub bool);
impl Global for SmoothScrollSettings {}

/// A wheel trajectory with an exact final offset and bounded pending distance.
#[derive(Clone, Debug)]
pub struct WheelMotion {
    start: Instant,
    from: [f32; 2],
    target: [f32; 2],
    last: [f32; 2],
}
impl WheelMotion {
    /// Start a trajectory, clamping both axes to negative scroll extents.
    pub fn new(current: [f32; 2], delta: [f32; 2], max: [f32; 2], now: Instant) -> Self {
        let target = std::array::from_fn(|i| (current[i] + delta[i]).clamp(-max[i].max(0.), 0.));
        Self {
            start: now,
            from: current,
            target,
            last: current,
        }
    }
    /// Accumulate wheel bursts; reversing an axis discards its pending travel.
    pub fn push(&mut self, current: [f32; 2], delta: [f32; 2], max: [f32; 2], now: Instant) {
        let target = std::array::from_fn(|i| {
            let pending = self.target[i] - current[i];
            let base = if pending * delta[i] < 0. {
                current[i]
            } else {
                self.target[i]
            };
            (base + delta[i]).clamp(-max[i].max(0.), 0.)
        });
        if target != self.target {
            self.from = current;
            self.last = current;
            self.target = target;
            self.start = now;
        }
    }
    /// Sample elapsed wall time. An external offset change cancels the motion.
    pub fn sample(&mut self, current: [f32; 2], now: Instant) -> Option<([f32; 2], bool)> {
        if (0..2).any(|i| (current[i] - self.last[i]).abs() > 0.1) {
            return None;
        }
        let t = (now.saturating_duration_since(self.start).as_secs_f32() / 0.16).min(1.);
        let eased = 1. - (1. - t).powi(3);
        self.last = std::array::from_fn(|i| self.from[i] + (self.target[i] - self.from[i]) * eased);
        Some((self.last, t < 1.))
    }
}

/// Shared animation state for a single scroll container.
#[derive(Clone, Default, Debug)]
pub struct SmoothScroll(Rc<RefCell<ScrollAnimation>>);
#[derive(Default, Debug)]
struct ScrollAnimation {
    motion: Option<WheelMotion>,
    generation: u64,
}
type Apply = Rc<RefCell<dyn FnMut(Option<Point<Pixels>>, &mut Window, &mut App) -> Point<Pixels>>>;
impl SmoothScroll {
    /// Cancel pending wheel travel before precise input or programmatic scrolling.
    pub fn cancel(&self) {
        let mut state = self.0.borrow_mut();
        state.motion.take();
        state.generation = state.generation.wrapping_add(1);
    }
    /// Queue a discrete wheel event. Returns false when the caller should apply
    /// its ordinary immediate scroll path. The callback reads on None and writes
    /// on Some; it always returns the actual resulting offset.
    pub fn queue(
        &self,
        current: Point<Pixels>,
        delta: Point<Pixels>,
        max: Point<Pixels>,
        precise: bool,
        window: &mut Window,
        cx: &mut App,
        apply: impl FnMut(Option<Point<Pixels>>, &mut Window, &mut App) -> Point<Pixels> + 'static,
    ) -> bool {
        if precise || cx.reduce_motion() || !cx.try_global::<SmoothScrollSettings>().is_some_and(|s| s.0) {
            self.cancel();
            return false;
        }
        let values = |p: Point<Pixels>| [f32::from(p.x), f32::from(p.y)];
        let now = Instant::now();
        let mut state = self.0.borrow_mut();
        let scheduled = state.motion.is_some();
        if let Some(motion) = state.motion.as_mut() {
            motion.push(values(current), values(delta), values(max), now);
        } else {
            state.motion = Some(WheelMotion::new(values(current), values(delta), values(max), now));
        }
        let generation = state.generation;
        drop(state);
        if !scheduled {
            self.schedule(generation, window.current_view(), Rc::new(RefCell::new(apply)), window);
        }
        true
    }
    fn schedule(&self, generation: u64, view: EntityId, apply: Apply, window: &mut Window) {
        let this = self.clone();
        window.on_next_frame(move |window, cx| {
            if this.0.borrow().generation != generation {
                return;
            }
            let current = (apply.borrow_mut())(None, window, cx);
            let mut state = this.0.borrow_mut();
            let Some(motion) = state.motion.as_mut() else {
                return;
            };
            let sampled = if cx.reduce_motion() {
                Some((motion.target, false))
            } else {
                motion.sample([current.x.into(), current.y.into()], Instant::now())
            };
            let Some((next, active)) = sampled else {
                state.motion.take();
                return;
            };
            if !active {
                state.motion.take();
            }
            drop(state);
            (apply.borrow_mut())(Some(point(px(next[0]), px(next[1]))), window, cx);
            cx.notify(view);
            if active {
                this.schedule(generation, view, apply, window);
            }
        });
    }
}
