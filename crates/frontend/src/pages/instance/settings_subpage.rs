#[cfg(target_os = "linux")]
use schema::instance::InstanceLinuxWrapperConfiguration;
use std::{path::Path, sync::Arc};

use bridge::{
    handle::BackendHandle,
    instance::InstanceID,
    instance::InstanceStatus,
    message::{AotCacheSummary, EmbeddedOrRaw, MessageToBackend},
    meta::MetadataRequest,
};
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme as _, Disableable, Icon, IndexPath, WindowExt,
    button::ButtonVariants,
    h_flex,
    input::{InputEvent, InputState, Textarea, TextareaState},
    notification::{Notification, NotificationType},
    scroll::ScrollableElement,
    select::{SearchableVec, Select, SelectEvent, SelectState},
    skeleton::Skeleton,
    v_flex,
};
use schema::{
    fabric_loader_manifest::FabricLoaderManifest,
    forge::{ForgeMavenManifest, NeoforgeMavenManifest},
    instance::{
        AUTO_LIBRARY_PATH_GLFW, AUTO_LIBRARY_PATH_OPENAL, InstanceJvmBinaryConfiguration,
        InstanceJvmFlagsConfiguration, InstanceSystemLibrariesConfiguration, InstanceWrapperCommandConfiguration,
        LwjglLibraryPath,
    },
    loader::Loader,
    version_manifest::MinecraftVersionManifest,
};
use strum::IntoEnumIterator;
use uuid::Uuid;

use crate::{
    component::{
        design::{DesignButton, YesNo},
        named_dropdown::{DropdownName, NamedDropdown, NamedDropdownItem},
        path_label::PathLabel,
    },
    entity::{
        DataEntities,
        account::{AccountEntries, AccountExt},
        instance::InstanceEntry,
        metadata::{
            AsMetadataResult, FrontendMetadata, FrontendMetadataResult, FrontendMetadataState,
            TypelessFrontendMetadataResult,
        },
    },
    icon::PandoraIcon,
    interface_config::InterfaceConfig,
    pages::instances_page::VersionList,
    png_render_cache, theme,
};

#[derive(PartialEq, Eq)]
enum NewNameChangeState {
    NoChange,
    InvalidName,
    Pending,
}

pub struct InstanceSettingsSubpage {
    data: DataEntities,
    instance: Entity<InstanceEntry>,
    instance_id: InstanceID,
    new_name_input_state: Entity<InputState>,
    version_state: TypelessFrontendMetadataResult,
    version_select_state: Entity<SelectState<VersionList>>,
    account_items: Entity<SelectState<NamedDropdown<Uuid>>>,
    loader: Loader,
    loader_select_state: Entity<SelectState<Vec<&'static str>>>,
    loader_versions_state: TypelessFrontendMetadataResult,
    loader_version_select_state: Entity<SelectState<SearchableVec<&'static str>>>,
    loader_version_latest_string: &'static str,
    memory_section: Entity<super::memory_section::InstanceMemorySection>,
    artwork_section: Entity<super::artwork_section::InstanceArtworkSection>,
    profile_info: Option<bridge::instance::InstanceProfileInfo>,
    profile_name: Option<String>,
    profile_sequence: Option<u64>,
    save_group: Option<bridge::message::SaveGroupSummary>,
    show_advanced: bool,
    editing_version: bool,
    wrapper_command_enabled: bool,
    wrapper_command_input_state: Entity<TextareaState>,
    jvm_flags_enabled: bool,
    jvm_flags_input_state: Entity<TextareaState>,
    profile_jvm_section: Entity<super::profile_jvm_section::ProfileJvmSection>,
    jvm_binary_enabled: bool,
    jvm_binary_path: Option<PathLabel>,

    instance_root_label: PathLabel,

    override_glfw_enabled: bool,
    override_glfw_path: Option<PathLabel>,
    override_openal_enabled: bool,
    override_openal_path: Option<PathLabel>,

    #[cfg(target_os = "linux")]
    use_mangohud: bool,
    #[cfg(target_os = "linux")]
    use_gamemode: bool,
    #[cfg(target_os = "linux")]
    use_discrete_gpu: bool,
    #[cfg(target_os = "linux")]
    disable_gl_threaded_optimizations: bool,
    #[cfg(target_os = "linux")]
    mangohud_available: bool,
    #[cfg(target_os = "linux")]
    gamemode_available: bool,
    new_name_change_state: NewNameChangeState,
    icon: Option<EmbeddedOrRaw>,
    backend_handle: BackendHandle,
    _observe_minecraft_version_subscription: Option<Subscription>,
    _minecraft_version_retry_task: Task<()>,
    _observe_loader_version_subscription: Option<Subscription>,
    _loader_version_retry_task: Task<()>,
    _select_file_task: Task<()>,
}

impl InstanceSettingsSubpage {
    pub fn new(
        instance: &Entity<InstanceEntry>,
        data: &DataEntities,
        backend_handle: BackendHandle,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> Self {
        let entry = instance.read(cx);
        let instance_id = entry.id;
        let instance_name = entry.name.clone();
        let loader = entry.configuration.loader;
        let loader_version_latest_string = "Última";
        let preferred_loader_version = entry
            .configuration
            .preferred_loader_version
            .map(|s| s.as_str())
            .unwrap_or(loader_version_latest_string);
        let account = entry.configuration.preferred_account;
        let memory = entry.configuration.memory;
        let profile_jvm_choices = entry.configuration.profile_jvm_options.clone();
        let wrapper_command = entry.configuration.wrapper_command.clone().unwrap_or_default();
        let jvm_flags = entry.configuration.jvm_flags.clone().unwrap_or_default();
        let jvm_binary = entry.configuration.jvm_binary.clone().unwrap_or_default();
        #[cfg(target_os = "linux")]
        let linux_wrapper = entry.configuration.linux_wrapper.unwrap_or_default();
        let system_libraries = entry.configuration.system_libraries.clone().unwrap_or_default();

        let instance_root_label = PathLabel::new(entry.root_path.clone(), true);

        let icon = if let Some(raw) = entry.icon.clone() {
            Some(EmbeddedOrRaw::Raw(raw))
        } else if let Some(embedded) = entry.configuration.instance_fallback_icon {
            Some(EmbeddedOrRaw::Embedded(embedded.as_str().into()))
        } else {
            None
        };

        let glfw_path = system_libraries.glfw.get_or_auto(&*AUTO_LIBRARY_PATH_GLFW);
        let openal_path = system_libraries.openal.get_or_auto(&*AUTO_LIBRARY_PATH_OPENAL);

        let new_name_input_state = cx.new(|cx| InputState::new(window, cx).default_value(instance_name));
        cx.subscribe(&new_name_input_state, Self::on_new_name_input).detach();

        let version_select_state =
            cx.new(|cx| SelectState::new(VersionList::default(), None, window, cx).searchable(true));
        cx.subscribe(&version_select_state, Self::on_minecraft_version_selected).detach();

        let hide_usernames = InterfaceConfig::get(cx).hide_usernames;

        let account_items = cx.new(|cx| {
            let accounts = &data.accounts.read(cx).accounts;
            let mut account_items = Vec::with_capacity(accounts.len());
            let mut selected = None;
            for (index, loop_account) in accounts.iter().enumerate() {
                account_items.push(NamedDropdownItem {
                    name: DropdownName::new(loop_account.username(hide_usernames)),
                    item: loop_account.uuid,
                });
                if let Some(preferred_account) = account
                    && loop_account.uuid == preferred_account
                {
                    selected = Some(IndexPath::new(index));
                }
            }

            SelectState::new(NamedDropdown::new(account_items), selected, window, cx).searchable(true)
        });
        cx.observe_in(&data.accounts, window, |page, accounts, window, cx| {
            page.update_account_list(accounts, window, cx);
        })
        .detach();
        cx.subscribe(&account_items, Self::on_account_selected).detach();

        let loader_select_state = cx.new(|cx| {
            let loaders = Loader::iter().map(|l| l.pretty_name()).collect();
            let mut state = SelectState::new(loaders, None, window, cx);
            state.set_selected_value(&loader.pretty_name(), window, cx);
            state
        });
        cx.subscribe_in(&loader_select_state, window, Self::on_loader_selected).detach();

        cx.observe_in(instance, window, |page, instance, window, cx| {
            let entry = instance.read(cx);
            page.instance_root_label = PathLabel::new(entry.root_path.clone(), true);
            page.icon = if let Some(raw) = entry.icon.clone() {
                Some(EmbeddedOrRaw::Raw(raw))
            } else if let Some(embedded) = entry.configuration.instance_fallback_icon {
                Some(EmbeddedOrRaw::Embedded(embedded.as_str().into()))
            } else {
                None
            };
            if page.loader_version_select_state.read(cx).selected_index(cx).is_none() {
                let version = entry
                    .configuration
                    .preferred_loader_version
                    .map(|s| s.as_str())
                    .unwrap_or(loader_version_latest_string);
                page.loader_version_select_state.update(cx, |select_state, cx| {
                    select_state.set_selected_value(&version, window, cx);
                });
            }
        })
        .detach();

        let loader_version_select_state = cx.new(|cx| {
            let mut select_state = SelectState::new(SearchableVec::new(vec![]), None, window, cx).searchable(true);
            select_state.set_selected_value(&preferred_loader_version, window, cx);
            select_state
        });
        cx.subscribe(&loader_version_select_state, Self::on_loader_version_selected).detach();

        let memory_section = cx.new(|c| {
            super::memory_section::InstanceMemorySection::new(instance_id, memory, backend_handle.clone(), window, c)
        });

        let wrapper_command_input_state =
            cx.new(|cx| TextareaState::new(window, cx).auto_grow(1, 8).default_value(wrapper_command.flags));
        cx.subscribe(&wrapper_command_input_state, Self::on_wrapper_command_changed).detach();

        let jvm_flags_input_state =
            cx.new(|cx| TextareaState::new(window, cx).auto_grow(1, 8).default_value(jvm_flags.flags));
        cx.subscribe(&jvm_flags_input_state, Self::on_jvm_flags_changed).detach();

        let profile_jvm_section = {
            let backend_handle = backend_handle.clone();
            cx.new(|cx| {
                super::profile_jvm_section::ProfileJvmSection::new(backend_handle, instance_id, profile_jvm_choices, cx)
            })
        };
        // The page shows the section's "Last Java detected" line, so it redraws when that loads.
        cx.observe(&profile_jvm_section, |_, _, cx| cx.notify()).detach();
        // The AOT cache option can be turned off by the launcher while this page is open.
        cx.observe(instance, |_, _, cx| cx.notify()).detach();

        let artwork_section = cx.new(|cx| {
            super::artwork_section::InstanceArtworkSection::new(instance.clone(), backend_handle.clone(), window, cx)
        });
        let mut page = Self {
            data: data.clone(),
            instance: instance.clone(),
            instance_id,
            new_name_input_state,
            version_state: TypelessFrontendMetadataResult::Loading,
            version_select_state,
            account_items,
            loader,
            loader_select_state,
            loader_version_select_state,
            loader_version_latest_string,
            memory_section,
            artwork_section,
            profile_info: None,
            profile_name: None,
            profile_sequence: None,
            save_group: None,
            show_advanced: std::env::var("WBOMBO_UI_CAPTURE_PAGE")
                .is_ok_and(|route| route.starts_with("instance-settings:agente-274-"))
                && std::env::var("WBOMBO_UI_CAPTURE_STATE").as_deref() == Ok("advanced"),
            editing_version: std::env::var("WBOMBO_UI_CAPTURE_PAGE")
                .is_ok_and(|route| route.starts_with("instance-settings:agente-274-"))
                && std::env::var("WBOMBO_UI_CAPTURE_STATE").as_deref() == Ok("version"),
            wrapper_command_enabled: wrapper_command.enabled,
            wrapper_command_input_state,
            jvm_flags_enabled: jvm_flags.enabled,
            jvm_flags_input_state,
            profile_jvm_section,
            jvm_binary_enabled: jvm_binary.enabled,
            jvm_binary_path: jvm_binary.path.clone().map(|path| PathLabel::new(path, false)),
            override_glfw_enabled: system_libraries.override_glfw,
            override_glfw_path: glfw_path.map(|path| PathLabel::new(path, false)),
            override_openal_enabled: system_libraries.override_openal,
            override_openal_path: openal_path.map(|path| PathLabel::new(path, false)),
            instance_root_label,
            #[cfg(target_os = "linux")]
            use_mangohud: linux_wrapper.use_mangohud,
            #[cfg(target_os = "linux")]
            use_gamemode: linux_wrapper.use_gamemode,
            #[cfg(target_os = "linux")]
            use_discrete_gpu: linux_wrapper.use_discrete_gpu,
            #[cfg(target_os = "linux")]
            disable_gl_threaded_optimizations: linux_wrapper.disable_gl_threaded_optimizations,
            #[cfg(target_os = "linux")]
            mangohud_available: command::is_command_available("mangohud"),
            #[cfg(target_os = "linux")]
            gamemode_available: command::is_command_available("gamemoderun"),
            new_name_change_state: NewNameChangeState::NoChange,
            icon,
            backend_handle,
            loader_versions_state: TypelessFrontendMetadataResult::Loading,
            _observe_minecraft_version_subscription: None,
            _minecraft_version_retry_task: Task::ready(()),
            _observe_loader_version_subscription: None,
            _loader_version_retry_task: Task::ready(()),
            _select_file_task: Task::ready(()),
        };
        page.update_minecraft_versions(window, cx);
        page.update_loader_versions(window, cx);
        page.refresh_profile_info(cx);

        page
    }
}

impl InstanceSettingsSubpage {
    fn refresh_profile_info(&mut self, cx: &mut Context<Self>) {
        let (send, receive) = tokio::sync::oneshot::channel();
        self.backend_handle.send(MessageToBackend::GetInstanceProfileInfo {
            id: self.instance_id,
            channel: send,
        });
        let backend = self.backend_handle.clone();
        cx.spawn(async move |this, cx| {
            let Ok(Ok(info)) = receive.await else {
                return;
            };
            let profile = info.global_profile_id.clone();
            let revision = info.global_revision_id.clone();
            if this
                .update(cx, |this, cx| {
                    this.profile_info = Some(info);
                    cx.notify();
                })
                .is_err()
            {
                return;
            }
            if let Some(profile_id) = profile {
                let (send, receive) = tokio::sync::oneshot::channel();
                backend.send(MessageToBackend::GetGlobalProfiles { channel: send });
                if let Ok(Ok(profiles)) = receive.await {
                    let name = profiles.into_iter().find(|p| p.profile_id == profile_id).map(|p| p.name);
                    let _ = this.update(cx, |this, cx| {
                        this.profile_name = name;
                        cx.notify();
                    });
                }
                let (send, receive) = tokio::sync::oneshot::channel();
                backend.send(MessageToBackend::GetProfileChangelog {
                    profile_id,
                    query: schema::changelog::ChangelogQuery {
                        revision,
                        ..Default::default()
                    },
                    channel: send,
                });
                if let Ok(Ok(history)) = receive.await {
                    let sequence = history.entries.first().and_then(|e| e.revision.as_ref().map(|r| r.sequence));
                    let _ = this.update(cx, |this, cx| {
                        this.profile_sequence = sequence;
                        cx.notify();
                    });
                }
            }
            log::info!("instance-settings-profile-ready pid={}", std::process::id());
        })
        .detach();
        let (send, receive) = tokio::sync::oneshot::channel();
        self.backend_handle.send(MessageToBackend::GetSaveGroups {
            id: self.instance_id,
            channel: send,
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(groups)) = receive.await {
                let _ = this.update(cx, |this, cx| {
                    this.save_group = groups.into_iter().find(|g| g.selected);
                    cx.notify();
                    log::info!("instance-settings-group-ready pid={}", std::process::id());
                });
            }
        })
        .detach();
    }
    fn update_minecraft_versions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let minecraft_versions =
            FrontendMetadata::request(&self.data.metadata, MetadataRequest::MinecraftVersionManifest, cx);

        self._observe_minecraft_version_subscription =
            Some(cx.observe_in(&minecraft_versions, window, |page, minecraft_versions, window, cx| {
                page.process_minecraft_version_metadata(minecraft_versions, window, cx);
                cx.notify();
            }));
        self.process_minecraft_version_metadata(minecraft_versions, window, cx);
    }

    fn process_minecraft_version_metadata(
        &mut self,
        minecraft_versions: Entity<FrontendMetadataState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self._minecraft_version_retry_task = Task::ready(());

        let result: FrontendMetadataResult<MinecraftVersionManifest> = minecraft_versions.read(cx).result();
        let versions = match &result {
            FrontendMetadataResult::Loading => Vec::new(),
            FrontendMetadataResult::Error(_, alive) => {
                if let Some(alive) = alive.clone() {
                    self._minecraft_version_retry_task = cx.spawn_in(window, async move |page, cx| {
                        alive.await_notification().await;
                        _ = page.update_in(cx, |page, window, cx| {
                            page.update_minecraft_versions(window, cx);
                            cx.notify();
                        });
                    });
                }
                Vec::new()
            },
            FrontendMetadataResult::Loaded(manifest) => {
                manifest.versions.iter().map(|v| SharedString::from(v.id.as_str())).collect()
            },
        };
        self.version_state = result.as_typeless();

        let current_version = self.instance.read(cx).configuration.minecraft_version;

        self.version_select_state.update(cx, |dropdown, cx| {
            let mut to_select = None;

            if let Some(last_selected) = dropdown.selected_value().cloned()
                && versions.contains(&last_selected)
            {
                to_select = Some(last_selected);
            }

            if to_select.is_none() && versions.contains(&SharedString::new_static(current_version.as_str())) {
                to_select = Some(SharedString::new_static(current_version.as_str()));
            }

            dropdown.set_items(
                VersionList {
                    versions: versions.clone(),
                    matched_versions: versions,
                },
                window,
                cx,
            );

            if let Some(to_select) = to_select {
                dropdown.set_selected_value(&to_select, window, cx);
            }

            cx.notify();
        });
    }

    fn update_loader_versions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self._observe_loader_version_subscription = None;
        self.loader_versions_state = TypelessFrontendMetadataResult::Loaded;
        self._loader_version_retry_task = Task::ready(());

        let latest_str = self.loader_version_latest_string;
        let loader_versions = match self.loader {
            Loader::Vanilla => {
                vec![""]
            },
            Loader::Fabric => self.update_loader_versions_for_loader(
                MetadataRequest::FabricLoaderManifest,
                move |manifest: &FabricLoaderManifest| {
                    std::iter::once(latest_str).chain(manifest.0.iter().map(|s| s.version.as_str())).collect()
                },
                window,
                cx,
            ),
            Loader::Forge => self.update_loader_versions_for_loader(
                MetadataRequest::ForgeMavenManifest,
                move |manifest: &ForgeMavenManifest| {
                    std::iter::once(latest_str).chain(manifest.0.iter().map(|s| s.as_str())).collect()
                },
                window,
                cx,
            ),
            Loader::NeoForge => self.update_loader_versions_for_loader(
                MetadataRequest::NeoforgeMavenManifest,
                move |manifest: &NeoforgeMavenManifest| {
                    std::iter::once(latest_str).chain(manifest.0.iter().map(|s| s.as_str())).collect()
                },
                window,
                cx,
            ),
        };

        let preferred_loader_version = self
            .instance
            .read(cx)
            .configuration
            .preferred_loader_version
            .map(|s| s.as_str())
            .unwrap_or(latest_str);
        self.loader_version_select_state.update(cx, move |select_state, cx| {
            select_state.set_items(SearchableVec::new(loader_versions), window, cx);
            select_state.set_selected_value(&preferred_loader_version, window, cx);
        });
    }

    fn update_loader_versions_for_loader<T>(
        &mut self,
        request: MetadataRequest,
        items_fn: impl Fn(&T) -> Vec<&'static str> + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<&'static str>
    where
        FrontendMetadataState: AsMetadataResult<T>,
    {
        let request = FrontendMetadata::request(&self.data.metadata, request, cx);

        let result: FrontendMetadataResult<T> = request.read(cx).result();
        let items = match &result {
            FrontendMetadataResult::Loading => vec![],
            FrontendMetadataResult::Loaded(manifest) => (items_fn)(&manifest),
            FrontendMetadataResult::Error(_, alive) => {
                if let Some(alive) = alive.clone() {
                    self._loader_version_retry_task = cx.spawn_in(window, async move |page, cx| {
                        alive.await_notification().await;
                        _ = page.update_in(cx, |page, window, cx| {
                            page.update_loader_versions(window, cx);
                            cx.notify();
                        });
                    });
                }
                vec![]
            },
        };
        self.loader_versions_state = result.as_typeless();
        self._observe_loader_version_subscription =
            Some(cx.observe_in(&request, window, move |page, _, window, cx| {
                page.update_loader_versions(window, cx);
            }));
        items
    }

    fn update_account_list(&mut self, accounts: Entity<AccountEntries>, window: &mut Window, cx: &mut Context<Self>) {
        let hide_usernames = InterfaceConfig::get(cx).hide_usernames;

        let list = accounts
            .read(cx)
            .accounts
            .iter()
            .map(|account| NamedDropdownItem {
                name: DropdownName::new(account.username(hide_usernames)),
                item: account.uuid,
            })
            .collect::<Vec<NamedDropdownItem<Uuid>>>();

        self.account_items.update(cx, move |items, cx| {
            items.set_items(NamedDropdown::new(list), window, cx);
        })
    }

    pub fn on_new_name_input(&mut self, state: Entity<InputState>, event: &InputEvent, cx: &mut Context<Self>) {
        if let InputEvent::Change = event {
            let new_name = state.read(cx).value();
            if new_name.is_empty() {
                self.new_name_change_state = NewNameChangeState::NoChange;
                return;
            }

            let instance = self.instance.read(cx);
            if instance.name == new_name {
                self.new_name_change_state = NewNameChangeState::NoChange;
                return;
            }

            if !crate::is_valid_instance_name(new_name.as_str()) {
                self.new_name_change_state = NewNameChangeState::InvalidName;
                return;
            }

            self.new_name_change_state = NewNameChangeState::Pending;
        }
    }

    pub fn on_minecraft_version_selected(
        &mut self,
        _state: Entity<SelectState<VersionList>>,
        event: &SelectEvent<VersionList>,
        _cx: &mut Context<Self>,
    ) {
        let SelectEvent::Confirm(value) = event;

        let Some(value) = value else {
            return;
        };

        self.backend_handle.send(MessageToBackend::SetInstanceMinecraftVersion {
            id: self.instance_id,
            version: value.as_str().into(),
        });
    }

    pub fn on_account_selected(
        &mut self,
        _state: Entity<SelectState<NamedDropdown<Uuid>>>,
        event: &SelectEvent<NamedDropdown<Uuid>>,
        _cx: &mut Context<Self>,
    ) {
        let SelectEvent::Confirm(value) = event;

        self.backend_handle.send(MessageToBackend::SetInstancePreferredAccount {
            id: self.instance_id,
            account: value.clone(),
        });
    }

    pub fn on_loader_selected(
        &mut self,
        _state: &Entity<SelectState<Vec<&'static str>>>,
        event: &SelectEvent<Vec<&'static str>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let SelectEvent::Confirm(value) = event;
        let Some(value) = value else {
            return;
        };

        let Some(loader) = Loader::from_name(value) else {
            return;
        };

        if self.loader != loader {
            self.loader = loader;
            self.backend_handle.send(MessageToBackend::SetInstanceLoader {
                id: self.instance_id,
                loader: self.loader,
            });
            self.update_loader_versions(window, cx);
            cx.notify();
        }
    }

    pub fn on_loader_version_selected(
        &mut self,
        _state: Entity<SelectState<SearchableVec<&'static str>>>,
        event: &SelectEvent<SearchableVec<&'static str>>,
        _cx: &mut Context<Self>,
    ) {
        let SelectEvent::Confirm(value) = event;

        let value = if value == &Some(self.loader_version_latest_string) {
            None
        } else {
            value.clone()
        };

        self.backend_handle.send(MessageToBackend::SetInstancePreferredLoaderVersion {
            id: self.instance_id,
            loader_version: value,
        });
    }

    pub fn on_wrapper_command_changed(&mut self, _: Entity<TextareaState>, event: &InputEvent, cx: &mut Context<Self>) {
        if let InputEvent::Change = event {
            self.backend_handle.send(MessageToBackend::SetInstanceWrapperCommand {
                id: self.instance_id,
                wrapper_command: self.get_wrapper_command_configuration(cx),
            });
        }
    }

    fn get_wrapper_command_configuration(&self, cx: &App) -> InstanceWrapperCommandConfiguration {
        let flags = self.wrapper_command_input_state.read(cx).value();

        InstanceWrapperCommandConfiguration {
            enabled: self.wrapper_command_enabled,
            flags: flags.into(),
        }
    }

    /// Profile options can be overridden by the player's JVM flags or memory setting.
    fn refresh_profile_jvm(&self, cx: &mut Context<Self>) {
        self.profile_jvm_section.update(cx, |section, cx| section.refresh(cx));
    }

    pub fn on_jvm_flags_changed(&mut self, _: Entity<TextareaState>, event: &InputEvent, cx: &mut Context<Self>) {
        if let InputEvent::Change = event {
            self.backend_handle.send(MessageToBackend::SetInstanceJvmFlags {
                id: self.instance_id,
                jvm_flags: self.get_jvm_flags_configuration(cx),
            });
            self.refresh_profile_jvm(cx);
        }
    }

    fn get_jvm_flags_configuration(&self, cx: &App) -> InstanceJvmFlagsConfiguration {
        let flags = self.jvm_flags_input_state.read(cx).value();

        InstanceJvmFlagsConfiguration {
            enabled: self.jvm_flags_enabled,
            flags: flags.into(),
        }
    }

    fn get_jvm_binary_configuration(&self) -> InstanceJvmBinaryConfiguration {
        InstanceJvmBinaryConfiguration {
            enabled: self.jvm_binary_enabled,
            path: self.jvm_binary_path.as_ref().map(PathLabel::path),
        }
    }

    fn get_system_libraries_configuration(&self) -> InstanceSystemLibrariesConfiguration {
        InstanceSystemLibrariesConfiguration {
            override_glfw: self.override_glfw_enabled,
            glfw: Self::create_lwjgl_library_path(
                &self.override_glfw_path.as_ref().map(PathLabel::path),
                &*AUTO_LIBRARY_PATH_GLFW,
            ),
            override_openal: self.override_openal_enabled,
            openal: Self::create_lwjgl_library_path(
                &self.override_openal_path.as_ref().map(PathLabel::path),
                &*AUTO_LIBRARY_PATH_OPENAL,
            ),
        }
    }

    fn create_lwjgl_library_path(path: &Option<Arc<Path>>, auto: &Option<Arc<Path>>) -> LwjglLibraryPath {
        if let Some(path) = path {
            if let Some(auto) = auto
                && path == auto
            {
                LwjglLibraryPath::AutoPreferred(path.clone())
            } else {
                LwjglLibraryPath::Explicit(path.clone())
            }
        } else {
            LwjglLibraryPath::Auto
        }
    }

    #[cfg(target_os = "linux")]
    fn get_linux_wrapper_configuration(&self) -> InstanceLinuxWrapperConfiguration {
        InstanceLinuxWrapperConfiguration {
            use_mangohud: self.use_mangohud,
            use_gamemode: self.use_gamemode,
            use_discrete_gpu: self.use_discrete_gpu,
            disable_gl_threaded_optimizations: self.disable_gl_threaded_optimizations,
        }
    }

    pub fn select_file(
        &mut self,
        message: &'static str,
        handle: impl FnOnce(&mut Self, Option<Arc<Path>>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(message.into()),
        });

        let this_entity = cx.entity();
        self._select_file_task = window.spawn(cx, async move |cx| {
            let Ok(result) = receiver.await else {
                return;
            };
            _ = cx.update_window_entity(&this_entity, move |this, window, cx| match result {
                Ok(Some(paths)) => {
                    (handle)(this, paths.first().map(|v| v.as_path().into()));
                    cx.notify();
                },
                Ok(None) => {},
                Err(error) => {
                    let error = format!("{}", error);
                    let notification =
                        Notification::new().autohide(false).with_type(NotificationType::Error).title(error);
                    window.push_notification(notification, cx);
                },
            });
        });
    }
}

impl Render for InstanceSettingsSubpage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let basic = self.render_basic_content(cx);
        let runtime = self.render_runtime_content(cx);
        let actions = self.render_actions_content(cx);
        let left = v_flex()
            .flex_1()
            .min_w_0()
            .gap(px(16.))
            .child(settings_panel("General").child(basic))
            .child(settings_panel("Memoria y Java").child(runtime))
            .child(self.profile_jvm_section.clone());
        let right = v_flex()
            .flex_1()
            .min_w_0()
            .gap(px(16.))
            .child(settings_panel("Artwork").child(self.artwork_section.clone()))
            .child(actions);
        v_flex()
            .id("instance-settings-scroll")
            .size_full()
            .overflow_y_scrollbar()
            .px(px(28.))
            .py(px(20.))
            .text_size(px(13.))
            .text_color(rgb(theme::INK))
            .child(h_flex().w_full().items_start().gap(px(16.)).child(left).child(right))
    }
}
fn settings_panel(title: &'static str) -> Div {
    v_flex().w_full().gap(px(10.)).px(px(18.)).py(px(16.)).bg(rgb(theme::PANEL)).child(
        div()
            .font_family(theme::DISPLAY)
            .font_weight(FontWeight::BOLD)
            .text_size(px(20.))
            .line_height(px(24.))
            .child(title),
    )
}

impl InstanceSettingsSubpage {
    // Keep these sections in separate render frames and erase their concrete
    // builder types before composing them. This prevents the large settings
    // tree from exhausting the Windows UI thread's limited stack.
    fn render_basic_content(&mut self, cx: &mut gpui::Context<Self>) -> AnyElement {
        let theme_radius = cx.theme().radius;

        let icon_element: Option<AnyElement> = self.icon.clone().map(|icon| match icon {
            EmbeddedOrRaw::Embedded(path) => Icon::default().path(path).size_8().min_w_8().min_h_8().into_any_element(),
            EmbeddedOrRaw::Raw(data) => {
                let radius = theme_radius;
                let transform = png_render_cache::ImageTransformation::Resize { width: 32, height: 32 };
                png_render_cache::render_with_transform(data, transform, cx)
                    .rounded(radius)
                    .size_8()
                    .min_w_8()
                    .min_h_8()
                    .into_any_element()
            },
        });

        let mut basic_content = v_flex()
            .gap_4()
            .w_full()
            .child(crate::labelled(
                "Nombre",
                h_flex()
                    .gap_2()
                    .child(crate::component::design::text_field(&self.new_name_input_state))
                    .when(self.new_name_change_state != NewNameChangeState::NoChange, |this| {
                        if self.new_name_change_state == NewNameChangeState::InvalidName {
                            this.child("El nombre no es válido o ya existe.")
                        } else {
                            this.child(DesignButton::new("setname").label("Actualizar").on_click({
                                let instance = self.instance.clone();
                                let backend_handle = self.backend_handle.clone();
                                let new_name = self.new_name_input_state.read(cx).value();
                                move |_, _, cx| {
                                    let instance = instance.read(cx);
                                    let id = instance.id;
                                    backend_handle.send(MessageToBackend::RenameInstance {
                                        id,
                                        name: new_name.as_str().into(),
                                    });
                                }
                            }))
                        }
                    }),
            ))
            .child({
                let mut row = h_flex().gap_2().child(
                    DesignButton::new("icon")
                        .child(Icon::new(crate::icon::PandoraIcon::Plus).size(px(14.)))
                        .label("Cambiar icono")
                        .on_click({
                            let entity = cx.entity();
                            move |_, window, cx| {
                                let entity = entity.clone();
                                crate::modals::select_icon::open_select_icon(
                                    Box::new(move |icon, cx| {
                                        cx.update_entity(&entity, |this, _| {
                                            this.icon = Some(icon.clone());
                                            this.backend_handle.send(MessageToBackend::SetInstanceIcon {
                                                id: this.instance_id,
                                                icon: Some(icon),
                                            });
                                        });
                                    }),
                                    window,
                                    cx,
                                );
                            }
                        }),
                );
                if let Some(el) = icon_element {
                    row = row.child(el);
                }
                row
            });

        let mut version_content = v_flex().gap_2();

        match self.version_state {
            TypelessFrontendMetadataResult::Loading => {
                version_content = version_content.child(Skeleton::new().w_full().min_h_8().max_h_8().rounded_md());
            },
            TypelessFrontendMetadataResult::Loaded => {
                version_content = version_content.child(
                    Select::new(&self.version_select_state)
                        .bg(rgb(theme::PANEL_SUNKEN))
                        .h(px(36.))
                        .text_size(px(13.))
                        .search_placeholder("Buscar...")
                        .w_full(),
                );
            },
            TypelessFrontendMetadataResult::Error(_, ..) => {
                version_content = version_content.child("No se han podido cargar las versiones.")
            },
        }

        version_content = version_content.child(
            Select::new(&self.loader_select_state)
                .bg(rgb(theme::PANEL_SUNKEN))
                .h(px(36.))
                .text_size(px(13.))
                .search_placeholder("Buscar...")
                .w_full(),
        );

        if self.loader != Loader::Vanilla {
            match self.loader_versions_state {
                TypelessFrontendMetadataResult::Loading => {
                    version_content = version_content.child(Skeleton::new().w_full().min_h_8().max_h_8().rounded_md())
                },
                TypelessFrontendMetadataResult::Loaded => {
                    version_content = version_content.child(
                        Select::new(&self.loader_version_select_state)
                            .bg(rgb(theme::PANEL_SUNKEN))
                            .h(px(36.))
                            .text_size(px(13.))
                            .search_placeholder("Buscar...")
                            .w_full(),
                    )
                },
                TypelessFrontendMetadataResult::Error(_, ..) => {
                    version_content = version_content.child("No se han podido cargar las versiones.")
                },
            }
        }

        let configuration = &self.instance.read(cx).configuration;
        let version = format!(
            "{:?} {} · Minecraft {}",
            configuration.loader,
            configuration.preferred_loader_version.map(|v| v.as_str()).unwrap_or(""),
            configuration.minecraft_version
        );
        let version_row = v_flex()
            .gap(px(10.))
            .child(
                h_flex().w_full().gap(px(10.)).child(div().flex_1().child(version)).child(
                    DesignButton::new("edit-instance-version")
                        .tooltip("Versión")
                        .w(px(28.))
                        .h(px(28.))
                        .child(Icon::new(PandoraIcon::Pencil).size(px(14.)))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.editing_version = !this.editing_version;
                            cx.notify();
                        })),
                ),
            )
            .when(self.editing_version, |d| d.child(version_content));
        basic_content = basic_content.child(crate::labelled("Versión", version_row)).when(
            !self
                .profile_info
                .as_ref()
                .is_some_and(|i| i.global_profile_id.is_some() || i.local_parent_name.is_some()),
            |d| {
                d.child(crate::labelled(
                    "Cuenta para esta instancia",
                    h_flex().gap_2().child(
                        Select::new(&self.account_items)
                            .placeholder("La activa")
                            .bg(rgb(theme::PANEL_SUNKEN))
                            .h(px(36.))
                            .text_size(px(13.))
                            .search_placeholder("Buscar...")
                            .cleanable(true),
                    ),
                ))
            },
        );

        basic_content = basic_content.child(crate::labelled(
            "Carpeta de la instancia",
            DesignButton::new("relocate").child(self.instance_root_label.clone()).on_click({
                let instance = self.instance.clone();
                let backend_handle = self.backend_handle.clone();
                move |_: &ClickEvent, _, cx| {
                    let instance = instance.read(cx);
                    let id = instance.id;

                    let receiver = cx.prompt_for_paths(PathPromptOptions {
                        files: false,
                        directories: true,
                        multiple: false,
                        prompt: Some("Selecciona una carpeta vacía".into()),
                    });
                    let backend_handle = backend_handle.clone();
                    cx.spawn(async move |_| {
                        let Ok(Ok(Some(mut paths))) = receiver.await else {
                            return;
                        };
                        if paths.is_empty() {
                            return;
                        }
                        backend_handle.send(MessageToBackend::RelocateInstance {
                            id,
                            path: paths.swap_remove(0),
                        });
                    })
                    .detach();
                }
            }),
        ));
        basic_content.into_any_element()
    }

    /// "Optimize launch (Java 25 AOT cache)": on by default; shows what the cache is doing in plain
    /// words, and why the launcher turned the option off.
    fn render_aot_cache_setting(&self, cx: &mut gpui::Context<Self>) -> AnyElement {
        let instance = self.instance.read(cx);
        let setting = instance.configuration.aot_cache.clone();
        let (status, summary) = (instance.status, instance.aot_cache);
        let enabled = setting.as_ref().is_none_or(|setting| setting.enabled);
        let auto_disabled = setting.and_then(|setting| setting.auto_disabled_reason).filter(|_| !enabled);
        v_flex().gap(px(12.))
            .child(h_flex().w_full().justify_between().gap(px(12.))
                .child(v_flex().flex_1().gap(px(3.)).child("Optimizar arranque (caché AOT de Java 25)")
                    .child(div().text_size(px(12.)).text_color(rgb(theme::INK_MUTED)).child("4–9 s más rápido hasta el menú; usa unos 300 MB")))
                .child(YesNo::new("aot-cache").checked(enabled).on_click(cx.listener(|this,value:&bool,_,c|{
                    this.backend_handle.send(MessageToBackend::SetInstanceAotCache{id:this.instance_id,enabled:*value});c.notify();
                }))))
            .children(enabled.then(||self.render_aot_cache_state(status,summary,cx)))
            .children(auto_disabled.map(|reason|div().text_size(px(12.)).line_height(px(18.)).text_color(rgb(theme::INK_MUTED))
                .child(format!("Desactivada automáticamente porque Java ha rechazado la caché ({reason}). Puedes volver a activarla."))))
            .into_any_element()
    }

    /// One line under the option: whether the cache is ready, being prepared or not built yet.
    fn render_aot_cache_state(
        &self,
        status: InstanceStatus,
        summary: AotCacheSummary,
        cx: &mut gpui::Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let (dot, text) = match (status, summary) {
            (InstanceStatus::Optimizing, _) => (theme.info, "Creando la caché de arranque".to_owned()),
            (_, AotCacheSummary::Ready { bytes }) => (
                theme.success,
                format!("Lista: los arranques son más rápidos. Usa {} MB.", bytes.div_ceil(1_000_000)),
            ),
            (_, AotCacheSummary::Training) => (
                theme.info,
                "Preparándose durante esta partida. Cierra el juego normalmente para terminar.".to_owned(),
            ),
            (_, AotCacheSummary::Waiting) => {
                (theme.info, "Casi lista: el launcher la termina cuando quede libre.".to_owned())
            },
            (_, AotCacheSummary::NotBuilt | AotCacheSummary::Off) => {
                (theme.muted_foreground, "Todavía no está lista: la próxima partida la prepara.".to_owned())
            },
        };
        let can_delete = matches!(summary, AotCacheSummary::Ready { .. }) && status == InstanceStatus::NotRunning;
        let instance_id = self.instance_id;
        let data = self.data.clone();
        h_flex()
            .w_full()
            .p(px(12.))
            .gap(px(10.))
            .border_1()
            .border_color(rgba(0xffffff0b))
            .items_center()
            .text_size(px(12.))
            .line_height(px(18.))
            .text_color(theme.muted_foreground)
            .child(div().flex_none().size_2().rounded_full().bg(dot))
            .child(div().child(text))
            .when(can_delete, |row| {
                row.child(
                    DesignButton::new("aot_cache_delete")
                        .label("Borrar caché")
                        .on_click(move |_, window, cx| {
                            crate::modals::aot_optimization::confirm_delete(instance_id, &data, window, cx)
                        }),
                )
            })
            .into_any_element()
    }

    fn render_runtime_content(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let wrapper_command_enabled = self.wrapper_command_enabled;
        let jvm_flags_enabled = self.jvm_flags_enabled;
        let jvm_binary_enabled = self.jvm_binary_enabled;
        let content = v_flex()
            .w_full()
            .gap(px(16.))
            .child(self.memory_section.clone())
            .child(
                v_flex()
                    .gap_1()
                    .child(h_flex().w_full().gap(px(12.)).child(div().flex_1().child("Opciones de la JVM")).child(
                        YesNo::new("jvm_flags").checked(jvm_flags_enabled).on_click(cx.listener(
                            |page, value, _, cx| {
                                if page.jvm_flags_enabled != *value {
                                    page.jvm_flags_enabled = *value;
                                    page.backend_handle.send(MessageToBackend::SetInstanceJvmFlags {
                                        id: page.instance_id,
                                        jvm_flags: page.get_jvm_flags_configuration(cx),
                                    });
                                    page.refresh_profile_jvm(cx);
                                    cx.notify();
                                }
                            },
                        )),
                    ))
                    .child(
                        Textarea::new(&self.jvm_flags_input_state)
                            .bg(rgb(theme::PANEL_SUNKEN))
                            .disabled(!jvm_flags_enabled),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(h_flex().w_full().gap(px(12.)).child(div().flex_1().child("Comando envoltorio")).child(
                        YesNo::new("wrapper_command").checked(wrapper_command_enabled).on_click(cx.listener(
                            |page, value, _, cx| {
                                if page.wrapper_command_enabled != *value {
                                    page.wrapper_command_enabled = *value;
                                    page.backend_handle.send(MessageToBackend::SetInstanceWrapperCommand {
                                        id: page.instance_id,
                                        wrapper_command: page.get_wrapper_command_configuration(cx),
                                    });
                                    cx.notify();
                                }
                            },
                        )),
                    ))
                    .child(
                        Textarea::new(&self.wrapper_command_input_state)
                            .bg(rgb(theme::PANEL_SUNKEN))
                            .disabled(!wrapper_command_enabled),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(h_flex().w_full().justify_between().child("Java").child(
                        crate::component::design::chip("java-automatic", "Automático", !jvm_binary_enabled).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.jvm_binary_enabled = false;
                                this.backend_handle.send(MessageToBackend::SetInstanceJvmBinary {
                                    id: this.instance_id,
                                    jvm_binary: this.get_jvm_binary_configuration(),
                                });
                                cx.notify();
                            }),
                        ),
                    ))
                    .child(DesignButton::new("select_jvm_binary").label("Elegir java.exe…").on_click(cx.listener(
                        |this, _, window, cx| {
                            this.select_file(
                                "Elegir java.exe…",
                                |this, path| {
                                    if path.is_none() {
                                        return;
                                    }
                                    this.jvm_binary_enabled = true;
                                    this.jvm_binary_path = path.map(|path| PathLabel::new(path, false));
                                    this.backend_handle.send(MessageToBackend::SetInstanceJvmBinary {
                                        id: this.instance_id,
                                        jvm_binary: this.get_jvm_binary_configuration(),
                                    });
                                },
                                window,
                                cx,
                            );
                        },
                    ))),
            )
            .when_some(self.jvm_binary_path.clone().filter(|_| jvm_binary_enabled), |d, path| d.child(path))
            .when_some(self.profile_jvm_section.read(cx).last_java_text(), |d, java| {
                d.child(div().text_size(px(12.)).text_color(rgb(theme::INK_MUTED)).child(java))
            })
            .child(
                DesignButton::new("advanced-settings")
                    .label("Ajustes avanzados")
                    .toggled(self.show_advanced)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show_advanced = !this.show_advanced;
                        cx.notify();
                    })),
            )
            .when(self.show_advanced, |d| d.child(self.render_advanced_content(cx)));
        content.into_any_element()
    }
    fn render_advanced_content(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let advanced = v_flex()
            .w_full()
            .gap(px(12.))
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .w_full()
                            .gap(px(12.))
                            .child(div().flex_1().child("Biblioteca GLFW del sistema"))
                            .child(YesNo::new("system_glfw").checked(self.override_glfw_enabled).on_click(
                                cx.listener(|page, value, _, cx| {
                                    if page.override_glfw_enabled != *value {
                                        page.override_glfw_enabled = *value;
                                        page.backend_handle.send(MessageToBackend::SetInstanceSystemLibraries {
                                            id: page.instance_id,
                                            system_libraries: page.get_system_libraries_configuration(),
                                        });
                                        cx.notify();
                                    }
                                }),
                            )),
                    )
                    .child(
                        DesignButton::new("select_glfw")
                            .label("Seleccionar biblioteca…")
                            .disabled(!self.override_glfw_enabled)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.select_file(
                                    "Seleccionar biblioteca…",
                                    |this, path| {
                                        this.override_glfw_path = path.map(|path| PathLabel::new(path, false));
                                        this.backend_handle.send(MessageToBackend::SetInstanceSystemLibraries {
                                            id: this.instance_id,
                                            system_libraries: this.get_system_libraries_configuration(),
                                        });
                                    },
                                    window,
                                    cx,
                                );
                            })),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .w_full()
                            .gap(px(12.))
                            .child(div().flex_1().child("Biblioteca OpenAL del sistema"))
                            .child(YesNo::new("system_openal").checked(self.override_openal_enabled).on_click(
                                cx.listener(|page, value, _, cx| {
                                    if page.override_openal_enabled != *value {
                                        page.override_openal_enabled = *value;
                                        page.backend_handle.send(MessageToBackend::SetInstanceSystemLibraries {
                                            id: page.instance_id,
                                            system_libraries: page.get_system_libraries_configuration(),
                                        });
                                        cx.notify();
                                    }
                                }),
                            )),
                    )
                    .child(
                        DesignButton::new("select_openal")
                            .label("Seleccionar biblioteca…")
                            .disabled(!self.override_openal_enabled)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.select_file(
                                    "Seleccionar biblioteca…",
                                    |this, path| {
                                        this.override_openal_path = path.map(|path| PathLabel::new(path, false));
                                        this.backend_handle.send(MessageToBackend::SetInstanceSystemLibraries {
                                            id: this.instance_id,
                                            system_libraries: this.get_system_libraries_configuration(),
                                        });
                                    },
                                    window,
                                    cx,
                                );
                            })),
                    ),
            );
        #[cfg(target_os = "linux")]
        let advanced = advanced.child(
            v_flex()
                .gap_1()
                .child(
                    h_flex().w_full().gap(px(12.)).child(div().flex_1().child("MangoHud")).child(
                        YesNo::new("use_mangohud")
                            .checked(self.use_mangohud && self.mangohud_available)
                            .disabled(!self.mangohud_available)
                            .on_click(cx.listener(|page, value, _, cx| {
                                if page.use_mangohud != *value {
                                    page.use_mangohud = *value;
                                    page.backend_handle.send(MessageToBackend::SetInstanceLinuxWrapper {
                                        id: page.instance_id,
                                        linux_wrapper: page.get_linux_wrapper_configuration(),
                                    });
                                    cx.notify();
                                }
                            })),
                    ),
                )
                .child(
                    h_flex().w_full().gap(px(12.)).child(div().flex_1().child("GameMode")).child(
                        YesNo::new("use_gamemode")
                            .checked(self.use_gamemode && self.gamemode_available)
                            .disabled(!self.gamemode_available)
                            .on_click(cx.listener(|page, value, _, cx| {
                                if page.use_gamemode != *value {
                                    page.use_gamemode = *value;
                                    page.backend_handle.send(MessageToBackend::SetInstanceLinuxWrapper {
                                        id: page.instance_id,
                                        linux_wrapper: page.get_linux_wrapper_configuration(),
                                    });
                                    cx.notify();
                                }
                            })),
                    ),
                )
                .child(h_flex().w_full().gap(px(12.)).child(div().flex_1().child("Usar la GPU dedicada")).child(
                    YesNo::new("use_discrete_gpu").checked(self.use_discrete_gpu).on_click(cx.listener(
                        |page, value, _, cx| {
                            if page.use_discrete_gpu != *value {
                                page.use_discrete_gpu = *value;
                                page.backend_handle.send(MessageToBackend::SetInstanceLinuxWrapper {
                                    id: page.instance_id,
                                    linux_wrapper: page.get_linux_wrapper_configuration(),
                                });
                                cx.notify();
                            }
                        },
                    )),
                ))
                .child(
                    h_flex()
                        .w_full()
                        .gap(px(12.))
                        .child(div().flex_1().child("Desactivar las optimizaciones multihilo de OpenGL"))
                        .child(
                            YesNo::new("disable_gl_threaded_optimizations")
                                .checked(self.disable_gl_threaded_optimizations)
                                .on_click(cx.listener(|page, value, _, cx| {
                                    if page.disable_gl_threaded_optimizations != *value {
                                        page.disable_gl_threaded_optimizations = *value;
                                        page.backend_handle.send(MessageToBackend::SetInstanceLinuxWrapper {
                                            id: page.instance_id,
                                            linux_wrapper: page.get_linux_wrapper_configuration(),
                                        });
                                        cx.notify();
                                    }
                                })),
                        ),
                ),
        );
        advanced
            .when_some(self.override_glfw_path.clone().filter(|_| self.override_glfw_enabled), |d, path| {
                d.child(path)
            })
            .when_some(self.override_openal_path.clone().filter(|_| self.override_openal_enabled), |d, path| {
                d.child(path)
            })
            .into_any_element()
    }
    fn render_actions_content(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let launch = settings_panel("Arranque")
            .child(self.render_aot_cache_setting(cx))
            .child(DesignButton::new("repair_game_files").label("Reparar archivos del juego").on_click({
                let id = self.instance_id;
                let backend = self.backend_handle.clone();
                move |_, window, cx| crate::root::repair_game_files(id, &backend, window, cx)
            }))
            .child(
                DesignButton::new("shortcut")
                    .label("Crear acceso directo")
                    .child(Icon::new(PandoraIcon::ExternalLink).size(px(14.)))
                    .overflow_x_hidden()
                    .on_click({
                        let instance = self.instance.clone();
                        let backend_handle = self.backend_handle.clone();
                        move |_: &ClickEvent, _, cx| {
                            let user_dirs = directories::UserDirs::new();
                            let directory = user_dirs
                                .as_ref()
                                .and_then(directories::UserDirs::desktop_dir)
                                .unwrap_or(Path::new("."));
                            let instance = instance.read(cx);
                            let id = instance.id;
                            let name = instance.name.clone();

                            #[cfg(target_os = "linux")]
                            let suggested_name = format!("{name}.desktop");
                            #[cfg(target_os = "windows")]
                            let suggested_name = format!("{name}.lnk");
                            #[cfg(target_os = "macos")]
                            let suggested_name = format!("{name}.app");

                            let receiver = cx.prompt_for_new_path(directory, Some(&suggested_name));
                            let backend_handle = backend_handle.clone();
                            cx.spawn(async move |_| {
                                let Ok(Ok(Some(path))) = receiver.await else {
                                    return;
                                };
                                backend_handle.send(MessageToBackend::CreateInstanceShortcut { id, path });
                            })
                            .detach();
                        }
                    }),
            );
        let worlds = settings_panel("Mundos compartidos")
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .child("Comparte la carpeta saves con otras instancias mediante un grupo local."),
                    )
                    .child(DesignButton::new("manage-save-groups").label("Gestionar grupo").on_click({
                        let id = self.instance_id;
                        let backend = self.backend_handle.clone();
                        move |_, window, cx| {
                            crate::modals::save_groups::open_save_groups(id, backend.clone(), window, cx)
                        }
                    })),
            )
            .when_some(self.save_group.as_ref(), |d, group| {
                d.child(format!("Grupo: {} ({} instancias)", group.name, group.member_count))
            });
        let mut inherited = settings_panel("Perfil heredado")
            .child(div().child("Lo que edites o borres en local se conserva al actualizar."));
        let parent = self
            .profile_info
            .as_ref()
            .and_then(|i| i.local_parent_name.clone())
            .or_else(|| self.profile_name.clone());
        if let Some(parent) = parent {
            let caption = if let Some(sequence) = self
                .profile_sequence
                .filter(|_| self.profile_info.as_ref().is_some_and(|i| i.local_parent_name.is_none()))
            {
                format!("Deriva de {parent} (perfil global, revisión {sequence}).")
            } else {
                format!("Deriva de {parent}.")
            };
            inherited = inherited.child(caption);
        }
        inherited = inherited.child(
            DesignButton::new("update_global_profile")
                .label("Buscar actualización")
                .child(Icon::new(PandoraIcon::RefreshCcw).size(px(14.)))
                .overflow_x_hidden()
                .on_click({
                    let id = self.instance_id;
                    let data = self.data.clone();
                    move |_: &ClickEvent, window, cx| {
                        crate::root::update_inherited_files(id, &data, window, cx);
                    }
                }),
        );
        inherited = inherited.child(crate::labelled(
            "Cuenta para esta instancia",
            Select::new(&self.account_items)
                .placeholder("La activa")
                .bg(rgb(theme::PANEL_SUNKEN))
                .h(px(36.))
                .text_size(px(13.))
                .search_placeholder("Buscar...")
                .cleanable(true),
        ));
        let has_parent = self
            .profile_info
            .as_ref()
            .is_some_and(|i| i.global_profile_id.is_some() || i.local_parent_name.is_some());
        let actions = settings_panel("Acciones")
            .child(
                DesignButton::new("duplicate")
                    .label("Duplicar")
                    .child(Icon::new(PandoraIcon::Copy).size(px(14.)))
                    .overflow_x_hidden()
                    .on_click({
                        let instance = self.instance.clone();
                        let instances = self.data.instances.clone();
                        let backend_handle = self.backend_handle.clone();
                        move |_: &ClickEvent, window, cx| {
                            let instance = instance.read(cx);
                            crate::modals::duplicate_instance::open_duplicate_instance(
                                instance.id,
                                instance.name.clone(),
                                instances.clone(),
                                backend_handle.clone(),
                                window,
                                cx,
                            );
                        }
                    }),
            )
            .child(
                DesignButton::new("export")
                    .label("Exportar")
                    .child(Icon::new(PandoraIcon::Archive).size(px(14.)))
                    .overflow_x_hidden()
                    .on_click({
                        let instance = self.instance.clone();
                        let backend_handle = self.backend_handle.clone();
                        move |_: &ClickEvent, window, cx| {
                            let instance = instance.read(cx);
                            crate::modals::export_instance::open_export_instance(
                                instance.id,
                                instance.name.clone(),
                                backend_handle.clone(),
                                window,
                                cx,
                            );
                        }
                    }),
            )
            .child(
                DesignButton::new("derive")
                    .label("Crear hija")
                    .child(Icon::new(PandoraIcon::Copy).size(px(14.)))
                    .on_click({
                        let instance = self.instance.clone();
                        let instances = self.data.instances.clone();
                        let backend = self.backend_handle.clone();
                        move |_, window, cx| {
                            let instance = instance.read(cx);
                            crate::modals::duplicate_instance::open_derived_instance(
                                instance.id,
                                instance.name.clone(),
                                instances.clone(),
                                backend.clone(),
                                window,
                                cx,
                            );
                        }
                    }),
            )
            .child(
                DesignButton::new("delete")
                    .label("Eliminar instancia")
                    .child(Icon::new(PandoraIcon::Trash2).size(px(14.)))
                    .overflow_x_hidden()
                    .danger()
                    .on_click({
                        let instance = self.instance.clone();
                        let backend_handle = self.backend_handle.clone();
                        move |click: &ClickEvent, window, cx| {
                            let instance = instance.read(cx);
                            let id = instance.id;
                            let name = instance.name.clone();

                            if InterfaceConfig::get(cx).quick_delete_instance && click.modifiers().shift {
                                backend_handle.send(bridge::message::MessageToBackend::DeleteInstance { id });
                            } else {
                                crate::modals::delete_instance::open_delete_instance(
                                    id,
                                    name,
                                    backend_handle.clone(),
                                    window,
                                    cx,
                                );
                            }
                        }
                    }),
            );
        v_flex()
            .w_full()
            .gap(px(16.))
            .child(launch)
            .when(has_parent, |d| d.child(inherited))
            .child(worlds)
            .child(actions)
            .into_any_element()
    }
}
