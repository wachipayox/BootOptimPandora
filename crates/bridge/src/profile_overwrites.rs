#[derive(Clone, Debug)]
pub struct ProfileOverwrite {
    pub path: String,
    pub change: OverwriteChange,
}

#[derive(Clone, Debug)]
pub struct ProfileFile {
    pub path: String,
    pub comparison_path: String,
    pub size: u64,
    pub editable: bool,
}

#[derive(Clone, Debug)]
pub struct ProfileTextFile {
    pub path: String,
    pub contents: String,
    pub sha256: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverwriteChange {
    Added,
    AddedDisabled,
    Modified,
    Removed,
    Disabled,
    Enabled,
    ModifiedAndDisabled,
    ModifiedAndEnabled,
}

#[derive(Clone, Debug)]
pub struct ProfileOverwritesReport {
    pub files: Vec<ProfileFile>,
    pub local_parent: Option<String>,
    pub local_changes: Vec<ProfileOverwrite>,
    pub local_error: Option<String>,
    pub global_profile: Option<String>,
    pub global_changes: Vec<ProfileOverwrite>,
    pub global_error: Option<String>,
    pub skipped_entries: usize,
}
