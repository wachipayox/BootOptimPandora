//! A 150 ms sRGB background transition. Independent of input dispatch and rendering.
use std::time::{Duration, Instant};
pub type Stops = [[f32; 4]; 2];
pub type Colors = [Stops; 3];
const DURATION: Duration = Duration::from_millis(150);
pub struct Motion {
    from: Stops,
    to: Stops,
    started: Instant,
    hovered: bool,
    pressed: bool,
}
impl Motion {
    pub fn new(colors: Colors, now: Instant) -> Self {
        Self {
            from: colors[0],
            to: colors[0],
            started: now,
            hovered: false,
            pressed: false,
        }
    }
    pub fn frame(&self, now: Instant) -> Stops {
        let t = (now.saturating_duration_since(self.started).as_secs_f32() / DURATION.as_secs_f32()).clamp(0., 1.);
        if t == 0. {
            return self.from;
        }
        if t == 1. {
            return self.to;
        }
        std::array::from_fn(|stop| {
            let alpha = self.from[stop][3] * (1. - t) + self.to[stop][3] * t;
            std::array::from_fn(|channel| {
                if channel == 3 {
                    alpha
                } else if alpha > 0. {
                    (self.from[stop][channel] * self.from[stop][3] * (1. - t)
                        + self.to[stop][channel] * self.to[stop][3] * t)
                        / alpha
                } else {
                    0.
                }
            })
        })
    }
    pub fn active(&self, now: Instant) -> bool {
        self.from != self.to && now.saturating_duration_since(self.started) < DURATION
    }
    pub fn event(&mut self, hovered: Option<bool>, pressed: Option<bool>, now: Instant, colors: Colors) {
        if let Some(v) = hovered {
            self.hovered = v;
        }
        if let Some(v) = pressed {
            self.pressed = v;
        }
        let target = colors[if self.pressed {
            2
        } else if self.hovered {
            1
        } else {
            0
        }];
        if target != self.to {
            self.from = self.frame(now);
            self.to = target;
            self.started = now;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn colors() -> Colors {
        [[[0.; 4]; 2], [[1.; 4]; 2], [[0.25; 4]; 2]]
    }
    #[test]
    fn reaches_hover_at_150_ms() {
        let now = Instant::now();
        let mut m = Motion::new(colors(), now);
        m.event(Some(true), None, now, colors());
        assert_eq!(m.frame(now), [[0.; 4]; 2]);
        assert_eq!(m.frame(now + Duration::from_millis(75)), [[1., 1., 1., 0.5]; 2]);
        assert_eq!(m.frame(now + DURATION), [[1.; 4]; 2]);
        assert!(!m.active(now + DURATION));
    }
    #[test]
    fn interruption_preserves_the_current_color() {
        let now = Instant::now();
        let mut m = Motion::new(colors(), now);
        m.event(Some(true), None, now, colors());
        let at = now + Duration::from_millis(60);
        let before = m.frame(at);
        m.event(None, Some(true), at, colors());
        assert_eq!(before, m.frame(at));
        assert_eq!(m.frame(at + DURATION), [[0.25; 4]; 2]);
    }
}
