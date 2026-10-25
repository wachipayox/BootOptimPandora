use super::content_subpage::ContentType;
use crate::{
    component::{
        design,
        generic_title_bar::{TitleBarState, WindowControl},
        tracked_text::TrackedText,
    },
    entity::{DataEntities, instance::InstanceEntry},
    game_output::GameOutputRoot,
    interface_config::InterfaceConfig,
    pages::instance::{
        content_subpage::InstanceContentSubpage, logs_subpage::InstanceLogsSubpage,
        overwrites_subpage::InstanceOverwritesSubpage, quickplay_subpage::InstanceQuickplaySubpage,
        settings_subpage::InstanceSettingsSubpage,
    },
    root, theme,
    ui::PageType,
};
use bridge::{handle::BackendHandle, instance::InstanceID};
use gpui::{prelude::*, *};
use gpui_component::{Disableable, h_flex, v_flex};
use serde::{Deserialize, Serialize};

pub struct InstancePage {
    data: DataEntities,
    pub instance: Entity<InstanceEntry>,
    subpage: InstanceSubpage,
    overwrites_subpage: Option<Entity<InstanceOverwritesSubpage>>,
    has_global_history: bool,
}
impl InstancePage {
    pub fn new(instance_id: InstanceID, data: &DataEntities, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let instance = data.instances.read(cx).entries.get(&instance_id).unwrap().clone();
        let desired = InterfaceConfig::get(cx).instance_subpage;
        let subpage = desired
            .create(&instance, data, data.backend_handle.clone(), window, cx)
            .unwrap_or_else(|| {
                InterfaceConfig::get_mut(cx).instance_subpage = InstanceSubpageType::Mods;
                InstanceSubpageType::Mods
                    .create(&instance, data, data.backend_handle.clone(), window, cx)
                    .unwrap()
            });
        let overwrites_subpage = match &subpage {
            InstanceSubpage::Overwrites(p) => Some(p.clone()),
            _ => None,
        };
        let (send, receive) = tokio::sync::oneshot::channel();
        data.backend_handle.send(bridge::message::MessageToBackend::GetInstanceProfileInfo {
            id: instance_id,
            channel: send,
        });
        cx.spawn(async move |this, cx| {
            let global = matches!(receive.await, Ok(Ok(info)) if info.global_profile_id.is_some());
            let _ = this.update(cx, |this, cx| {
                this.has_global_history = global;
                if !global && InterfaceConfig::get(cx).instance_subpage == InstanceSubpageType::Changelog {
                    InterfaceConfig::get_mut(cx).instance_subpage = InstanceSubpageType::Mods;
                }
                cx.notify();
            });
        })
        .detach();
        Self {
            has_global_history: false,
            data: data.clone(),
            instance,
            subpage,
            overwrites_subpage,
        }
    }
    fn header(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let name = self.instance.read(cx).name.clone();
        let selected = self.subpage.page_type();
        let state = window.use_keyed_state("instance-titlebar-state", cx, |_, _| TitleBarState::default());
        let mut header = h_flex()
            .id("instance-header")
            .w_full()
            .h(px(48.))
            .flex_shrink_0()
            .px(px(24.))
            .gap(px(20.))
            .bg(rgba(0x0b0b0ce6))
            .border_b_1()
            .border_color(rgba(0xffffff0b))
            .window_control_area(WindowControlArea::Drag)
            .on_mouse_down_out(window.listener_for(&state, |s, _, _, _| s.should_move = false))
            .on_mouse_down(
                MouseButton::Left,
                window.listener_for(&state, |s, _, w, _| s.should_move = !w.default_prevented()),
            )
            .on_mouse_up(MouseButton::Left, window.listener_for(&state, |s, _, _, _| s.should_move = false))
            .on_mouse_move(window.listener_for(&state, |s, _, w, _| {
                if s.should_move {
                    s.should_move = false;
                    w.start_window_move();
                }
            }))
            .child(
                div()
                    .font_family(theme::DISPLAY)
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(theme::INK))
                    .child(TrackedText::new(name.to_uppercase(), 0.16)),
            );
        let content = matches!(
            selected,
            InstanceSubpageType::Mods | InstanceSubpageType::ResourcePacks | InstanceSubpageType::Shaders
        );
        let logs = matches!(selected, InstanceSubpageType::Logs | InstanceSubpageType::LiveGameOutput);
        for (ix, label, target, active) in [
            (0, "Jugar", InstanceSubpageType::Quickplay, false),
            (1, "Contenido", InstanceSubpageType::Mods, content),
            (
                2,
                "Sobrescrituras",
                InstanceSubpageType::Overwrites,
                selected == InstanceSubpageType::Overwrites,
            ),
            (3, "Registros", InstanceSubpageType::Logs, logs),
            (5, "Changelog", InstanceSubpageType::Changelog, selected == InstanceSubpageType::Changelog),
            (
                4,
                "Ajustes de la instancia",
                InstanceSubpageType::Settings,
                selected == InstanceSubpageType::Settings,
            ),
        ] {
            if target == InstanceSubpageType::Changelog && !self.has_global_history {
                continue;
            }
            let name = name.clone();
            header = header.child(design::tab(("instance-tab", ix as usize), label, active).on_click(cx.listener(
                move |_, _, w, c| {
                    if target == InstanceSubpageType::Quickplay {
                        InterfaceConfig::get_mut(c).home_instance = Some(name.clone());
                        root::switch_page(PageType::Instances, &[], w, c);
                    } else {
                        InterfaceConfig::get_mut(c).instance_subpage = target;
                        c.notify();
                    }
                },
            )));
        }
        let mut controls = h_flex().child(crate::launcher_updates::indicator(cx));
        if root::should_render_custom_titlebar() && !cfg!(target_os = "macos") {
            controls = controls
                .child(WindowControl::Minimize)
                .child(if window.is_maximized() {
                    WindowControl::Restore
                } else {
                    WindowControl::Maximize
                })
                .child(WindowControl::Close);
        }
        header = header.child(div().flex_1()).child(controls);
        header.into_any_element()
    }
}
impl Render for InstancePage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let updating = root::inherited_operation_active(self.instance.read(cx).id);
        let desired = InterfaceConfig::get(cx).instance_subpage;
        // Play and update progress belong to Home. Defer the navigation until
        // this entity's render borrow has ended.
        if updating || desired == InstanceSubpageType::Quickplay {
            let name = self.instance.read(cx).name.clone();
            let handle = window.window_handle();
            cx.defer(move |c| {
                let _ = handle.update(c, |_, w, c| {
                    InterfaceConfig::get_mut(c).home_instance = Some(name);
                    root::switch_page(PageType::Instances, &[], w, c);
                });
            });
            return div().size_full().into_any_element();
        }
        if desired != self.subpage.page_type() {
            let page = if desired == InstanceSubpageType::Overwrites {
                self.overwrites_subpage
                    .clone()
                    .map(|p| {
                        p.update(cx, |p, c| p.on_reenter(window, c));
                        InstanceSubpage::Overwrites(p)
                    })
                    .or_else(|| {
                        desired.create(&self.instance, &self.data, self.data.backend_handle.clone(), window, cx)
                    })
            } else {
                desired.create(&self.instance, &self.data, self.data.backend_handle.clone(), window, cx)
            };
            if let Some(page) = page {
                self.subpage = page;
                if let InstanceSubpage::Overwrites(p) = &self.subpage {
                    self.overwrites_subpage = Some(p.clone());
                }
            } else {
                InterfaceConfig::get_mut(cx).instance_subpage = InstanceSubpageType::Logs;
            }
        }
        if let InstanceSubpage::LiveGameOutput(current) = &self.subpage {
            if let Some(output) = &self.instance.read(cx).live_game_output {
                if current != output {
                    self.subpage = InstanceSubpage::LiveGameOutput(output.clone());
                }
            }
        }
        let mut body = v_flex()
            .size_full()
            .text_size(px(13.))
            .text_color(rgb(theme::INK))
            .font_family(theme::SANS)
            .child(self.header(window, cx));
        if matches!(self.subpage, InstanceSubpage::Logs(_) | InstanceSubpage::LiveGameOutput(_)) {
            let live = self.instance.read(cx).live_game_output.is_some();
            let selected = self.subpage.page_type();
            let mut tabs = h_flex().px(px(24.)).py(px(16.));
            for (ix, label, target) in [
                (0, "Registros", InstanceSubpageType::Logs),
                (1, "Salida en vivo", InstanceSubpageType::LiveGameOutput),
            ] {
                tabs = tabs.child(
                    design::chip(("instance-log-mode", ix as usize), label, selected == target)
                        .disabled(target == InstanceSubpageType::LiveGameOutput && !live)
                        .on_click(cx.listener(move |_, _, _, c| {
                            InterfaceConfig::get_mut(c).instance_subpage = target;
                            c.notify();
                        })),
                );
            }
            body = body.child(tabs);
        }
        body.child(div().flex_1().min_h_0().child(self.subpage.clone().into_any_element()))
            .into_any_element()
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstanceSubpageType {
    #[default]
    Quickplay,
    Logs,
    Mods,
    ResourcePacks,
    Shaders,
    Overwrites,
    Changelog,
    Settings,
    LiveGameOutput,
}

impl InstanceSubpageType {
    pub fn create(
        self,
        instance: &Entity<InstanceEntry>,
        data: &DataEntities,
        backend_handle: BackendHandle,
        window: &mut gpui::Window,
        cx: &mut App,
    ) -> Option<InstanceSubpage> {
        Some(match self {
            InstanceSubpageType::Quickplay => {
                InstanceSubpage::Quickplay(cx.new(|cx| InstanceQuickplaySubpage::new(instance, data, window, cx)))
            },
            InstanceSubpageType::Logs => {
                InstanceSubpage::Logs(cx.new(|cx| InstanceLogsSubpage::new(instance, backend_handle, window, cx)))
            },
            InstanceSubpageType::Mods => {
                InstanceSubpage::Mods(cx.new(|cx| {
                    InstanceContentSubpage::new(instance, ContentType::Mods, data, backend_handle, window, cx)
                }))
            },
            InstanceSubpageType::ResourcePacks => InstanceSubpage::ResourcePacks(cx.new(|cx| {
                InstanceContentSubpage::new(instance, ContentType::ResourcePacks, data, backend_handle, window, cx)
            })),
            InstanceSubpageType::Shaders => InstanceSubpage::Shaders(cx.new(|cx| {
                InstanceContentSubpage::new(instance, ContentType::Shaders, data, backend_handle, window, cx)
            })),
            InstanceSubpageType::Overwrites => InstanceSubpage::Overwrites(
                cx.new(|cx| InstanceOverwritesSubpage::new(instance, backend_handle, window, cx)),
            ),
            InstanceSubpageType::Changelog => InstanceSubpage::Changelog(cx.new(|cx| {
                super::changelog_subpage::InstanceChangelogSubpage::new(
                    instance.read(cx).id,
                    backend_handle,
                    window,
                    cx,
                )
            })),
            InstanceSubpageType::Settings => InstanceSubpage::Settings(
                cx.new(|cx| InstanceSettingsSubpage::new(instance, data, backend_handle, window, cx)),
            ),
            InstanceSubpageType::LiveGameOutput => {
                if let Some(game_output) = instance.read(cx).live_game_output.clone() {
                    InstanceSubpage::LiveGameOutput(game_output)
                } else {
                    return None;
                }
            },
        })
    }
}

#[derive(Clone)]
pub enum InstanceSubpage {
    Quickplay(Entity<InstanceQuickplaySubpage>),
    Logs(Entity<InstanceLogsSubpage>),
    Mods(Entity<InstanceContentSubpage>),
    ResourcePacks(Entity<InstanceContentSubpage>),
    Shaders(Entity<InstanceContentSubpage>),
    Overwrites(Entity<InstanceOverwritesSubpage>),
    Settings(Entity<InstanceSettingsSubpage>),
    Changelog(Entity<super::changelog_subpage::InstanceChangelogSubpage>),
    LiveGameOutput(Entity<GameOutputRoot>),
}

impl InstanceSubpage {
    pub fn page_type(&self) -> InstanceSubpageType {
        match self {
            InstanceSubpage::Quickplay(_) => InstanceSubpageType::Quickplay,
            InstanceSubpage::Logs(_) => InstanceSubpageType::Logs,
            InstanceSubpage::Mods(_) => InstanceSubpageType::Mods,
            InstanceSubpage::ResourcePacks(_) => InstanceSubpageType::ResourcePacks,
            InstanceSubpage::Shaders(_) => InstanceSubpageType::Shaders,
            InstanceSubpage::Overwrites(_) => InstanceSubpageType::Overwrites,
            InstanceSubpage::Settings(_) => InstanceSubpageType::Settings,
            InstanceSubpage::Changelog(_) => InstanceSubpageType::Changelog,
            InstanceSubpage::LiveGameOutput(_) => InstanceSubpageType::LiveGameOutput,
        }
    }

    pub fn into_any_element(self) -> AnyElement {
        match self {
            Self::Quickplay(entity) => entity.into_any_element(),
            Self::Logs(entity) => entity.into_any_element(),
            Self::Mods(entity) => entity.into_any_element(),
            Self::ResourcePacks(entity) => entity.into_any_element(),
            Self::Shaders(entity) => entity.into_any_element(),
            Self::Overwrites(entity) => entity.into_any_element(),
            Self::Settings(entity) => entity.into_any_element(),
            Self::Changelog(entity) => entity.into_any_element(),
            Self::LiveGameOutput(entity) => entity.into_any_element(),
        }
    }
}
