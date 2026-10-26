//! Deterministic port of the approved six-second Canvas timeline.
//! All paths and rectangles are painted on the GPUI window thread.
use super::geometry as d;
use gpui::{prelude::*, *};
use std::f64::consts::{PI, TAU};

#[derive(Clone, Copy)]
pub(super) enum Command {
    Move(f64, f64),
    Line(f64, f64),
    Quad(f64, f64, f64, f64),
    Cubic(f64, f64, f64, f64, f64, f64),
    Close,
}
const BG: u32 = 0x0e0e0f;
const ACC: u32 = 0x5bcef5;
const FACE: u32 = 0xffffff;
#[derive(Clone, Copy)]
struct Scheme {
    bg: u32,
    face: u32,
    relief: u32,
    bits: [u32; 2],
    inverse: bool,
}
const DARK: Scheme = Scheme {
    bg: BG,
    face: FACE,
    relief: ACC,
    bits: [FACE, ACC],
    inverse: false,
};
const INV: Scheme = Scheme {
    bg: ACC,
    face: BG,
    relief: 0x1d7ba0,
    bits: [BG, FACE],
    inverse: true,
};

fn clamp(v: f64) -> f64 {
    v.clamp(0., 1.)
}
fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}
fn seg(t: f64, a: f64, b: f64) -> f64 {
    clamp((t - a) / (b - a))
}
fn eio(t: f64) -> f64 {
    if t < 0.5 {
        4. * t * t * t
    } else {
        1. - (-2. * t + 2.).powi(3) / 2.
    }
}
fn eo(t: f64) -> f64 {
    1. - (1. - t).powi(3)
}
fn eob(t: f64) -> f64 {
    1. + 2.6 * (t - 1.).powi(3) + 1.6 * (t - 1.).powi(2)
}
fn eio5(t: f64) -> f64 {
    if t < 0.5 {
        16. * t.powi(5)
    } else {
        1. - (-2. * t + 2.).powi(5) / 2.
    }
}

struct Rng(u32);
impl Rng {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x6d2b79f5);
        let mut t = (self.0 ^ (self.0 >> 15)).wrapping_mul(1 | self.0);
        t = t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t)) ^ t;
        (t ^ (t >> 14)) as f64 / 4294967296.
    }
}
#[derive(Clone, Copy)]
struct State {
    sx: f64,
    sy: f64,
    tx: f64,
    ty: f64,
    alpha: f64,
    mask: Bounds<Pixels>,
}
struct Painter<'a> {
    window: &'a mut Window,
    state: State,
    origin: Point<Pixels>,
    scale: f64,
    viewport: Bounds<Pixels>,
    visible: [f64; 4],
}
impl Painter<'_> {
    fn identity(&mut self) {
        self.state.sx = 1.;
        self.state.sy = 1.;
        self.state.tx = 0.;
        self.state.ty = 0.;
    }
    fn cam(&mut self, z: f64, x: f64, y: f64) {
        self.state.sx = z;
        self.state.sy = z;
        self.state.tx = 960. - z * x;
        self.state.ty = 540. - z * y;
    }
    fn translate(&mut self, x: f64, y: f64) {
        self.state.tx += self.state.sx * x;
        self.state.ty += self.state.sy * y;
    }
    fn scale(&mut self, x: f64, y: f64) {
        self.state.sx *= x;
        self.state.sy *= y;
    }
    fn point(&self, x: f64, y: f64) -> Point<Pixels> {
        point(
            self.origin.x + px(((self.state.sx * x + self.state.tx) * self.scale) as f32),
            self.origin.y + px(((self.state.sy * y + self.state.ty) * self.scale) as f32),
        )
    }
    fn rect_bounds(&self, x: f64, y: f64, w: f64, h: f64) -> Bounds<Pixels> {
        let a = self.point(x, y);
        let b = self.point(x + w, y + h);
        let (x0, y0) = (a.x.as_f32().min(b.x.as_f32()), a.y.as_f32().min(b.y.as_f32()));
        Bounds::new(
            point(px(x0), px(y0)),
            size(px((b.x - a.x).as_f32().abs()), px((b.y - a.y).as_f32().abs())),
        )
    }
    fn clip(&mut self, x: f64, y: f64, w: f64, h: f64) {
        let a = self.state.mask;
        let b = self.rect_bounds(x, y, w, h);
        let x = a.origin.x.as_f32().max(b.origin.x.as_f32());
        let y = a.origin.y.as_f32().max(b.origin.y.as_f32());
        let right = (a.origin.x + a.size.width).as_f32().min((b.origin.x + b.size.width).as_f32());
        let bottom = (a.origin.y + a.size.height).as_f32().min((b.origin.y + b.size.height).as_f32());
        self.state.mask = Bounds::new(point(px(x), px(y)), size(px((right - x).max(0.)), px((bottom - y).max(0.))));
    }
    fn color(&self, hex: u32) -> Rgba {
        let mut c = rgb(hex);
        c.a = self.state.alpha as f32;
        c
    }
    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64, color: u32) {
        let bounds = self.rect_bounds(x, y, w, h);
        let color = self.color(color);
        let mask = self.state.mask;
        self.window
            .with_content_mask(Some(ContentMask { bounds: mask }), |window| window.paint_quad(fill(bounds, color)));
    }
    fn path(&mut self, commands: &[Command], color: u32, even_odd: bool) {
        // Bézier curves stay inside their control hull. Skip only hulls wholly
        // outside the clip, with two physical pixels of antialiasing padding.
        // The giant mirrored words otherwise tessellate dozens of invisible
        // letters on every frame. This check runs on the window thread too.
        let [left, top, right, bottom] = command_bounds(commands);
        if !left.is_finite() {
            return;
        }
        let bounds = self.rect_bounds(left, top, right - left, bottom - top);
        let mask = self.state.mask;
        let margin = px(2. / self.window.scale_factor());
        if bounds.origin.x + bounds.size.width < mask.origin.x - margin
            || bounds.origin.y + bounds.size.height < mask.origin.y - margin
            || bounds.origin.x > mask.origin.x + mask.size.width + margin
            || bounds.origin.y > mask.origin.y + mask.size.height + margin
        {
            return;
        }
        let rule = if even_odd { FillRule::EvenOdd } else { FillRule::NonZero };
        let mut path = PathBuilder::fill().with_style(PathStyle::Fill(
            FillOptions::default()
                .with_fill_rule(rule)
                .with_tolerance(0.02 / self.window.scale_factor()),
        ));
        for c in commands {
            match *c {
                Command::Move(x, y) => path.move_to(self.point(x, y)),
                Command::Line(x, y) => path.line_to(self.point(x, y)),
                Command::Quad(cx, cy, x, y) => path.curve_to(self.point(x, y), self.point(cx, cy)),
                Command::Cubic(ax, ay, bx, by, x, y) => {
                    path.cubic_bezier_to(self.point(x, y), self.point(ax, ay), self.point(bx, by))
                },
                Command::Close => path.close(),
            }
        }
        self.paint(path, color);
    }
    fn paint(&mut self, builder: PathBuilder, color: u32) {
        match builder.build() {
            Ok(path) => {
                let color = self.color(color);
                let mask = self.state.mask;
                self.window
                    .with_content_mask(Some(ContentMask { bounds: mask }), |window| window.paint_path(path, color));
            },
            Err(error) => log::warn!("Intro path tessellation failed: {error:?}"),
        }
    }
    fn line(&mut self, a: [f64; 2], b: [f64; 2], width: f64, color: u32) {
        let mut path = PathBuilder::stroke(px((width * self.state.sx.abs() * self.scale) as f32));
        path.move_to(self.point(a[0], a[1]));
        path.line_to(self.point(b[0], b[1]));
        self.paint(path, color);
    }
    fn arc(&mut self, x: f64, y: f64, radius: f64, width: f64, color: u32, start: f64, end: f64) {
        let mut path =
            PathBuilder::stroke(px((width * self.state.sx.abs() * self.scale) as f32)).with_style(PathStyle::Stroke(
                StrokeOptions::default()
                    .with_line_width((width * self.state.sx.abs() * self.scale) as f32)
                    .with_tolerance(0.02 / self.window.scale_factor()),
            ));
        let span = (end - start).min(TAU);
        // GPUI's SVG arc_to converts arcs into quadratic pieces of up to 45°.
        // At the intro's large zoom that visibly moves the circular contour.
        // Small cubic pieces retain the source circle to subpixel precision.
        let n = (span / (PI / 8.)).ceil().max(1.) as usize;
        path.move_to(self.point(x + radius * start.cos(), y + radius * start.sin()));
        for i in 1..=n {
            let previous = start + span * (i - 1) as f64 / n as f64;
            let a = start + span * i as f64 / n as f64;
            let k = 4. / 3. * ((a - previous) / 4.).tan();
            path.cubic_bezier_to(
                self.point(x + radius * a.cos(), y + radius * a.sin()),
                self.point(
                    x + radius * (previous.cos() - k * previous.sin()),
                    y + radius * (previous.sin() + k * previous.cos()),
                ),
                self.point(x + radius * (a.cos() + k * a.sin()), y + radius * (a.sin() - k * a.cos())),
            );
        }
        if (span - TAU).abs() < 1e-8 {
            path.close();
        }
        self.paint(path, color);
    }
    fn arcs(&mut self, x: f64, y: f64, r: f64, w: f64, color: u32, arcs: &[[f64; 2]]) {
        for [a, b] in arcs {
            self.arc(x, y, r, w, color, a.to_radians(), b.to_radians());
        }
    }
    fn draw_w(&mut self, x: f64, y: f64, s: f64, sc: Scheme, relief: f64, alpha: f64) {
        let saved = self.state;
        self.state.alpha = alpha;
        self.translate(x, y);
        self.scale(s, s);
        let position = self.state;
        self.translate(-d::RELIEF_OFFSET[0] * (1. - relief), -d::RELIEF_OFFSET[1] * (1. - relief));
        self.path(d::RELIEF, sc.relief, false);
        self.state = position;
        self.path(d::W, sc.face, true);
        self.state = saved;
    }
    fn assembly(&mut self, t: f64, sc: Scheme) {
        let [x, y, s] = d::START;
        let [rx, ry, r, w] = d::RING;
        let e = eio(seg(t, 0., 1.6));
        let rad = r * s * lerp(1.5, 1., e);
        let (cx, cy) = (x + rx * s, y + ry * s);
        for p in pieces() {
            let k = eob(seg(t, p.delay * 0.9, 1.6));
            if seg(t, p.delay * 0.5, p.delay * 0.5 + 0.08) <= 0. {
                continue;
            }
            let rr = rad * (1. + p.rad * (1. - k));
            let rot = p.rot * (1. - k);
            self.arcs(cx, cy, rr, w * s * lerp(1.4, 1., e), sc.face, &[[p.a0 + rot, p.a1 + rot]]);
        }
        let alpha = eo(seg(t, 0.2, 0.45));
        if alpha > 0. {
            self.draw_w(x, y, s, sc, eo(seg(t, 0.3, 0.9)), alpha);
        }
    }
    fn full_ring(&mut self, sc: Scheme, t: f64) {
        let [x, y, s] = d::START;
        let [rx, ry, r, w] = d::RING;
        let arcs = [
            [491., 571., 0.],
            [411., 491., 0.1],
            [331., 411., 0.2],
            [237., 305., 0.3],
        ]
        .map(|[a, b, delay]| {
            let angle = 360. * eio5(seg(t, 1.80 + delay, 2.65 + delay));
            [a + angle, b + angle]
        });
        self.arcs(x + rx * s, y + ry * s, r * s, w * s, sc.face, &arcs);
        self.draw_w(x, y, s, sc, 1., 1.);
    }
    fn rolling_center(t: f64) -> [f64; 2] {
        let [x, y, s] = d::END;
        let [rx, ry, r, _] = d::RING;
        let rr = r * s;
        let xs = d::LINE[0] + rr * 0.9;
        let hop = eio(seg(t, 3.05, 3.2));
        let p = clamp(eio(seg(t, 3.15, 4.)));
        if p > 0. {
            [xs + p * TAU * rr, d::LINE[2] - rr]
        } else {
            [lerp(x + rx * s, xs, hop), lerp(y + ry * s, d::LINE[2] - rr, hop)]
        }
    }
    fn rolling(&mut self, t: f64, sc: Scheme) {
        let [x, y, s] = d::END;
        let rr = d::RING[2] * s;
        let rw = d::RING[3] * s;
        let p = clamp(eio(seg(t, 3.15, 4.)));
        let xs = d::LINE[0] + rr * 0.9;
        let [cx, cy] = Self::rolling_center(t);
        if p > 0. {
            let y = d::LINE[2] + (rw - d::LINE[3]) / 2.;
            self.line([xs, y], [cx, y], rw, sc.face);
        }
        if p < 1. {
            self.arc(cx, cy, rr, rw, sc.face, PI / 2. - TAU * (1. - p), PI / 2.);
        }
        self.draw_w(x, y, s, sc, 1., 1.);
    }
    fn letter(&mut self, i: usize, dx: f64, dy: f64, color: u32) {
        let saved = self.state;
        self.translate(d::TEXT[0] + dx, d::TEXT[1] + dy);
        self.scale(d::TEXT[2], d::TEXT[2]);
        self.path(d::LETTERS[i], color, false);
        self.state = saved;
    }
    fn lockup(&mut self, sc: Scheme, on: impl Fn(usize) -> f64, glitch: f64, t: f64, relief: f64) {
        let [x, y, s] = d::END;
        self.draw_w(x, y, s, sc, 1., 1.);
        self.line([d::LINE[0], d::LINE[2]], [d::LINE[1], d::LINE[2]], d::LINE[3], sc.face);
        let cap = d::TEXT[2];
        let mut rng = Rng((t * 30.).floor() as u32 * 31 + 5);
        for i in 0..8 {
            let k = on(i);
            if k <= 0. {
                continue;
            }
            let jitter = (1. - k) * cap * 0.9;
            if k < 1. || glitch > 0. {
                let amp = jitter.max(glitch * cap * 0.5);
                for slice in 0..3 {
                    let saved = self.state;
                    self.clip(0., d::TEXT[1] - cap * (1. - slice as f64 / 3.) - 2., 1920., cap / 3. + 2.);
                    let dx = (rng.next() - 0.5) * amp;
                    if amp > 2. {
                        self.letter(i, dx + amp * 0.12, 0., if sc.inverse { FACE } else { ACC });
                    }
                    self.letter(i, dx, 0., sc.face);
                    self.state = saved;
                }
            } else {
                if relief > 0. {
                    self.letter(i, d::TEXT[3] * cap * relief, d::TEXT[4] * cap * relief, sc.relief);
                }
                self.letter(i, 0., 0., sc.face);
            }
        }
    }
    fn word(&mut self, x: f64, y: f64, cap: f64, color: u32, mirror: bool) {
        let saved = self.state;
        self.translate(x, y);
        if mirror {
            self.scale(-1., 1.);
        }
        self.scale(cap, cap);
        for path in d::LETTERS {
            self.path(path, color, false);
        }
        self.state = saved;
    }
    fn bg(&mut self, sc: Scheme) {
        self.identity();
        self.window.paint_quad(fill(self.viewport, rgb(sc.bg)));
    }
    fn debris(&mut self, t: f64, sc: Scheme, seed: u32, n: usize, amount: f64) {
        if amount <= 0. {
            return;
        }
        self.identity();
        let mut r = Rng(seed);
        self.state.alpha = 0.07 * amount;
        let [left, top, right, bottom] = self.visible;
        let intercepts = [r.next() * 1920., r.next() * 1920., r.next() * 1920.];
        // Continue the diagonal lines beyond the source frame. The period
        // includes their horizontal drift, so adjacent copies cannot alter
        // the approved 1920x1080 composition.
        let first = ((left + 620. * top / 1080. - 1920.) / 2540.).floor() as i32 - 1;
        let last = ((right + 620. * bottom / 1080.) / 2540.).ceil() as i32 + 1;
        for tile in first..=last {
            for x in intercepts {
                let a = x + tile as f64 * 2540. - 620. * top / 1080.;
                let b = x + tile as f64 * 2540. - 620. * bottom / 1080.;
                if a.max(b) >= left - 2. && a.min(b) <= right + 2. {
                    self.line([a, top], [b, bottom], 1.5, sc.bits[0]);
                }
            }
        }
        self.state.alpha = amount;
        for _ in 0..n {
            let long = r.next() < 0.3;
            let w = if long {
                30. + r.next() * 70.
            } else {
                4. + r.next() * 14.
            };
            let h = if long { 3. } else { 3. + r.next() * 5. };
            let v = (r.next() - 0.5) * 900.;
            let x = (r.next() * 1920. + v * t).rem_euclid(1920.);
            let y = r.next() * 1080.;
            let c = sc.bits[if r.next() < 0.7 { 0 } else { 1 }];
            for ix in ((left - x - w) / 2020.).floor() as i32..=((right - x) / 2020.).ceil() as i32 {
                for iy in ((top - y - h) / 1090.).floor() as i32..=((bottom - y) / 1090.).ceil() as i32 {
                    let (xx, yy) = (x + ix as f64 * 2020., y + iy as f64 * 1090.);
                    if xx + w >= left && xx <= right && yy + h >= top && yy <= bottom {
                        self.rect(xx, yy, w, h, c);
                    }
                }
            }
        }
        self.state.alpha = 1.;
    }
    fn band(&mut self, y0: f64, y1: f64, color: u32, draw: impl FnOnce(&mut Self)) {
        let saved = self.state;
        self.identity();
        let [left, top, right, bottom] = self.visible;
        let y0 = if y0 == 0. { top.min(0.) } else { y0 };
        let y1 = if y1 == 1080. { bottom.max(1080.) } else { y1 };
        self.clip(left, y0, right - left, y1 - y0);
        self.window.with_content_mask(
            Some(ContentMask {
                bounds: self.state.mask,
            }),
            |window| window.paint_quad(fill(self.state.mask, rgb(color))),
        );
        draw(self);
        self.state = saved;
    }
    fn render(&mut self, t: f64) {
        let [x, y, s] = d::START;
        let [rx, ry, r, _] = d::RING;
        let ring = [x + rx * s, y + ry * s];
        if t < 0.30 {
            self.bg(DARK);
            let u = seg(t, 0., 0.3);
            let p = pieces()[0];
            let e = eio(seg(t, 0., 1.6));
            let k = eob(seg(t, 0., 1.6));
            let a = ((p.a0 + p.a1) / 2. + p.rot * (1. - k)).to_radians();
            let rr = r * s * lerp(1.5, 1., e) * (1. + p.rad * (1. - k));
            self.cam(3., ring[0] + rr * a.cos() + 60. - u * 120., ring[1] + rr * a.sin());
            self.assembly(t, DARK);
            self.debris(t, DARK, 1, 14, 1.);
        } else if t < 0.55 {
            self.bg(DARK);
            let u = seg(t, 0.3, 0.55);
            self.cam(2.8, x + (80. + u * 60.) * s, y + (110. - u * 30.) * s);
            self.assembly(t, DARK);
            self.debris(t, DARK, 2, 14, 1.);
        } else if t < 0.80 {
            self.bg(INV);
            let u = seg(t, 0.55, 0.8);
            self.cam(3., ring[0] + 230. - u * 80., ring[1] - 90. + u * 50.);
            self.assembly(t, INV);
            self.debris(t, INV, 3, 14, 1.);
        } else if t < 1.30 {
            self.bg(DARK);
            let u = seg(t, 0.8, 1.3);
            let (a, b) = if t >= 1.05 { (INV, DARK) } else { (DARK, INV) };
            self.band(0., 352., a.bg, |p| {
                p.cam(2.2, x + (120. + u * 260.) * s, y + 130. * s);
                p.assembly(t, a);
            });
            self.band(364., 716., b.bg, |p| {
                p.cam(1.25, ring[0] + 200. - u * 400., ring[1]);
                p.assembly(t, b);
            });
            self.band(728., 1080., a.bg, |p| {
                p.cam(2.2, ring[0] + 260. - u * 300., ring[1] + r * s * 1.1);
                p.assembly(t, a);
            });
            self.debris(t, DARK, 4, 10, 1.);
        } else if t < 1.65 {
            self.bg(DARK);
            let u = eo(seg(t, 1.3, 1.65));
            self.cam(lerp(1.12, 1., u), 960., 540.);
            self.assembly(t, DARK);
            self.debris(t, DARK, 5, 10, 1. - u * 0.5);
        } else if t < 1.75 {
            self.bg(DARK);
            self.cam(1., 960., 540.);
            self.full_ring(DARK, 0.);
        } else if t < 2.50 {
            self.bg(DARK);
            self.cam(lerp(1., 1.05, seg(t, 1.75, 2.5)), 960., 540.);
            self.full_ring(DARK, t);
            self.debris(t, DARK, 6, 6, 0.6);
        } else if t < 3.05 {
            self.bg(DARK);
            let u = seg(t, 2.5, 3.05);
            self.cam(lerp(1.55, 1.62, u), 960., lerp(430., 470., u));
            self.full_ring(DARK, t);
            self.debris(t, DARK, 7, 6, 0.6);
        } else if t < 3.55 {
            let sc = if t < 3.12 { INV } else { DARK };
            self.bg(sc);
            let probe = Self::rolling_center(t);
            self.cam(1.7, probe[0].clamp(700., 1500.), d::LINE[2] - r * d::END[2] * 0.7);
            self.rolling(t, sc);
            self.debris(t, sc, 8, 12, 1.);
        } else if t < 4.05 {
            self.bg(DARK);
            self.cam(lerp(1.08, 1., eo(seg(t, 3.55, 4.05))), 1000., 540.);
            self.rolling(t, DARK);
            self.debris(t, DARK, 9, 10, 1.);
        } else if t < 4.55 {
            let u = seg(t, 4.05, 4.55);
            let (a, b) = if t >= 4.30 { (DARK, INV) } else { (INV, DARK) };
            let cap = 300.;
            let ww = d::LETTERS_WIDTH * cap;
            self.bg(a);
            let copies = ((self.visible[2] - self.visible[0]) / (ww + 160.)).ceil() as i32 + 3;
            self.band(0., 534., a.bg, |p| {
                for i in -copies..copies {
                    p.word(-200. - u * 700. + i as f64 * (ww + 160.), 450., cap, a.face, false);
                }
            });
            self.band(546., 1080., b.bg, |p| {
                for i in -copies..copies {
                    p.word(1700. + u * 700. - i as f64 * (ww + 160.), 960., cap, b.face, true);
                }
            });
            self.debris(t, a, 10, 12, 1.);
        } else if t < 5.05 {
            self.bg(DARK);
            self.cam(lerp(1.06, 1., eo(seg(t, 4.55, 5.05))), 960., 540.);
            let order = [3, 0, 6, 1, 5, 2, 7, 4];
            let glitch = if (4.80..4.87).contains(&t) { 1. } else { 0. };
            self.lockup(
                DARK,
                |i| {
                    let n = order.iter().position(|n| *n == i).unwrap();
                    let a = 4.57 + n as f64 * 0.045;
                    eo(seg(t, a, a + 0.12))
                },
                glitch,
                t,
                eo(seg(t, 4.85, 5.)),
            );
            self.debris(t, DARK, 11, 10, 0.8);
        } else if t < 5.30 {
            self.bg(INV);
            self.cam(1., 960., 540.);
            self.lockup(INV, |_| 1., 0., 0., 1.);
            self.debris(t, INV, 12, 8, 0.6);
        } else {
            self.bg(DARK);
            self.cam(1., 960., 540.);
            self.lockup(DARK, |_| 1., 0., 0., 1.);
            self.debris(t, DARK, 13, 8, 1. - eo(seg(t, 5.3, 5.7)));
        }
    }
}
fn command_bounds(commands: &[Command]) -> [f64; 4] {
    let mut bounds = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    let mut include = |x: f64, y: f64| {
        bounds[0] = bounds[0].min(x);
        bounds[1] = bounds[1].min(y);
        bounds[2] = bounds[2].max(x);
        bounds[3] = bounds[3].max(y);
    };
    for command in commands {
        match *command {
            Command::Move(x, y) | Command::Line(x, y) => include(x, y),
            Command::Quad(cx, cy, x, y) => {
                include(cx, cy);
                include(x, y);
            },
            Command::Cubic(ax, ay, bx, by, x, y) => {
                include(ax, ay);
                include(bx, by);
                include(x, y);
            },
            Command::Close => {},
        }
    }
    bounds
}

#[cfg(test)]
mod tests {
    use super::{Command, command_bounds};
    #[test]
    fn control_hull_keeps_curves_whose_endpoints_are_outside_the_viewport() {
        let quadratic = [Command::Move(-100., 0.), Command::Quad(300., 100., -100., 200.)];
        let bounds = command_bounds(&quadratic);
        // At t=1/2 the curve reaches x=100 despite both endpoints being x=-100.
        assert!(bounds[0] <= 100. && bounds[2] >= 100.);
        let cubic = [
            Command::Move(-100., 0.),
            Command::Cubic(300., -40., 400., 240., -100., 200.),
        ];
        let bounds = command_bounds(&cubic);
        assert_eq!(bounds, [-100., -40., 400., 240.]);
    }
}
#[derive(Clone, Copy, Default)]
struct Piece {
    a0: f64,
    a1: f64,
    rot: f64,
    rad: f64,
    delay: f64,
}
fn pieces() -> [Piece; 8] {
    let mut r = Rng(7);
    let mut out = [Piece::default(); 8];
    let mut index = 0;
    for [a, b] in d::RING_ARCS {
        let n = if b - a > 100. { 6 } else { 2 };
        for i in 0..n {
            out[index] = Piece {
                a0: a + (b - a) * i as f64 / n as f64,
                a1: a + (b - a) * (i + 1) as f64 / n as f64,
                rot: (r.next() - 0.5) * 140.,
                rad: 0.25 + r.next() * 0.55,
                delay: r.next() * 0.75,
            };
            index += 1;
        }
    }
    out[0].delay = 0.;
    out[1].delay = 0.05;
    out
}

pub(crate) fn paint(t: f64, bounds: Bounds<Pixels>, window: &mut Window) {
    window.paint_quad(fill(bounds, rgb(BG)));
    let scale = (bounds.size.width.as_f32() as f64 / 1920.).min(bounds.size.height.as_f32() as f64 / 1080.);
    if !scale.is_finite() || scale <= 0. {
        return;
    }
    let origin = point(
        bounds.origin.x + px(((bounds.size.width.as_f32() as f64 - 1920. * scale) / 2.) as f32),
        bounds.origin.y + px(((bounds.size.height.as_f32() as f64 - 1080. * scale) / 2.) as f32),
    );
    let left = (bounds.origin.x - origin.x).as_f32() as f64 / scale;
    let top = (bounds.origin.y - origin.y).as_f32() as f64 / scale;
    let mut p = Painter {
        window,
        state: State {
            sx: 1.,
            sy: 1.,
            tx: 0.,
            ty: 0.,
            alpha: 1.,
            mask: bounds,
        },
        origin,
        scale,
        viewport: bounds,
        visible: [
            left,
            top,
            left + bounds.size.width.as_f32() as f64 / scale,
            top + bounds.size.height.as_f32() as f64 / scale,
        ],
    };
    p.render(t.clamp(0., 6.));
}
pub(crate) fn frame(t: f64) -> AnyElement {
    frame_timed(t, None)
}
pub(crate) struct FirstPaint {
    pub native: std::time::Instant,
    pub main: std::time::Instant,
    pub ready: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub scheduled: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
pub(crate) fn frame_timed(t: f64, first_paint_origin: Option<FirstPaint>) -> AnyElement {
    canvas(
        move |bounds, _, _| bounds,
        move |bounds, _, window, _| {
            #[cfg(feature = "ui-capture")]
            let started = std::time::Instant::now();
            paint(t, bounds, window);
            #[cfg(feature = "ui-capture")]
            if std::env::var("WBOMBO_UI_CAPTURE_PAGE").is_ok_and(|page| page.starts_with("startup-bench:")) {
                static ORIGIN: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
                let origin = *ORIGIN.get_or_init(|| started);
                log::info!(
                    "startup-frame-cost pid={} t={} logical_width={} logical_height={} scale={} cpu_paint_ns={} wall_frame_start_ns={}",
                    std::process::id(),
                    t,
                    bounds.size.width.as_f32(),
                    bounds.size.height.as_f32(),
                    window.scale_factor(),
                    started.elapsed().as_nanos(), started.duration_since(origin).as_nanos()
                );
            }
            if let Some(signal) = &first_paint_origin {
                if !signal.scheduled.swap(true, std::sync::atomic::Ordering::AcqRel) {
                let painted = std::time::Instant::now();
                log::info!(
                    "startup-first-paint pid={} native_window_to_paint_us={} main_to_paint_us={}",
                    std::process::id(),
                    painted.duration_since(signal.native).as_micros(), painted.duration_since(signal.main).as_micros()
                );
                let ready = signal.ready.clone();
                window.on_next_frame(move |_, _| ready.store(true, std::sync::atomic::Ordering::Release));
                }
            }
        },
    )
    .size_full()
    .into_any_element()
}
