//! Instance artwork uses the same resolved presentation as Home.
use crate::{component::design, entity::instance::InstanceEntry, theme};
use bridge::{handle::BackendHandle, home::HomePresentation, message::MessageToBackend};
use gpui::{prelude::*, *};
use gpui_component::{h_flex, v_flex};
use std::{path::PathBuf, sync::Arc};

pub struct InstanceArtworkSection {
    instance: Entity<InstanceEntry>,
    backend: BackendHandle,
    presentation: HomePresentation,
    profile_name: Option<String>,
    error: bool,
    pending: bool,
}
impl InstanceArtworkSection {
    pub fn new(
        instance: Entity<InstanceEntry>,
        backend: BackendHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&instance, |this, _, cx| {
            this.refresh(cx);
        })
        .detach();
        let mut this = Self {
            instance,
            backend,
            presentation: HomePresentation::default(),
            profile_name: None,
            error: false,
            pending: false,
        };
        this.refresh(cx);
        let _ = window;
        this
    }
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let (send, receive) = tokio::sync::oneshot::channel();
        self.backend.send(MessageToBackend::GetHomePresentation {
            id: Some(self.instance.read(cx).id),
            channel: send,
        });
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(presentation)) = receive.await {
                let profile = presentation.profile_id.clone();
                let _ = this.update(cx, |this, cx| {
                    this.presentation = presentation;
                    cx.notify();
                });
                if let Some(profile) = profile {
                    let (send, receive) = tokio::sync::oneshot::channel();
                    backend.send(MessageToBackend::GetGlobalProfiles { channel: send });
                    if let Ok(Ok(profiles)) = receive.await {
                        let name = profiles.into_iter().find(|p| p.profile_id == profile).map(|p| p.name);
                        let _ = this.update(cx, |this, cx| {
                            this.profile_name = name;
                            cx.notify();
                        });
                    }
                }
                log::info!("instance-artwork-ready pid={}", std::process::id());
            }
        })
        .detach();
    }
    fn save(&mut self, path: Option<PathBuf>, cx: &mut Context<Self>) {
        self.pending = true;
        self.error = false;
        let (send, receive) = tokio::sync::oneshot::channel();
        self.backend.send(MessageToBackend::SetInstanceHomeArtwork {
            id: self.instance.read(cx).id,
            path,
            channel: send,
        });
        cx.spawn(async move |this, cx| {
            let success = matches!(receive.await, Ok(Ok(())));
            let _ = this.update(cx, |this, cx| {
                this.pending = false;
                this.error = !success;
                if success {
                    this.refresh(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }
}
impl Render for InstanceArtworkSection {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_component::Disableable;
        let own = self.instance.read(cx).configuration.home_artwork.clone();
        let image = self
            .presentation
            .artwork
            .as_ref()
            .map(|p| ImageSource::Resource(Resource::Path(Arc::from(p.as_path()))))
            .unwrap_or_else(|| ImageSource::Resource(Resource::Embedded("images/artwork-vanilla.png".into())));
        let origin = if own.is_some() {
            "De esta instancia".to_owned()
        } else if self.presentation.artwork.is_some() {
            self.profile_name.as_ref().map(|n| format!("Del perfil {n}")).unwrap_or_default()
        } else {
            "Vanilla".to_owned()
        };
        v_flex()
            .gap(px(12.))
            .child(img(image).w(px(240.)).h(px(135.)).object_fit(ObjectFit::Cover))
            .child(div().text_size(px(12.)).text_color(rgb(theme::INK_MUTED)).child(origin))
            .when_some(
                own.as_ref().and_then(|p| p.file_name()).map(|p| p.to_string_lossy().into_owned()),
                |d, name| d.child(div().text_size(px(11.)).text_color(rgb(theme::INK_MUTED)).child(name)),
            )
            .child(
                h_flex()
                    .gap(px(8.))
                    .child(
                        design::DesignButton::new("choose-artwork")
                            .label("Elegir imagen…")
                            .disabled(self.pending)
                            .on_click(cx.listener(|_this, _, _, cx| {
                                let selected = cx.prompt_for_paths(PathPromptOptions {
                                    files: true,
                                    directories: false,
                                    multiple: false,
                                    prompt: Some("Elegir imagen…".into()),
                                });
                                cx.spawn(async move |this, cx| match selected.await {
                                    Ok(Ok(Some(paths))) => {
                                        if let Some(path) = paths.into_iter().next() {
                                            let _ = this.update(cx, |this, cx| this.save(Some(path), cx));
                                        }
                                    },
                                    Ok(Ok(None)) => {},
                                    _ => {
                                        let _ = this.update(cx, |this, cx| {
                                            this.error = true;
                                            cx.notify();
                                        });
                                    },
                                })
                                .detach();
                            })),
                    )
                    .child(
                        design::DesignButton::new("reset-artwork")
                            .label("Restablecer")
                            .disabled(self.pending || own.is_none())
                            .on_click(cx.listener(|this, _, _, cx| this.save(None, cx))),
                    ),
            )
            .when(self.error, |d| {
                d.child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgb(theme::DANGER))
                        .child("No se ha podido abrir la imagen."),
                )
            })
    }
}
