//! Recommended/Manual memory controls. Manual values remain persisted when disabled.
use crate::{component::design, theme};
use bridge::{handle::BackendHandle, instance::InstanceID, message::MessageToBackend};
use gpui::{prelude::*, *};
use gpui_component::{
    h_flex,
    input::{InputEvent, InputState},
    v_flex,
};
use schema::instance::{InstanceMemoryConfiguration, MemoryRecommendation};

pub struct InstanceMemorySection {
    id: InstanceID,
    backend: BackendHandle,
    stored: InstanceMemoryConfiguration,
    had_manual: bool,
    recommendation: MemoryRecommendation,
    field: Entity<InputState>,
    displayed: String,
    invalid: bool,
}
fn gib(mib: u32) -> String {
    let v = mib as f64 / 1024.;
    if v.fract() == 0. {
        format!("{v:.0}")
    } else {
        format!("{v:.2}").trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}
impl InstanceMemorySection {
    pub fn new(
        id: InstanceID,
        memory: Option<InstanceMemoryConfiguration>,
        backend: BackendHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let stored = memory.unwrap_or_default();
        let recommendation = MemoryRecommendation::default();
        let displayed = gib(if stored.enabled {
            stored.max
        } else {
            recommendation.recommended_mib
        });
        let field = cx.new(|c| InputState::new(window, c).default_value(displayed.clone()));
        cx.subscribe(&field, |this, field, event: &InputEvent, c| {
            if !matches!(event, InputEvent::Change) {
                return;
            }
            let value = field.read(c).value().to_string();
            if value == this.displayed {
                return;
            }
            this.displayed = value.clone();
            this.invalid = true;
            if !this.stored.enabled {
                return;
            }
            if let Ok(gb) = value.replace(',', ".").parse::<f64>() {
                if gb.is_finite() && gb >= 0.125 && gb <= u32::MAX as f64 / 1024. {
                    this.invalid = false;
                    this.stored.max = (gb * 1024.).round() as u32;
                    this.stored.min = this.stored.min.min(this.stored.max);
                    this.had_manual = true;
                    this.persist();
                    c.notify();
                }
            }
            c.notify();
        })
        .detach();
        let (send, receive) = tokio::sync::oneshot::channel();
        backend.send(MessageToBackend::GetInstanceProfileInfo { id, channel: send });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(info)) = receive.await {
                let _ = this.update_in(cx, |this, w, c| {
                    this.recommendation = info.memory;
                    if !this.had_manual {
                        this.stored.max = info.memory.recommended_mib;
                    }
                    this.sync_field(w, c);
                    c.notify();
                    log::info!("instance-memory-ready pid={}", std::process::id());
                });
            }
        })
        .detach();
        Self {
            id,
            backend,
            stored,
            had_manual: memory.is_some(),
            recommendation,
            field,
            displayed,
            invalid: false,
        }
    }
    fn persist(&self) {
        self.backend.send(MessageToBackend::SetInstanceMemory {
            id: self.id,
            memory: self.stored,
        });
    }
    fn sync_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.invalid = false;
        self.displayed = gib(if self.stored.enabled {
            self.stored.max
        } else {
            self.recommendation.recommended_mib
        });
        let text = self.displayed.clone();
        self.field.update(cx, |s, c| s.set_value(text, window, c));
    }
}
impl Render for InstanceMemorySection {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let m = self.recommendation;
        let manual = self.stored.enabled;
        let mut modes = h_flex().gap(px(2.));
        for (ix, label, value) in [(0, "Recomendada", false), (1, "Manual", true)] {
            modes = modes.child(design::chip(("memory-mode", ix as usize), label, manual == value).on_click(
                cx.listener(move |this, _, w, c| {
                    if this.stored.enabled == value {
                        return;
                    }
                    this.stored.enabled = value;
                    this.had_manual = true;
                    this.persist();
                    this.sync_field(w, c);
                    c.notify();
                }),
            ));
        }
        let value = if manual { self.stored.max } else { m.recommended_mib };
        let total = m.total_mib.unwrap_or(m.maximum_mib as u64 * 2).max(1) as f32;
        let lower = (m.minimum_mib as f32 / total).clamp(0., 1.);
        let upper = (m.maximum_mib as f32 / total).clamp(lower, 1.);
        let bar = div()
            .relative()
            .flex_1()
            .h(px(4.))
            .bg(rgba(0xffffff10))
            .child(
                div()
                    .absolute()
                    .h(px(10.))
                    .top(px(-3.))
                    .left(relative(lower))
                    .w(relative(upper - lower))
                    .border_1()
                    .border_color(rgba(0x92d76a66))
                    .bg(rgba(0x92d76a18)),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .h_full()
                    .w(relative((value as f32 / total).clamp(0., 1.)))
                    .bg(rgb(theme::ACCENT_TEXT)),
            );
        let mut row = h_flex()
            .gap(px(14.))
            .child(
                design::text_field(&self.field)
                    .w(px(92.))
                    .disabled(!manual)
                    .suffix(div().text_size(px(12.)).text_color(rgb(theme::INK_MUTED)).child("GB")),
            )
            .child(bar);
        if let Some(total) = m.total_mib {
            row = row.child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(theme::INK_MUTED))
                    .child(format!("de {} GB", gib(total.min(u32::MAX as u64) as u32))),
            );
        }
        v_flex()
            .gap(px(12.))
            .child(h_flex().justify_between().child("Memoria").child(modes))
            .child(row)
            .when(manual && self.invalid, |d| {
                d.child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgb(theme::DANGER))
                        .child("Introduce una cantidad de memoria válida."),
                )
            })
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(18.))
                    .text_color(rgb(theme::INK_MUTED))
                    .child(format!(
                        "Recomendada para tu equipo: {} GB (entre {} y {}).",
                        gib(m.recommended_mib),
                        gib(m.minimum_mib),
                        gib(m.maximum_mib)
                    )),
            )
    }
}
