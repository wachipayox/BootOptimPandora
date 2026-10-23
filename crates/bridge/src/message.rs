use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicU64},
};

use schema::{
    backend_config::{BackendConfig, DistributionConfig, ProxyConfig},
    instance::{
        InstanceConfiguration, InstanceJvmBinaryConfiguration, InstanceJvmFlagsConfiguration,
        InstanceLinuxWrapperConfiguration, InstanceMemoryConfiguration, InstanceSystemLibrariesConfiguration,
        InstanceWrapperCommandConfiguration, UpdateChannel,
    },
    loader::Loader,
    minecraft_profile::{MinecraftProfileCape, SkinVariant},
    pandora_update::UpdatePrompt,
    unique_bytes::UniqueBytes,
};
use ustr::Ustr;
use uuid::Uuid;

use crate::{
    account::Account,
    game_output::GameOutputLogLevel,
    import::{ImportFromOtherLauncherJob, OtherLauncher},
    install::ContentInstall,
    instance::{
        ContentFolder, InstanceContentID, InstanceContentSummary, InstanceID, InstancePlaytime, InstanceServerSummary,
        InstanceStatus, InstanceWorldSummary,
    },
    manual_download::ManualCurseforgeDownloadRequest,
    meta::{MetadataRequest, MetadataResult},
    modal_action::ModalAction,
    notify_signal::KeepAliveNotifySignalHandle,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Zip,
    Modrinth,
    Curseforge,
}

#[derive(Debug, Clone)]
pub struct ExportModrinthOptions {
    pub name: Arc<str>,
    pub version: Arc<str>,
    pub summary: Option<Arc<str>>,
}

#[derive(Debug, Clone)]
pub struct ExportCurseforgeOptions {
    pub name: Arc<str>,
    pub version: Arc<str>,
    pub author: Option<Arc<str>>,
    pub recommended_ram: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct ExportOptions {
    pub include_saves: bool,
    pub include_mods: bool,
    pub include_resourcepacks: bool,
    pub include_shaders: bool,
    pub include_configs: bool,
    pub include_screenshots: bool,
    pub include_backups: bool,
    pub include_logs: bool,
    pub include_cache: bool,
    pub include_synced: bool,
    pub modrinth: ExportModrinthOptions,
    pub curseforge: ExportCurseforgeOptions,
}

pub enum MessageToBackend {
    GetSaveGroups {
        id: InstanceID,
        channel: tokio::sync::oneshot::Sender<Result<Vec<SaveGroupSummary>, String>>,
    },
    CreateSaveGroup {
        id: InstanceID,
        name: String,
        channel: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    JoinSaveGroup {
        id: InstanceID,
        group_id: uuid::Uuid,
        channel: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    LeaveSaveGroup {
        id: InstanceID,
        channel: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    RenameSaveGroup {
        group_id: uuid::Uuid,
        name: String,
        channel: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    RequestMetadata {
        request: MetadataRequest,
        force_reload: bool,
    },
    CreateInstance {
        name: Ustr,
        version: Ustr,
        loader: Loader,
        icon: Option<EmbeddedOrRaw>,
    },
    DeleteInstance {
        id: InstanceID,
    },
    DuplicateInstance {
        id: InstanceID,
        name: Ustr,
        modal_action: ModalAction,
    },
    CreateLocalBranch {
        create_save_group: bool,
        reuse_parent_icon: bool,
        icon_hue_degrees: i32,
        id: InstanceID,
        name: Ustr,
        modal_action: ModalAction,
    },
    ExportInstance {
        id: InstanceID,
        format: ExportFormat,
        options: ExportOptions,
        output: PathBuf,
        modal_action: ModalAction,
    },
    RenameInstance {
        id: InstanceID,
        name: Ustr,
    },
    SetInstanceMinecraftVersion {
        id: InstanceID,
        version: Ustr,
    },
    SetInstanceLoader {
        id: InstanceID,
        loader: Loader,
    },
    SetInstanceUpdateChannel {
        id: InstanceID,
        update_channel: UpdateChannel,
    },
    SetInstancePreferredAccount {
        id: InstanceID,
        account: Option<Uuid>,
    },
    SetInstancePreferredLoaderVersion {
        id: InstanceID,
        loader_version: Option<&'static str>,
    },
    SetInstanceDisableFileSyncing {
        id: InstanceID,
        disable_file_syncing: bool,
    },
    SetInstanceSandboxing {
        id: InstanceID,
        sandbox: bool,
    },
    SetInstanceMemory {
        id: InstanceID,
        memory: InstanceMemoryConfiguration,
    },
    SetInstanceWrapperCommand {
        id: InstanceID,
        wrapper_command: InstanceWrapperCommandConfiguration,
    },
    SetInstanceJvmFlags {
        id: InstanceID,
        jvm_flags: InstanceJvmFlagsConfiguration,
    },
    SetInstanceJvmBinary {
        id: InstanceID,
        jvm_binary: InstanceJvmBinaryConfiguration,
    },
    SetInstanceLinuxWrapper {
        id: InstanceID,
        linux_wrapper: InstanceLinuxWrapperConfiguration,
    },
    SetInstanceSystemLibraries {
        id: InstanceID,
        system_libraries: InstanceSystemLibrariesConfiguration,
    },
    SetInstanceIcon {
        id: InstanceID,
        icon: Option<EmbeddedOrRaw>,
    },
    KillInstance {
        id: InstanceID,
    },
    StartInstanceByName {
        name: String,
        quick_play: Option<QuickPlayLaunch>,
        modal_action: ModalAction,
    },
    /// Command-line Start: runs the inherited update gate before entering the launch pipeline.
    CliStartInstanceByName {
        name: String,
        modal_action: ModalAction,
        update_policy: CliUpdatePolicy,
        outcome: tokio::sync::oneshot::Sender<CliStartOutcome>,
    },
    /// Command-line instance management (list, info, create, update, delete). Never opens a window or starts a game.
    CliInstanceCommand {
        command: CliInstanceCommand,
        outcome: tokio::sync::oneshot::Sender<CliStartOutcome>,
    },
    StartInstance {
        id: InstanceID,
        quick_play: Option<QuickPlayLaunch>,
        live_game_output: Option<tokio::sync::oneshot::Sender<tokio::sync::mpsc::UnboundedReceiver<GameOutputMsg>>>,
        modal_action: ModalAction,
    },
    RepairGameFiles {
        id: InstanceID,
        modal_action: ModalAction,
    },
    RequestLoadWorlds {
        id: InstanceID,
    },
    RequestLoadServers {
        id: InstanceID,
    },
    ReorderServers {
        id: InstanceID,
        from_index: usize,
        to_index: usize,
    },
    RequestLoadContentFolder {
        id: InstanceID,
        content_folder: ContentFolder,
    },
    SetContentEnabled {
        id: InstanceID,
        content_ids: Vec<InstanceContentID>,
        enabled: bool,
    },
    SetContentChildEnabled {
        id: InstanceID,
        content_id: InstanceContentID,
        child_id: Option<Arc<str>>,
        child_name: Option<Arc<str>>,
        child_filename: Arc<str>,
        disabled_default: bool,
        enabled: bool,
    },
    DownloadContentChildren {
        id: InstanceID,
        content_id: InstanceContentID,
        modal_action: ModalAction,
    },
    DeleteContent {
        id: InstanceID,
        content_ids: Vec<InstanceContentID>,
    },
    InstallContent {
        content: ContentInstall,
        modal_action: ModalAction,
    },
    CreateInstanceFromFile {
        file: PathBuf,
        modal_action: ModalAction,
    },
    DownloadAllMetadata,
    UpdateCheck {
        instance: InstanceID,
        modal_action: ModalAction,
    },
    UpdateContent {
        instance: InstanceID,
        content_id: InstanceContentID,
        modal_action: ModalAction,
    },
    UnzipModpack {
        id: InstanceID,
        content_id: InstanceContentID,
        modal_action: ModalAction,
    },
    Sleep5s,
    ReadLog {
        path: Arc<Path>,
        send: tokio::sync::mpsc::Sender<Arc<str>>,
    },
    GetLogFiles {
        instance: InstanceID,
        channel: tokio::sync::oneshot::Sender<LogFiles>,
    },
    GetImportFromOtherLauncherJob {
        channel: tokio::sync::oneshot::Sender<Option<ImportFromOtherLauncherJob>>,
        launcher: OtherLauncher,
        path: Arc<Path>,
    },
    GetSyncState {
        channel: tokio::sync::oneshot::Sender<SyncState>,
    },
    GetBackendConfiguration {
        channel: tokio::sync::oneshot::Sender<BackendConfig>,
    },
    SetIgnoredProfilePaths {
        paths: Vec<String>,
    },
    AddIgnoredProfilePath {
        path: String,
        channel: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    RefreshHomeServer {
        id: InstanceID,
    },
    GetNews {
        channel: tokio::sync::oneshot::Sender<Result<schema::news::NewsFeed, String>>,
    },
    GetNewsImages {
        requests: Vec<schema::news::NewsImageRequest>,
        channel: tokio::sync::oneshot::Sender<Vec<schema::news::NewsLoadedImage>>,
    },
    GetHomePlayerHeads {
        uuids: Vec<String>,
        channel: tokio::sync::oneshot::Sender<Vec<(String, std::path::PathBuf)>>,
    },
    GetHomePresentation {
        id: Option<InstanceID>,
        channel: tokio::sync::oneshot::Sender<Result<crate::home::HomePresentation, String>>,
    },
    GetGlobalProfiles {
        channel: tokio::sync::oneshot::Sender<Result<Vec<GlobalProfileSummary>, String>>,
    },
    GetProfileOverwrites {
        id: InstanceID,
        channel: tokio::sync::oneshot::Sender<Result<crate::profile_overwrites::ProfileOverwritesReport, String>>,
    },
    ReadProfileTextFile {
        id: InstanceID,
        path: String,
        channel: tokio::sync::oneshot::Sender<Result<crate::profile_overwrites::ProfileTextFile, String>>,
    },
    ReadProfileAncestorText {
        id: InstanceID,
        path: String,
        level: usize,
        channel: tokio::sync::oneshot::Sender<Result<crate::profile_overwrites::ProfileAncestorText, String>>,
    },
    SaveProfileTextFile {
        id: InstanceID,
        path: String,
        contents: String,
        expected_sha256: String,
        channel: tokio::sync::oneshot::Sender<Result<crate::profile_overwrites::ProfileTextFile, String>>,
    },
    ToggleProfileMod {
        id: InstanceID,
        path: String,
        channel: tokio::sync::oneshot::Sender<Result<String, String>>,
    },
    RestoreProfileFile {
        id: InstanceID,
        path: String,
        source: crate::profile_overwrites::RestoreSource,
        channel: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    CheckInheritedUpdates {
        id: InstanceID,
        channel: tokio::sync::oneshot::Sender<Result<Vec<String>, String>>,
    },
    UpdateInheritedChain {
        id: InstanceID,
        channel: tokio::sync::oneshot::Sender<Result<(), String>>,
        modal_action: ModalAction,
    },
    CheckDistributionConnection,
    CreateGlobalProfileInstance {
        save_group_target: Option<GlobalProfileSaveGroupTarget>,
        name: String,
        profile_id: String,
        revision_id: String,
        sequence: i64,
        manifest_sha256: String,
        modal_action: ModalAction,
    },
    GetGlobalProfileSaveGroupTargets {
        profile_id: String,
        channel: tokio::sync::oneshot::Sender<Result<Vec<GlobalProfileSaveGroupTarget>, String>>,
    },
    UpdateGlobalProfileInstance {
        id: InstanceID,
    },
    SetSyncing {
        target: Arc<str>,
        is_file: bool,
        value: bool,
    },
    CleanupOldLogFiles {
        instance: InstanceID,
    },
    UploadLogFile {
        path: Arc<Path>,
        modal_action: ModalAction,
    },
    AddNewAccount {
        modal_action: ModalAction,
    },
    AddOfflineAccount {
        name: Arc<str>,
        uuid: Uuid,
    },
    SelectAccount {
        uuid: Uuid,
    },
    DeleteAccount {
        uuid: Uuid,
    },
    ReorderAccounts {
        from_index: usize,
        delta: isize,
    },
    SetProxyConfiguration {
        config: ProxyConfig,
    },
    SetDistributionConfiguration {
        config: DistributionConfig,
    },
    SetLauncherChannel {
        channel: schema::backend_config::LauncherChannel,
    },
    SetProxyPassword {
        password: String,
    },
    CreateInstanceShortcut {
        id: InstanceID,
        path: PathBuf,
    },
    RelocateInstance {
        id: InstanceID,
        path: PathBuf,
    },
    InstallUpdate {
        update: UpdatePrompt,
        modal_action: ModalAction,
    },
    ImportFromOtherLauncher {
        launcher: OtherLauncher,
        import_job: ImportFromOtherLauncherJob,
        modal_action: ModalAction,
    },
    GetAccountSkin {
        account: Uuid,
        result: tokio::sync::oneshot::Sender<AccountSkinResult>,
    },
    SetAccountSkin {
        account: Uuid,
        skin: UniqueBytes,
        variant: SkinVariant,
    },
    /// The profile JVM options of an instance and what the next Start would do with each.
    /// `None` when the instance has no profile state.
    GetProfileJvmOptions {
        id: InstanceID,
        channel: tokio::sync::oneshot::Sender<Option<schema::profile_jvm::ProfileJvmOptionsView>>,
    },
    /// Stores the player's choice for one option; an untouched choice removes the entry so the
    /// option follows the profile again.
    SetInstanceProfileJvmOption {
        id: InstanceID,
        option_id: String,
        choice: schema::profile_jvm::ProfileJvmOptionChoice,
    },
    /// "Optimize launch (Java 25 AOT cache)" for one instance. Turning it on clears an automatic
    /// disable; turning it off removes the instance's cache.
    SetInstanceAotCache {
        id: InstanceID,
        enabled: bool,
    },
    /// Cancels and postpones the AOT cache optimization of one instance, or of every instance with
    /// `None` (closing the launcher). A training profile already written is kept.
    /// Deletes the AOT cache of an idle instance (the player asked for it).
    DeleteAotCache {
        id: InstanceID,
    },
    CancelAotOptimization {
        id: Option<InstanceID>,
    },
    GetAccountCapes {
        account: Uuid,
        result: tokio::sync::oneshot::Sender<AccountCapesResult>,
    },
    SetAccountCape {
        account: Uuid,
        cape: Option<Uuid>,
    },
    RequestSkinLibrary,
    RemoveFromSkinLibrary {
        skin: UniqueBytes,
    },
    AddToSkinLibrary {
        source: UrlOrFile,
    },
    CopyPlayerSkin {
        username: Arc<str>,
    },
    Login {
        account: Uuid,
        modal_action: ModalAction,
    },
    Quit,
}

#[derive(Debug, Clone)]
pub struct SaveGroupSummary {
    pub id: uuid::Uuid,
    pub name: String,
    pub member_count: usize,
    pub selected: bool,
}

/// State of an instance's JDK 25 AOT cache, as the instance settings show it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AotCacheSummary {
    /// The option is off.
    Off,
    /// No cache yet: the next game records what it loads.
    #[default]
    NotBuilt,
    /// The running game records what it loads.
    Training,
    /// Recorded; the launcher still has to build the cache.
    Waiting,
    /// A cache is ready (its size on disk).
    Ready { bytes: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AotOptimizationPhase {
    /// The game is closed; its JVM writes the training profile.
    Recording,
    /// The launcher builds the cache from the profile (`-XX:AOTMode=create`).
    Creating,
}

#[derive(Debug)]
pub enum MessageToFrontend {
    InstanceAdded {
        id: InstanceID,
        name: Ustr,
        icon: Option<UniqueBytes>,
        save_group_id: Option<uuid::Uuid>,
        root_path: Arc<Path>,
        dot_minecraft_folder: Arc<Path>,
        configuration: InstanceConfiguration,
        playtime: InstancePlaytime,
        worlds_state: BridgeDataLoadState,
        servers_state: BridgeDataLoadState,
        content_states: enum_map::EnumMap<ContentFolder, BridgeDataLoadState>,
        aot_cache: AotCacheSummary,
    },
    InstanceRemoved {
        id: InstanceID,
    },
    InstanceSaveGroupUpdated {
        id: InstanceID,
        group_id: Option<uuid::Uuid>,
    },
    InstanceModified {
        id: InstanceID,
        name: Ustr,
        icon: Option<UniqueBytes>,
        root_path: Arc<Path>,
        dot_minecraft_folder: Arc<Path>,
        configuration: InstanceConfiguration,
        playtime: InstancePlaytime,
        status: InstanceStatus,
        aot_cache: AotCacheSummary,
    },
    InstancePlaytimeUpdated {
        id: InstanceID,
        playtime: InstancePlaytime,
    },
    /// Approximate progress (0..1) of the AOT cache optimization of an instance, sent about once a
    /// second while its status is `Optimizing`.
    InstanceOptimizationProgress {
        id: InstanceID,
        progress: f32,
        phase: AotOptimizationPhase,
    },
    InstanceWorldsUpdated {
        id: InstanceID,
        worlds: Arc<[InstanceWorldSummary]>,
    },
    InstanceServersUpdated {
        id: InstanceID,
        servers: Arc<[InstanceServerSummary]>,
    },
    InstanceContentUpdated {
        id: InstanceID,
        content_folder: ContentFolder,
        content: Arc<[InstanceContentSummary]>,
    },
    AddNotification {
        notification_type: BridgeNotificationType,
        message: Arc<str>,
    },
    AccountsUpdated {
        accounts: Arc<[Account]>,
        selected_account: Option<Uuid>,
    },
    Refresh,
    Quit,
    CloseModal,
    MoveInstanceToTop {
        id: InstanceID,
    },
    MetadataResult {
        request: MetadataRequest,
        result: Result<MetadataResult, Arc<str>>,
        keep_alive_handle: Option<KeepAliveNotifySignalHandle>,
    },
    SkinLibraryUpdated {
        skin_library: SkinLibrary,
    },
    UpdateAvailable {
        update: UpdatePrompt,
    },
    OpenOrFocusMainWindow {
        /// One-based display number from the platform's active display list.
        monitor: Option<usize>,
    },
    ManualCurseforgeDownloadsRequired {
        request: ManualCurseforgeDownloadRequest,
    },
}

#[derive(Debug, Clone)]
pub struct GlobalProfileSummary {
    pub parent_profile_id: Option<String>,
    pub description: String,
    pub minecraft: String,
    pub loader: schema::loader::Loader,
    pub loader_version: String,
    pub icon_path: Option<PathBuf>,
    pub profile_id: String,
    pub name: String,
    pub latest_revision_id: String,
    pub latest_sequence: i64,
    pub latest_manifest_sha256: String,
    pub stable_revision_id: Option<String>,
    pub stable_sequence: Option<i64>,
    pub stable_manifest_sha256: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GlobalProfileSaveGroupTarget {
    pub anchor_id: InstanceID,
    pub group_id: Option<uuid::Uuid>,
    pub name: String,
}

#[derive(Debug, Default)]
pub struct LogFiles {
    pub paths: Vec<Arc<Path>>,
    pub total_gzipped_size: usize,
}

#[derive(Debug)]
pub struct SyncTargetState {
    pub enabled: bool,
    pub is_file: bool,
    pub sync_count: usize,
    pub cannot_sync_count: usize,
    pub cannot_sync_instances: Vec<Arc<str>>,
}

#[derive(Debug)]
pub struct SyncState {
    pub sync_folder: Arc<Path>,
    pub targets: BTreeMap<Arc<str>, SyncTargetState>,
    pub total_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BridgeNotificationType {
    Success,
    Info,
    Error,
    Warning,
}

#[derive(Clone, Debug)]
pub struct BridgeDataLoadState(Arc<AtomicU64>);

impl Default for BridgeDataLoadState {
    fn default() -> Self {
        Self(Arc::new(AtomicU64::new(BridgeDataLoadState::UNLOADED)))
    }
}

impl BridgeDataLoadState {
    const LOADING: u64 = 1;
    const OBSERVED: u64 = 2;
    const DIRTY: u64 = 4;
    const CANCELLED_BY_LAUNCH: u64 = 8;
    const UNLOADED: u64 = 16;
    const STATE_MASK: u64 = 0xff;
    const GENERATION_ONE: u64 = 1 << 8;

    fn state(value: u64) -> u64 {
        value & Self::STATE_MASK
    }

    pub fn should_load(&self) -> bool {
        // Initial state is loadable. Afterwards it must be observed and dirty,
        // and launch cancellation/loading always suppresses a new request.
        let value = Self::state(self.0.load(std::sync::atomic::Ordering::Acquire));
        if value & (Self::LOADING | Self::CANCELLED_BY_LAUNCH) != 0 {
            return false;
        }
        value & Self::UNLOADED != 0 || value & (Self::OBSERVED | Self::DIRTY) == (Self::OBSERVED | Self::DIRTY)
    }

    pub fn is_not_unloaded(&self) -> bool {
        Self::state(self.0.load(std::sync::atomic::Ordering::Acquire)) & Self::UNLOADED == 0
    }

    pub fn set_observed(&self) {
        self.0.fetch_or(Self::OBSERVED, std::sync::atomic::Ordering::AcqRel);
    }

    pub fn set_dirty(&self) {
        self.0.fetch_or(Self::DIRTY, std::sync::atomic::Ordering::AcqRel);
    }

    pub fn load_started(&self) -> u64 {
        let mut current = self.0.load(std::sync::atomic::Ordering::Acquire);
        loop {
            // A load task may have been queued before Start and reach this
            // state only after launch cancellation. Never let load_started
            // erase that cancellation bit.
            let cancelled = Self::state(current) & Self::CANCELLED_BY_LAUNCH;
            let next = (current & !Self::STATE_MASK) | Self::LOADING | cancelled;
            match self.0.compare_exchange_weak(
                current,
                next,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            ) {
                Ok(_) => return next / Self::GENERATION_ONE,
                Err(observed) => current = observed,
            }
        }
    }

    pub fn load_finished(&self) {
        self.0.fetch_and(!Self::LOADING, std::sync::atomic::Ordering::AcqRel);
    }

    pub fn generation(&self) -> u64 {
        self.0.load(std::sync::atomic::Ordering::Acquire) / Self::GENERATION_ONE
    }

    pub fn is_generation_current(&self, generation: u64) -> bool {
        self.generation() == generation
    }

    pub fn cancel_for_launch(&self) {
        let mut current = self.0.load(std::sync::atomic::Ordering::Acquire);
        loop {
            let generation = (current & !Self::STATE_MASK).wrapping_add(Self::GENERATION_ONE);
            let next = generation | Self::CANCELLED_BY_LAUNCH;
            match self.0.compare_exchange_weak(
                current,
                next,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            ) {
                Ok(_) => return,
                Err(observed) => current = observed,
            }
        }
    }

    pub fn is_cancelled_by_launch(&self) -> bool {
        Self::state(self.0.load(std::sync::atomic::Ordering::Acquire)) & Self::CANCELLED_BY_LAUNCH != 0
    }

    pub fn resume_after_launch(&self) {
        let mut current = self.0.load(std::sync::atomic::Ordering::Acquire);
        loop {
            let next = (current & !Self::STATE_MASK) | Self::DIRTY;
            match self.0.compare_exchange_weak(
                current,
                next,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            ) {
                Ok(_) => return,
                Err(observed) => current = observed,
            }
        }
    }
}

#[cfg(test)]
mod bridge_data_load_state_tests {
    use super::BridgeDataLoadState;

    #[test]
    fn default_state_is_loadable_without_looking_launch_cancelled() {
        let state = BridgeDataLoadState::default();

        assert!(state.should_load());
        assert!(!state.is_not_unloaded());
        assert!(!state.is_cancelled_by_launch());
    }

    #[test]
    fn queued_load_start_cannot_clear_launch_cancellation() {
        let state = BridgeDataLoadState::default();
        state.cancel_for_launch();
        let cancelled_generation = state.generation();

        let started_generation = state.load_started();

        assert_eq!(started_generation, cancelled_generation);
        assert!(state.is_cancelled_by_launch());
        assert!(!state.should_load());
    }

    #[test]
    fn launch_cancel_invalidates_inflight_generation_until_resumed() {
        let state = BridgeDataLoadState::default();
        let generation = state.load_started();
        assert!(state.is_generation_current(generation));

        state.cancel_for_launch();
        assert!(state.is_cancelled_by_launch());
        assert!(!state.is_generation_current(generation));
        assert!(!state.should_load());

        state.resume_after_launch();
        state.set_observed();
        assert!(!state.is_cancelled_by_launch());
        assert!(state.should_load());
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuickPlayLaunch {
    Singleplayer(OsString),
    Multiplayer(OsString),
    Realms(OsString),
}

#[derive(Debug, Clone)]
pub enum EmbeddedOrRaw {
    Embedded(Arc<str>),
    Raw(UniqueBytes),
}

#[derive(Debug, Clone)]
pub enum AccountSkinResult {
    Success {
        skin: Option<UniqueBytes>,
        variant: SkinVariant,
    },
    NeedsLogin,
    UnableToLoadSkin,
}

#[derive(Debug, Clone)]
pub enum AccountCapesResult {
    Success {
        capes: Vec<MinecraftProfileCape>,
    },
    NeedsLogin,
}

#[derive(Clone, Debug)]
pub struct SkinLibrary {
    pub state: BridgeDataLoadState,
    pub skins: Arc<[UniqueBytes]>,
    pub folder: Arc<Path>,
}

pub enum UrlOrFile {
    Url { url: Arc<str> },
    File { path: PathBuf },
}

pub struct GameOutputMsg {
    pub time: i64,
    pub level: GameOutputLogLevel,
    pub text: Arc<[Arc<str>]>,
}

/// What a command-line Start does when an inherited update is pending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CliUpdatePolicy {
    /// No flag: report pending updates and exit without starting.
    #[default]
    Ask,
    /// `--update`: apply the same update as the UI's "Update and start", then start.
    Update,
    /// `--no-update`: start the installed files unchanged.
    NoUpdate,
}

/// Which revision of a global profile a command-line create or update uses.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum CliRevision {
    /// What the GUI uses: the `stable` revision channel, or the latest revision if the profile has none.
    #[default]
    Selected,
    /// `--revision latest`.
    Latest,
    /// `--revision rev_<id>`: that exact signed revision.
    Id(String),
}

/// Instance management from the command line. They go through the same backend paths as the GUI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliInstanceCommand {
    List {
        json: bool,
    },
    Info {
        name: String,
        json: bool,
    },
    Create {
        name: String,
        profile_id: String,
        revision: CliRevision,
    },
    Update {
        name: String,
        revision: CliRevision,
    },
    Delete {
        name: String,
        confirm: String,
    },
}

/// Lines to print and the process exit code for a command-line Start request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliStartOutcome {
    pub lines: Vec<String>,
    pub exit_code: u8,
}
