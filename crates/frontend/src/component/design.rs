//! Shared controls for the approved Wachiland design.
use crate::theme;
use gpui::{prelude::*, *};
use gpui_component::{
    Disableable, StyledExt,
    button::{Button, ButtonCustomVariant, ButtonVariants},
    h_flex, v_flex,
};
use std::rc::Rc;

/// Adds state styling to the native button without changing its input handling.
#[derive(IntoElement)]
pub struct DesignButton {
    inner: Button,
    paint: Option<([Background; 3], Hsla, Hsla)>,
    action: bool,
    disabled: bool,
}
impl DesignButton {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            inner: Button::new(id),
            paint: None,
            action: false,
            disabled: false,
        }
    }
    pub fn action(mut self) -> Self {
        self.action = true;
        self
    }
    pub fn paint(mut self, backgrounds: [Background; 3], foreground: Hsla, hover_foreground: Hsla) -> Self {
        self.paint = Some((backgrounds, foreground, hover_foreground));
        self
    }
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.inner = self.inner.label(label);
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
impl ButtonVariants for DesignButton {
    fn with_variant(mut self, variant: gpui_component::button::ButtonVariant) -> Self {
        self.inner = self.inner.with_variant(variant);
        self
    }
}
impl RenderOnce for DesignButton {
    fn render(mut self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        if let Some((b, fg, hover_fg)) = self.paint {
            self.inner = self.inner.custom(
                ButtonCustomVariant::new(cx)
                    .backgrounds(b[0], b[1], b[2])
                    .foreground(fg)
                    .hover_foreground(hover_fg),
            );
        }
        if self.action && self.disabled {
            self.inner = self.inner.border_0().bg(rgba(0xffffff00)).text_color(rgb(theme::INK_DISABLED));
        }
        self.inner
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
        .px(px(16.))
        .rounded(px(0.))
        .font_family(theme::SANS)
        .font_weight(FontWeight::MEDIUM)
        .border_0()
        .when(!matches!(action, ButtonAction::Secondary), |b| b.border_l_2().border_color(color))
        .paint(
            [
                linear_gradient(90., linear_color_stop(rgba(from), 0.), linear_color_stop(rgba(to), 1.)),
                linear_gradient(90., linear_color_stop(rgba(hover_from), 0.), linear_color_stop(rgba(hover_to), 1.)),
                rgba(if matches!(action, ButtonAction::Secondary) {
                    0xffffff05
                } else {
                    to
                })
                .into(),
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
        .child(label.into())
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
                    .child(
                        div()
                            .font_family(theme::DISPLAY)
                            .text_size(px(11.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(if value { "SÍ" } else { "NO" }),
                    )
                    .h(px(26.))
                    .px(px(9.))
                    .rounded(px(0.))
                    .border_0()
                    .bg(if selected { rgba(0x0f2a35ff) } else { rgba(0xffffff08) })
                    .text_color(if selected {
                        rgb(theme::ACCENT_TEXT)
                    } else {
                        rgb(0x5f5f67)
                    })
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
    DesignButton::new(id)
        .ghost()
        .child(
            div()
                .font_family(theme::DISPLAY)
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(label.into()),
        )
        .h(px(32.))
        .px(px(12.))
        .rounded(px(0.))
        .border_0()
        .toggled(selected)
        .bg(if selected { rgba(0x0f2a35ff) } else { rgba(0xffffff08) })
        .text_color(if selected {
            rgb(theme::ACCENT_TEXT)
        } else {
            rgb(0x7a7a82)
        })
}

pub fn tab(id: impl Into<ElementId>, label: impl Into<SharedString>, selected: bool) -> DesignButton {
    DesignButton::new(id)
        .ghost()
        .label(label)
        .h(px(42.))
        .rounded(px(0.))
        .border_0()
        .when(selected, |b| b.border_b_2().border_color(rgb(theme::ACCENT_TEXT)))
        .text_color(if selected {
            rgb(theme::INK)
        } else {
            rgb(theme::INK_MUTED)
        })
}

pub fn dialog(dialog: gpui_component::dialog::Dialog) -> gpui_component::dialog::Dialog {
    dialog
        .w(px(416.))
        .bg(rgba(theme::DIALOG_BG))
        .border_1()
        .border_color(rgba(theme::DIALOG_OUTLINE))
        .rounded(px(0.))
        .px(px(22.))
        .pt(px(20.))
        .pb(px(20.))
        .child(div().absolute().top_0().left_0().w(px(44.)).h(px(2.)).bg(rgb(theme::ACCENT_TEXT)))
}

/// The caller supplies the cached, blurred crop of the artwork behind this control.
/// Keeping texture preparation outside RenderOnce avoids decoding on each frame.
#[derive(IntoElement)]
pub struct PlayButton {
    id: ElementId,
    status: SharedString,
    artwork: Option<ImageSource>,
    disabled: bool,
    on_click: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>>,
}
impl PlayButton {
    pub fn new(id: impl Into<ElementId>, status: impl Into<SharedString>, artwork: Option<ImageSource>) -> Self {
        Self {
            id: id.into(),
            status: status.into(),
            artwork,
            disabled: false,
            on_click: None,
        }
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
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let disabled = self.disabled;
        div()
            .relative()
            .w(px(theme::PLAY_MIN_W))
            .h(px(theme::PLAY_H))
            .when(disabled, |d| d.bg(rgb(theme::DISABLED_FILL)))
            .when(!disabled, |d| {
                d.when_some(self.artwork, |d, image| {
                    d.child(img(image).absolute().size_full().object_fit(ObjectFit::Cover))
                })
            })
            .child(
                DesignButton::new(self.id)
                    .ghost()
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
                        if disabled {
                            [rgba(0x111114ff).into(); 3]
                        } else {
                            [
                                linear_gradient(
                                    180.,
                                    linear_color_stop(rgba(theme::PLAY), 0.),
                                    linear_color_stop(rgba(theme::PLAY_PRESSED), 1.),
                                ),
                                linear_gradient(
                                    180.,
                                    linear_color_stop(rgba(theme::PLAY_HOVER), 0.),
                                    linear_color_stop(rgba(0xffffff1f), 1.),
                                ),
                                linear_gradient(
                                    180.,
                                    linear_color_stop(rgba(0xffffff1f), 0.),
                                    linear_color_stop(rgba(0xffffff0d), 1.),
                                ),
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
                            .child(div().text_size(px(18.)).child("▶"))
                            .child(
                                v_flex()
                                    .items_start()
                                    .child(
                                        div()
                                            .font_family(theme::SANS)
                                            .text_size(px(17.))
                                            .line_height(px(20.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child("Jugar"),
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
            .when(!disabled, |d| {
                d.child(div().absolute().top_0().left_0().w_full().h(px(1.)).bg(rgba(theme::PLAY_HIGHLIGHT)))
            })
    }
}

/// CSS saturate(1.5) after the specified 14 px Gaussian blur, cached by the caller.
pub fn prepare_play_artwork(crop: &image::DynamicImage) -> image::RgbaImage {
    let mut blurred = crop.blur(14.).to_rgba8();
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
