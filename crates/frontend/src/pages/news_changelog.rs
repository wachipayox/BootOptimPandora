use crate::{component::design, entity::DataEntities, theme};
use gpui::{prelude::*, *};
use gpui_component::{WindowExt, scroll::ScrollableElement, v_flex};
use schema::changelog::{ChangelogQuery, ProfileChangelog};

pub fn open(
    data: &DataEntities,
    profile_id: String,
    revision_id: String,
    date: String,
    window: &mut Window,
    cx: &mut App,
) {
    let (send, receive) = tokio::sync::oneshot::channel();
    data.backend_handle.send(bridge::message::MessageToBackend::GetProfileChangelog {
        profile_id,
        query: ChangelogQuery {
            revision: Some(revision_id),
            ..Default::default()
        },
        channel: send,
    });
    let view = cx.new(|_| ChangelogDialog { date, result: None });
    view.update(cx, |_, cx| {
        cx.spawn_in(window, async move |this, cx| {
            let result = receive.await.unwrap_or_else(|_| Err(String::new()));
            let _ = this.update(cx, |this, cx| {
                this.result = Some(result);
                cx.notify();
            });
        })
        .detach()
    });
    window.open_dialog(cx, move |dialog, _, _| {
        design::dialog(dialog).w(px(680.)).title("Changelog").child(view.clone())
    });
}
struct ChangelogDialog {
    date: String,
    result: Option<Result<ProfileChangelog, String>>,
}
impl Render for ChangelogDialog {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let mut content = v_flex().w_full().max_h(px(500.)).overflow_y_scrollbar().gap(px(16.)).text_size(px(13.));
        if let Ok(date) = chrono::DateTime::parse_from_rfc3339(&self.date) {
            content = content.child(div().text_color(rgb(theme::INK_MUTED)).child(date.format("%d/%m/%Y").to_string()));
        }
        match &self.result {
            Some(Err(_)) => {
                content = content.child(
                    div()
                        .w_full()
                        .py(px(24.))
                        .text_center()
                        .text_color(rgb(theme::INK_MUTED))
                        .child("No se ha podido cargar el changelog."),
                )
            },
            Some(Ok(result)) => {
                for entry in &result.entries {
                    let mut section = v_flex().gap(px(8.));
                    if let Some(revision) = &entry.revision {
                        section = section.child(
                            div()
                                .font_family(theme::DISPLAY)
                                .text_size(px(18.))
                                .child(format!("v{}", revision.sequence)),
                        );
                    }
                    if !entry.title.is_empty() {
                        section = section
                            .child(div().font_family(theme::DISPLAY).text_size(px(18.)).child(entry.title.clone()));
                    }
                    if !entry.description.is_empty() {
                        section = section.child(entry.description.clone());
                    }
                    for change in &entry.changes {
                        section = section.child(format!("• {change}"));
                    }
                    // Filenames and arrows are protocol data, not invented prose.
                    for added in &entry.mods.added {
                        section =
                            section.child(div().text_color(rgb(theme::SUCCESS)).child(format!("+ {}", added.file)));
                    }
                    for removed in &entry.mods.removed {
                        section =
                            section.child(div().text_color(rgb(theme::DANGER)).child(format!("− {}", removed.file)));
                    }
                    for update in &entry.mods.updated {
                        section = section.child(format!("{} → {}", update.from, update.to));
                    }
                    content = content.child(section);
                }
            },
            None => {},
        }
        content
    }
}
