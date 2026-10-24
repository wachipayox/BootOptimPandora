//! The update prompt shows every skipped cosmetic changelog in the signed check's exact range.
use crate::{
    component::design::{self, DesignButton},
    theme,
};
use bridge::{
    handle::BackendHandle,
    message::{InheritedGlobalUpdate, MessageToBackend},
};
use gpui::{prelude::*, *};
use gpui_component::{h_flex, scroll::ScrollableElement, v_flex};
use schema::changelog::{ChangelogEntry, ChangelogQuery};
use std::collections::BTreeSet;

pub struct UpdateChangelog {
    name: SharedString,
    entries: Vec<ChangelogEntry>,
    old: Option<u64>,
    new: Option<u64>,
    expanded: bool,
    failed: bool,
}
impl UpdateChangelog {
    pub fn new(
        backend: BackendHandle,
        update: Option<InheritedGlobalUpdate>,
        name: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        if let Some(update) = update {
            cx.spawn_in(window, async move |this, cx| {
                let (send, receive) = tokio::sync::oneshot::channel();
                backend.send(MessageToBackend::GetProfileChangelog {
                    profile_id: update.profile_id.clone(),
                    query: ChangelogQuery {
                        revision: Some(update.current_revision.clone()),
                        ..Default::default()
                    },
                    channel: send,
                });
                if let Ok(Ok(history)) = receive.await {
                    let old = history
                        .entries
                        .first()
                        .and_then(|entry| entry.revision.as_ref().map(|revision| revision.sequence));
                    if this
                        .update(cx, |this, cx| {
                            this.old = old;
                            cx.notify();
                        })
                        .is_err()
                    {
                        return;
                    }
                }
                let mut cursor = None;
                let mut visited = BTreeSet::new();
                loop {
                    let (send, receive) = tokio::sync::oneshot::channel();
                    backend.send(MessageToBackend::GetProfileChangelog {
                        profile_id: update.profile_id.clone(),
                        query: ChangelogQuery {
                            after: Some(update.current_revision.clone()),
                            until: Some(update.available_revision.clone()),
                            cursor: cursor.clone(),
                            ..Default::default()
                        },
                        channel: send,
                    });
                    match receive.await {
                        Ok(Ok(history)) => {
                            let next = history.next_cursor;
                            if this
                                .update(cx, |this, cx| {
                                    this.entries.extend(history.entries);
                                    this.new = this
                                        .entries
                                        .iter()
                                        .filter_map(|entry| entry.revision.as_ref().map(|revision| revision.sequence))
                                        .max();
                                    cx.notify();
                                })
                                .is_err()
                            {
                                return;
                            }
                            match next {
                                Some(next) if visited.insert(next.clone()) => cursor = Some(next),
                                _ => break,
                            }
                        },
                        _ => {
                            let _ = this.update(cx, |this, cx| {
                                this.failed = true;
                                cx.notify();
                            });
                            break;
                        },
                    }
                }
                log::info!("instance-update-changelog-ready pid={}", std::process::id());
            })
            .detach();
        } else {
            log::info!("instance-update-changelog-ready pid={}", std::process::id());
        }
        Self {
            name,
            entries: Vec::new(),
            old: None,
            new: None,
            expanded: std::env::var("WBOMBO_UI_CAPTURE_PAGE")
                .is_ok_and(|route| route.starts_with("update:agente-274-"))
                && std::env::var("WBOMBO_UI_CAPTURE_STATE").as_deref() == Ok("expanded"),
            failed: false,
        }
    }
}
impl Render for UpdateChangelog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut panel = v_flex().w_full().gap(px(14.)).text_size(px(13.)).line_height(px(20.));
        let mut heading = h_flex().gap(px(12.)).items_baseline().child(design::dialog_title("Revisión nueva"));
        if let (Some(old), Some(new)) = (self.old, self.new) {
            heading = heading.child(
                h_flex()
                    .gap(px(8.))
                    .font_family(theme::DISPLAY)
                    .text_size(px(13.))
                    .child(format!("v{old}"))
                    .child("→")
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(theme::ACCENT_TEXT))
                            .child(format!("v{new}")),
                    ),
            );
        }
        panel = panel.child(v_flex().gap(px(6.)).child(design::eyebrow(self.name.clone())).child(heading));
        if self.failed {
            return panel
                .child(
                    div()
                        .text_center()
                        .text_color(rgb(theme::INK_MUTED))
                        .child("No se ha podido cargar el changelog."),
                )
                .into_any_element();
        }
        if self.entries.is_empty() {
            return panel.into_any_element();
        }
        let mut entries = v_flex()
            .w_full()
            .gap(px(14.))
            .px(px(14.))
            .py(px(12.))
            .bg(rgba(0xffffff05))
            .text_size(px(12.5))
            .line_height(px(19.))
            .text_color(rgb(theme::INK_MUTED));
        for entry in &self.entries {
            let version = entry.revision.as_ref().map(|revision| revision.sequence).unwrap_or(entry.origin.sequence);
            let date = chrono::DateTime::parse_from_rfc3339(&entry.created_at)
                .map(|date| date.with_timezone(&chrono::Local).format("%d/%m/%Y").to_string())
                .unwrap_or_else(|_| entry.created_at.clone());
            let mut section = v_flex().gap(px(4.)).child(
                h_flex()
                    .justify_between()
                    .child(
                        div()
                            .font_family(theme::DISPLAY)
                            .text_color(rgb(theme::ACCENT_TEXT))
                            .child(format!("v{version}")),
                    )
                    .child(div().text_size(px(11.)).text_color(rgb(theme::INK_MUTED)).child(date)),
            );
            if !entry.title.is_empty() {
                section = section.child(entry.title.clone());
            }
            if !entry.description.is_empty() {
                section = section.child(entry.description.clone());
            }
            for change in &entry.changes {
                section = section.child(change.clone());
            }
            for added in &entry.mods.added {
                section = section.child(div().text_color(rgb(theme::SUCCESS)).child(format!("+ {}", added.file)));
            }
            for removed in &entry.mods.removed {
                section = section.child(div().text_color(rgb(theme::DANGER)).child(format!("− {}", removed.file)));
            }
            for changed in &entry.mods.updated {
                section = section.child(format!("{} → {}", changed.from, changed.to));
            }
            entries = entries.child(section);
        }
        let content = if self.expanded {
            div()
                .id("skipped-changelog-scroll")
                .max_h(px(320.))
                .overflow_y_scrollbar()
                .child(entries)
                .into_any_element()
        } else {
            div()
                .relative()
                .max_h(px(96.))
                .overflow_hidden()
                .child(entries)
                .child(div().absolute().bottom_0().left_0().w_full().h(px(40.)).bg(linear_gradient(
                    180.,
                    linear_color_stop(rgba(0x0e0e1100), 0.),
                    linear_color_stop(rgb(0x0e0e11), 1.),
                )))
                .into_any_element()
        };
        panel = panel.child(content);
        if !self.expanded {
            let count = self.entries.len();
            panel = panel.child(
                DesignButton::new("expand-skipped-revisions")
                    .aria_label(format!("Ver todo · {count} {} ↓", if count == 1 { "revisión" } else { "revisiones" }))
                    .h(px(24.))
                    .px_0()
                    .self_start()
                    .child(
                        div()
                            .font_family(theme::DISPLAY)
                            .text_size(px(12.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(theme::ACCENT_TEXT))
                            .child(format!(
                                "Ver todo · {count} {} ↓",
                                if count == 1 { "revisión" } else { "revisiones" }
                            )),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.expanded = true;
                        cx.notify();
                    })),
            );
        }
        panel.into_any_element()
    }
}
