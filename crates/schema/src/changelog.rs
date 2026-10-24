//! Cosmetic revision history, independent of the signed installation manifest.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ChangelogQuery {
    pub after: Option<String>,
    pub until: Option<String>,
    pub revision: Option<String>,
    pub cursor: Option<String>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ProfileChangelog {
    pub schema_version: u32,
    pub profile_id: String,
    pub lineage: Vec<ChangelogProfile>,
    pub entries: Vec<ChangelogEntry>,
    pub next_cursor: Option<String>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ChangelogProfile {
    pub profile_id: String,
    pub name: String,
    pub depth: u32,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ChangelogOrigin {
    pub profile_id: String,
    pub revision_id: String,
    pub sequence: u64,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ChangelogRevision {
    pub revision_id: String,
    pub sequence: u64,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ChangelogEntry {
    pub origin: ChangelogOrigin,
    pub revision: Option<ChangelogRevision>,
    pub created_at: String,
    pub title: String,
    pub description: String,
    pub changes: Vec<String>,
    pub mods: ChangelogMods,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ChangelogMods {
    pub added: Vec<ChangelogMod>,
    pub removed: Vec<ChangelogMod>,
    pub updated: Vec<ChangelogModUpdate>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ChangelogMod {
    pub id: String,
    pub file: String,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ChangelogModUpdate {
    pub id: String,
    pub from: String,
    pub to: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inherited_history_and_attached_revisions_preserve_origin_and_mod_changes() {
        let raw = r#"{"schema_version":1,"profile_id":"profile_example","lineage":[{"profile_id":"profile_parent","name":"Parent","depth":1}],"entries":[{"origin":{"profile_id":"profile_parent","revision_id":"rev_parent","sequence":44},"revision":null,"created_at":"2026-10-23T10:00:00Z","title":"Title","description":"Description","changes":["Change"],"mods":{"added":[{"id":"new","file":"new.jar"}],"removed":[],"updated":[{"id":"mod","from":"old.jar","to":"current.jar"}]}}],"next_cursor":"opaque"}"#;
        let history: ProfileChangelog = serde_json::from_str(raw).unwrap();
        assert_eq!(history.entries[0].origin.sequence, 44);
        assert!(history.entries[0].revision.is_none());
        assert_eq!(history.entries[0].mods.updated[0].to, "current.jar");
        let attached = raw.replace("\"revision\":null", "\"revision\":{\"revision_id\":\"rev_child\",\"sequence\":12}");
        let attached: ProfileChangelog = serde_json::from_str(&attached).unwrap();
        assert_eq!(attached.entries[0].revision.as_ref().unwrap().sequence, 12);
        assert_eq!(attached.entries[0].origin.profile_id, "profile_parent");
    }
}
