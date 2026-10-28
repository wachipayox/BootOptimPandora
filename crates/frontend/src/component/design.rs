//! Shared controls for the approved Wachiland design.
use super::{
    background_motion::{Colors, Motion},
    tracked_text::TrackedText,
};
use crate::theme;
use gpui::{prelude::*, *};
use gpui_component::{
    Disableable, Selectable,
    button::{Button, ButtonCustomVariant, ButtonVariants},
    h_flex, v_flex,
};
use std::rc::Rc;
use std::time::Instant;

/// Adds state styling to the native button without changing its input handling.
#[derive(IntoElement)]
pub struct DesignButton {
    id: ElementId,
    inner: Button,
    paint: Option<(f32, Colors, Hsla, Hsla)>,
    action: bool,
    notched: bool,
    label: Option<SharedString>,
    label_left: bool,
    disabled: bool,
}
impl DesignButton {
    pub fn new(id: impl Into<ElementId>) -> Self {
        let id = id.into();
        Self {
            id: id.clone(),
            inner: Button::new(id).ghost().border_0(),
            paint: None,
            action: false,
            notched: false,
            label: None,
            label_left: false,
            disabled: false,
        }
    }
    pub fn notched(mut self) -> Self {
        self.notched = true;
        self
    }
    pub fn action(mut self) -> Self {
        self.action = true;
        self
    }
    pub fn paint(mut self, angle: f32, fills: [[u32; 2]; 3], foreground: Hsla, hover_foreground: Hsla) -> Self {
        let colors = fills.map(|stops| {
            stops.map(|v| {
                let c = rgba(v);
                [c.r, c.g, c.b, c.a]
            })
        });
        self.paint = Some((angle, colors, foreground, hover_foreground));
        self
    }
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }
    pub fn label_left(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self.label_left = true;
        self
    }
    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.inner = self.inner.tooltip(tooltip);
        self
    }
    pub fn toggled(mut self, value: bool) -> Self {
        self.inner = self.inner.toggled(value);
        self
    }
    pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.inner = self.inner.on_click(handler);
        self
    }
}
impl Styled for DesignButton {
    fn style(&mut self) -> &mut StyleRefinement {
        self.inner.style()
    }
}
impl InteractiveElement for DesignButton {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.inner.interactivity()
    }
}
impl StatefulInteractiveElement for DesignButton {}
impl ParentElement for DesignButton {
    fn extend(&mut self, children: impl IntoIterator<Item = AnyElement>) {
        self.inner.extend(children);
    }
}
impl Disableable for DesignButton {
    fn disabled(mut self, value: bool) -> Self {
        self.disabled = value;
        self.inner = self.inner.disabled(value);
        self
    }
}
impl Selectable for DesignButton {
    fn selected(mut self, selected: bool) -> Self {
        self.inner = self.inner.selected(selected);
        self
    }
    fn is_selected(&self) -> bool {
        self.inner.is_selected()
    }
}
impl gpui_component::menu::DropdownMenu for DesignButton {}
impl ButtonVariants for DesignButton {
    fn with_variant(mut self, variant: gpui_component::button::ButtonVariant) -> Self {
        self.inner = self.inner.with_variant(variant);
        self
    }
}
impl RenderOnce for DesignButton {
    fn render(mut self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        if let Some(label) = self.label.take() {
            self = self.aria_label(label.clone());
            self.inner = self.inner.child(
                div()
                    .w_full()
                    .font_family(theme::SANS)
                    .text_size(px(13.))
                    .line_height(px(16.))
                    .when_else(self.label_left, |d| d.text_left(), |d| d.text_center())
                    .child(label),
            );
        }
        let mut outer_fill = None;
        if let Some((angle, colors, fg, hover_fg)) = self.paint {
            let motion = window.use_keyed_state((self.id.clone(), "background-motion"), cx, |_, _| {
                Motion::new(colors, Instant::now())
            });
            motion.update(cx, |m, _| m.event(None, None, Instant::now(), colors));
            let now = Instant::now();
            let stops = if self.disabled {
                colors[0]
            } else {
                motion.read(cx).frame(now)
            };
            if !self.disabled && motion.read(cx).active(now) {
                window.request_animation_frame();
            }
            let color = |v: [f32; 4]| Rgba {
                r: v[0],
                g: v[1],
                b: v[2],
                a: v[3],
            };
            let background =
                linear_gradient(angle, linear_color_stop(color(stops[0]), 0.), linear_color_stop(color(stops[1]), 1.));
            if self.notched {
                outer_fill = Some(background);
            }
            let native_fill = if self.notched {
                rgba(0xffffff00).into()
            } else {
                background.into()
            };
            self.inner = self.inner.custom(
                ButtonCustomVariant::new(cx)
                    .backgrounds(native_fill, native_fill, native_fill)
                    .foreground(fg)
                    .hover_foreground(hover_fg),
            );
            if self.disabled && !self.action {
                self.inner = self.inner.bg(background).text_color(fg);
            }
            if !self.disabled {
                let hover_motion = motion.clone();
                let down_motion = motion.clone();
                let up_motion = motion.clone();
                let out_motion = motion.clone();
                let key_motion = motion.clone();
                self.inner = self
                    .inner
                    .on_hover(move |hovered, w, c| {
                        hover_motion.update(c, |m, _| m.event(Some(*hovered), None, Instant::now(), colors));
                        w.refresh();
                    })
                    .on_mouse_down(MouseButton::Left, move |_, w, c| {
                        down_motion.update(c, |m, _| m.event(None, Some(true), Instant::now(), colors));
                        w.refresh();
                    })
                    .on_mouse_up(MouseButton::Left, move |_, w, c| {
                        up_motion.update(c, |m, _| m.event(None, Some(false), Instant::now(), colors));
                        w.refresh();
                    })
                    .on_mouse_up_out(MouseButton::Left, move |_, w, c| {
                        out_motion.update(c, |m, _| m.event(None, Some(false), Instant::now(), colors));
                        w.refresh();
                    })
                    .on_key_down(move |e, w, c| {
                        if e.keystroke.key == "enter" || e.keystroke.key == "space" {
                            key_motion.update(c, |m, _| m.event(None, Some(true), Instant::now(), colors));
                            w.refresh();
                        }
                    })
                    .on_key_up(move |e, w, c| {
                        if e.keystroke.key == "enter" || e.keystroke.key == "space" {
                            motion.update(c, |m, _| m.event(None, Some(false), Instant::now(), colors));
                            w.refresh();
                        }
                    });
            }
        }
        if self.action && self.disabled {
            self.inner = self.inner.border_0().bg(rgba(0xffffff00)).text_color(rgb(theme::INK_DISABLED));
        }
        if let Some(fill) = outer_fill {
            notched_fill(fill.into()).flex().child(self.inner).into_any_element()
        } else {
            self.inner.into_any_element()
        }
    }
}

#[derive(Clone, Copy)]
pub enum ButtonAction {
    Secondary,
    Primary,
    Danger,
}

/// Native button interaction (keyboard, focus, disabled, click), with button A paint.
pub fn action_button(id: impl Into<ElementId>, label: impl Into<SharedString>, action: ButtonAction) -> DesignButton {
    let color = match action {
        ButtonAction::Secondary => rgb(theme::INK_MUTED),
        ButtonAction::Primary => rgb(theme::ACCENT_TEXT),
        ButtonAction::Danger => rgb(theme::DANGER),
    };
    let (from, to, hover_from, hover_to) = match action {
        ButtonAction::Secondary => (0xffffff00, 0xffffff00, 0xffffff0a, 0xffffff0a),
        ButtonAction::Primary => (0x123543ff, 0x0d2029ff, 0x174252ff, 0x0f2731ff),
        ButtonAction::Danger => (0x4a1919ff, 0x2e1010ff, 0x5a1e1eff, 0x361313ff),
    };
    let label = label.into();
    DesignButton::new(id)
        .ghost()
        .action()
        .aria_label(label.clone())
        .child(
            div()
                .font_family(theme::SANS)
                .text_size(px(13.))
                .line_height(px(16.))
                .font_weight(FontWeight::MEDIUM)
                .child(label),
        )
        .h(px(theme::BUTTON_H))
        .self_start()
        .px(px(16.))
        .rounded(px(0.))
        .font_family(theme::SANS)
        .font_weight(FontWeight::MEDIUM)
        .border_0()
        .when(!matches!(action, ButtonAction::Secondary), |b| b.border_l_2().border_color(color))
        .paint(
            90.,
            [
                [from, to],
                [hover_from, hover_to],
                [if matches!(action, ButtonAction::Secondary) {
                    0xffffff05
                } else {
                    to
                }; 2],
            ],
            color.into(),
            if matches!(action, ButtonAction::Secondary) {
                rgb(theme::INK).into()
            } else {
                color.into()
            },
        )
}

pub fn eyebrow(label: impl Into<SharedString>) -> Div {
    h_flex()
        .gap(px(7.))
        .items_center()
        .font_family(theme::DISPLAY)
        .text_size(px(10.))
        .line_height(px(14.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(theme::INK_DISABLED))
        .child(div().size(px(5.)).bg(rgb(theme::ACCENT_TEXT)))
        .child(TrackedText::new(label, 0.16))
}

pub fn setting_row(title: impl Into<SharedString>, help: impl Into<SharedString>, control: impl IntoElement) -> Div {
    h_flex()
        .w_full()
        .py(px(8.))
        .gap(px(20.))
        .items_center()
        .child(
            v_flex()
                .flex_1()
                .text_size(px(14.))
                .line_height(px(20.))
                .child(title.into())
                .child(div().text_size(px(12.)).line_height(px(17.)).text_color(rgb(0x6c6c74)).child(help.into())),
        )
        .child(control)
}

/// NO and SÍ set an explicit value. Choosing the active half is a no-op.
#[derive(IntoElement)]
pub struct YesNo {
    id: ElementId,
    checked: bool,
    disabled: bool,
    on_click: Option<Rc<dyn Fn(&bool, &mut Window, &mut App)>>,
}
impl YesNo {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            checked: false,
            disabled: false,
            on_click: None,
        }
    }
    pub fn checked(mut self, value: bool) -> Self {
        self.checked = value;
        self
    }
    pub fn on_click(mut self, f: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }
}
impl Disableable for YesNo {
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}
impl RenderOnce for YesNo {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        h_flex()
            .flex_shrink_0()
            .when(self.disabled, |d| d.opacity(0.45))
            .children([false, true].into_iter().map(|value| {
                let selected = value == self.checked;
                let handler = self.on_click.clone();
                DesignButton::new((self.id.clone(), if value { "yes" } else { "no" }))
                    .ghost()
                    .aria_label(if value { "Sí" } else { "No" })
                    .child(
                        div()
                            .font_family(theme::DISPLAY)
                            .text_size(px(11.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(TrackedText::new(if value { "SÍ" } else { "NO" }, 0.1)),
                    )
                    .h(px(26.))
                    .px(px(9.))
                    .rounded(px(0.))
                    .border_0()
                    .paint(
                        0.,
                        [[if selected { 0x0f2a35ff } else { 0xffffff08 }; 2]; 3],
                        if selected {
                            rgb(theme::ACCENT_TEXT).into()
                        } else {
                            rgb(0x5f5f67).into()
                        },
                        if selected {
                            rgb(theme::ACCENT_TEXT).into()
                        } else {
                            rgb(0x5f5f67).into()
                        },
                    )
                    .toggled(selected)
                    .disabled(self.disabled)
                    .on_click(move |_, window, cx| {
                        if !selected {
                            if let Some(f) = &handler {
                                f(&value, window, cx);
                            }
                        }
                    })
            }))
    }
}

pub fn chip(id: impl Into<ElementId>, label: impl Into<SharedString>, selected: bool) -> DesignButton {
    let label = label.into();
    DesignButton::new(id)
        .aria_label(label.clone())
        .ghost()
        .child(
            div()
                .font_family(theme::DISPLAY)
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(TrackedText::new(label, 0.04)),
        )
        .h(px(32.))
        .px(px(12.))
        .rounded(px(0.))
        .border_0()
        .toggled(selected)
        .paint(
            0.,
            [[if selected { 0x0f2a35ff } else { 0xffffff08 }; 2]; 3],
            if selected {
                rgb(theme::ACCENT_TEXT).into()
            } else {
                rgb(0x7a7a82).into()
            },
            if selected {
                rgb(theme::ACCENT_TEXT).into()
            } else {
                rgb(0x7a7a82).into()
            },
        )
}

pub fn tab(id: impl Into<ElementId>, label: impl Into<SharedString>, selected: bool) -> DesignButton {
    let label = label.into();
    DesignButton::new(id)
        .ghost()
        .aria_label(label.clone())
        .child(div().font_family(theme::SANS).text_size(px(13.)).line_height(px(18.)).child(label))
        .h(px(42.))
        .flex_shrink_0()
        .rounded(px(0.))
        .border_0()
        .when(selected, |b| b.border_b_2().border_color(rgb(theme::ACCENT_TEXT)))
        .paint(
            0.,
            [[0xffffff00; 2], [0xffffff0a; 2], [0xffffff05; 2]],
            if selected {
                rgb(theme::INK).into()
            } else {
                rgb(theme::INK_MUTED).into()
            },
            rgb(theme::INK).into(),
        )
}

pub fn dialog(dialog: gpui_component::dialog::Dialog) -> gpui_component::dialog::Dialog {
    dialog
        .w(px(416.))
        .close_button(false)
        .bg(rgba(theme::DIALOG_BG))
        .border_1()
        .border_color(rgba(theme::DIALOG_OUTLINE))
        .rounded(px(0.))
        .px(px(22.))
        .pt(px(20.))
        .pb(px(20.))
        .shadow(vec![BoxShadow {
            color: rgba(0x000000cc).into(),
            offset: point(px(0.), px(24.)),
            blur_radius: px(60.),
            spread_radius: px(0.),
            inset: false,
        }])
        .decoration(div().absolute().top_0().left_0().w(px(44.)).h(px(2.)).bg(rgb(theme::ACCENT_TEXT)))
}

/// A supplied texture represents an explicit surface behind this control; otherwise use the launcher backdrop.
/// Keeping texture preparation outside RenderOnce avoids decoding on each frame.
#[derive(IntoElement)]
pub struct PlayButton {
    progress: Option<f32>,
    id: ElementId,
    status: SharedString,
    title: SharedString,
    add_instance: bool,
    stop: bool,
    artwork: Option<ImageSource>,
    disabled: bool,
    tint: Option<u32>,
    on_click: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>>,
}
impl PlayButton {
    pub fn new(id: impl Into<ElementId>, status: impl Into<SharedString>, artwork: Option<ImageSource>) -> Self {
        Self {
            progress: None,
            id: id.into(),
            status: status.into(),
            title: "Jugar".into(),
            add_instance: false,
            stop: false,
            artwork,
            disabled: false,
            tint: None,
            on_click: None,
        }
    }
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = title.into();
        self
    }
    pub fn tint(mut self, color: u32) -> Self {
        self.tint = Some(color);
        self
    }
    pub fn stop_icon(mut self) -> Self {
        self.stop = true;
        self
    }
    pub fn progress(mut self, amount: f32) -> Self {
        self.progress = Some(amount.clamp(0., 1.));
        self
    }
    pub fn add_instance(mut self) -> Self {
        self.title = "Añadir instancia".into();
        self.add_instance = true;
        self
    }
    pub fn updating(mut self, amount: f32) -> Self {
        self.title = "Actualizando".into();
        self.progress = Some(amount.clamp(0., 1.));
        self
    }

    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }
}
impl Disableable for PlayButton {
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}
impl RenderOnce for PlayButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let backdrop = if !self.disabled && self.artwork.is_none() {
            Some(crate::background::glass(window, cx))
        } else {
            None
        };
        let disabled = self.disabled;
        let progress = self.progress;
        div()
            .relative()
            .w(px(theme::PLAY_MIN_W))
            .h(px(theme::PLAY_H))
            .when(disabled, |d| d.bg(rgb(theme::DISABLED_FILL)))
            .when(!disabled, |d| {
                d.when_some(backdrop, |d, backdrop| d.child(backdrop))
                    .when_some(self.artwork, |d, image| {
                        d.child(img(image).absolute().size_full().object_fit(ObjectFit::Cover))
                    })
            })
            .when_some(self.tint.filter(|_| !disabled), |d, tint| {
                d.child(div().absolute().size_full().bg(rgba(tint)))
            })
            .child(
                DesignButton::new(self.id)
                    .ghost()
                    .aria_label(self.title.clone())
                    .size_full()
                    .rounded(px(0.))
                    .px_0()
                    .border_1()
                    .border_color(if disabled {
                        rgba(0x1c1c20ff)
                    } else {
                        rgba(theme::PLAY_EDGE)
                    })
                    .paint(
                        180.,
                        if disabled {
                            [[0x111114ff; 2]; 3]
                        } else {
                            [
                                [theme::PLAY, theme::PLAY_PRESSED],
                                [theme::PLAY_HOVER, 0xffffff1f],
                                [0xffffff1f, 0xffffff0d],
                            ]
                        },
                        rgb(theme::ON_PLAY).into(),
                        rgb(theme::ON_PLAY).into(),
                    )
                    .disabled(disabled)
                    .when_some(self.on_click, |b, f| b.on_click(move |e, w, c| f(e, w, c)))
                    .child(
                        h_flex()
                            .gap(px(12.))
                            .items_center()
                            .text_color(if disabled {
                                rgb(theme::INK_DISABLED)
                            } else {
                                rgb(theme::ON_PLAY)
                            })
                            .child(div().size(px(18.)).text_size(px(18.)).line_height(px(18.)).child(
                                if self.add_instance {
                                    "+"
                                } else if self.stop {
                                    "■"
                                } else {
                                    "▶"
                                },
                            ))
                            .child(
                                v_flex()
                                    .items_start()
                                    .child(
                                        div()
                                            .font_family(theme::SANS)
                                            .text_size(px(17.))
                                            .line_height(px(20.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(TrackedText::new(self.title.clone(), 0.02)),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .line_height(px(13.))
                                            .text_color(if disabled {
                                                rgba(0x66666eff)
                                            } else {
                                                rgba(theme::ON_PLAY_MUTED)
                                            })
                                            .child(self.status),
                                    ),
                            ),
                    ),
            )
            .when_some(progress.filter(|_| !disabled), |d, amount| {
                d.child(
                    div()
                        .absolute()
                        .bottom_0()
                        .left_0()
                        .w_full()
                        .h(px(3.))
                        .bg(rgba(0xffffff14))
                        .child(div().h_full().w(relative(amount)).bg(rgb(0xffffff))),
                )
            })
            .when(!disabled, |d| {
                d.child(div().absolute().top_0().left_0().w_full().h(px(1.)).bg(rgba(theme::PLAY_HIGHLIGHT)))
            })
    }
}

/// CSS saturate(1.5), with the 14 logical px blur converted to source pixels.
pub fn prepare_play_artwork(crop: &image::DynamicImage, source_blur: f32) -> image::RgbaImage {
    let mut blurred = crop.blur(source_blur).to_rgba8();
    for pixel in blurred.pixels_mut() {
        let [r, g, b, a] = pixel.0;
        let luminance = 0.213 * r as f32 + 0.715 * g as f32 + 0.072 * b as f32;
        let [r, g, b] = [r, g, b].map(|c| (luminance + (c as f32 - luminance) * 1.5).round().clamp(0., 255.) as u8);
        pixel.0 = [r, g, b, a];
    }
    blurred
}

pub fn dialog_title(title: impl Into<SharedString>) -> Div {
    div()
        .font_family(theme::DISPLAY)
        .text_size(px(17.))
        .line_height(px(22.))
        .font_weight(FontWeight::BOLD)
        .child(title.into())
}

pub fn text_field(state: &Entity<gpui_component::input::InputState>) -> gpui_component::input::Input {
    gpui_component::input::Input::new(state)
        .h(px(theme::FIELD_H))
        .px(px(12.))
        .rounded(px(0.))
        .bg(rgb(theme::PANEL_SUNKEN))
        .text_size(px(14.))
}

pub fn progress(label: impl Into<SharedString>, amount: f32, detail: impl Into<SharedString>) -> Div {
    v_flex()
        .w_full()
        .gap(px(8.))
        .child(
            h_flex()
                .w_full()
                .justify_between()
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(rgb(theme::INK_MUTED))
                .child(label.into())
                .child(div().font_family(theme::DISPLAY).font_weight(FontWeight::MEDIUM).child(detail.into())),
        )
        .child(super::progress_bar::ProgressBar {
            amount,
            ..Default::default()
        })
}

/// A six-pixel block notch, painted beneath content on the UI thread.
pub fn notched_panel(color: u32) -> Div {
    notched_fill(rgba(color).into())
}
fn notched_fill(fill: Background) -> Div {
    div().relative().child(
        canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let x = bounds.origin.x;
                let y = bounds.origin.y;
                let w = bounds.size.width;
                let h = bounds.size.height;
                let n = px(6.);
                let mut path = PathBuilder::fill();
                path.move_to(point(x + n, y));
                path.line_to(point(x + w - n, y));
                path.line_to(point(x + w, y + n));
                path.line_to(point(x + w, y + h - n));
                path.line_to(point(x + w - n, y + h));
                path.line_to(point(x + n, y + h));
                path.line_to(point(x, y + h - n));
                path.line_to(point(x, y + n));
                path.close();
                if let Ok(path) = path.build() {
                    window.paint_path(path, fill);
                }
            },
        )
        .absolute()
        .size_full(),
    )
}

/// Main-shell heading shared by launcher destinations.
pub fn page_header(label: impl Into<SharedString>, window: &Window) -> Div {
    use super::generic_title_bar::WindowControl;
    let label = label.into();
    let stacked = matches!(label.as_ref(), "LAUNCHER" | "AÑADIR INSTANCIA" | "ARCHIVOS" | "EXPLORAR CONTENIDO");
    let title = div()
        .font_family(theme::DISPLAY)
        .text_size(px(11.))
        .line_height(px(14.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(0xd8d8dc))
        .when(stacked, |title| title.relative().top(px(4.)))
        .child(TrackedText::new(label, 0.16));
    let mut header = h_flex()
        .h(px(if stacked { 40. } else { 48. }))
        .flex_shrink_0()
        .px(px(20.))
        .bg(rgba(0x0b0b0ce6))
        .border_b_1()
        .border_color(rgba(0xffffff0b))
        .window_control_area(WindowControlArea::Drag)
        .child(title)
        .child(div().flex_1());
    if crate::root::should_render_custom_titlebar() && !cfg!(target_os = "macos") {
        header = header
            .child(WindowControl::Minimize)
            .child(if window.is_maximized() {
                WindowControl::Restore
            } else {
                WindowControl::Maximize
            })
            .child(WindowControl::Close);
    }
    header
}
