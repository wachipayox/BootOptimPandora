//! Mutable cosmetic news, independent of signed profile revisions.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug)]
pub enum NewsImageSource {
    Reference(NewsImage),
    CasDigest(String),
}
#[derive(Clone, Copy, Debug, Default)]
pub enum ImageCorners {
    #[default]
    None,
    Top,
    All,
}
#[derive(Clone, Debug)]
pub struct NewsImageRequest {
    pub key: String,
    pub source: NewsImageSource,
    pub width: u32,
    pub height: u32,
    pub cover: bool,
    pub corners: ImageCorners,
}
#[derive(Clone, Debug)]
pub struct NewsLoadedImage {
    pub key: String,
    pub path: Option<std::path::PathBuf>,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NewsLocalState {
    pub read: BTreeSet<String>,
    pub saved: BTreeSet<String>,
}
impl NewsLocalState {
    pub fn sync(&mut self, feed: &NewsFeed) {
        let valid: BTreeSet<_> = feed.items.iter().map(|n| n.id.as_str()).collect();
        self.read.retain(|id| valid.contains(id.as_str()));
        self.saved.retain(|id| valid.contains(id.as_str()));
    }
    pub fn toggle_saved(&mut self, id: &str) {
        if !self.saved.remove(id) {
            self.saved.insert(id.to_owned());
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NewsImage {
    Cas {
        sha256: String,
        size: u64,
        media_type: String,
    },
    External {
        url: String,
    },
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewsCategory {
    pub id: String,
    pub name: String,
    pub order: i64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkedRevision {
    pub profile_id: String,
    pub revision_id: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewsItem {
    pub id: String,
    pub title: String,
    pub body: String,
    pub author: String,
    pub category_id: String,
    pub target_profile_id: Option<String>,
    pub image: Option<NewsImage>,
    pub pinned: bool,
    pub published_at: String,
    pub updated_at: String,
    pub expires_at: Option<String>,
    pub linked_revision: Option<LinkedRevision>,
}
impl NewsItem {
    pub fn expired(&self, server_time: &str) -> bool {
        self.expires_at
            .as_deref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .zip(chrono::DateTime::parse_from_rfc3339(server_time).ok())
            .is_some_and(|(expiry, now)| expiry <= now)
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewsProfile {
    pub profile_id: String,
    pub parent_profile_id: Option<String>,
    pub name: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NewsFeed {
    pub origin_key: String,
    pub server_time: String,
    pub cursor: String,
    pub categories: Vec<NewsCategory>,
    pub items: Vec<NewsItem>,
    pub profiles: Vec<NewsProfile>,
    pub load_failed: bool,
}
impl NewsFeed {
    pub fn lineage(&self, id: &str) -> BTreeSet<String> {
        let mut result = BTreeSet::new();
        let mut current = Some(id);
        while let Some(id) = current {
            let Some(profile) = self.profiles.iter().find(|p| p.profile_id == id) else {
                break;
            };
            if result.len() >= 9 || !result.insert(id.to_owned()) {
                break;
            }
            current = profile.parent_profile_id.as_deref();
        }
        result
    }
    /// `None` removes the profile filter. Explicit selections include each
    /// selected profile's ancestors. General news is always included.
    pub fn matches_profiles(&self, item: &NewsItem, selection: Option<&BTreeSet<String>>) -> bool {
        let Some(target) = &item.target_profile_id else {
            return true;
        };
        if !self.profiles.iter().any(|p| &p.profile_id == target) {
            return false;
        }
        selection.is_none_or(|ids| ids.iter().any(|id| self.lineage(id).contains(target)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_families_include_ancestors_not_descendants_or_missing_targets() {
        let feed = NewsFeed {
            profiles: vec![
                NewsProfile {
                    profile_id: "parent".into(),
                    ..Default::default()
                },
                NewsProfile {
                    profile_id: "child".into(),
                    parent_profile_id: Some("parent".into()),
                    ..Default::default()
                },
                NewsProfile {
                    profile_id: "daughter".into(),
                    parent_profile_id: Some("child".into()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let selected = ["child".to_owned()].into_iter().collect();
        for (id, expected) in [
            (None, true),
            (Some("parent"), true),
            (Some("child"), true),
            (Some("daughter"), false),
            (Some("deleted"), false),
        ] {
            let item = NewsItem {
                target_profile_id: id.map(str::to_owned),
                ..Default::default()
            };
            assert_eq!(feed.matches_profiles(&item, Some(&selected)), expected);
        }
        assert!(feed.matches_profiles(
            &NewsItem {
                target_profile_id: Some("daughter".into()),
                ..Default::default()
            },
            None
        ));
    }
    #[test]
    fn expiration_uses_server_time_and_offsets() {
        let item = NewsItem {
            expires_at: Some("2026-10-23T12:00:00+02:00".into()),
            ..Default::default()
        };
        assert!(!item.expired("2026-10-23T09:59:59Z"));
        assert!(item.expired("2026-10-23T10:00:00Z"));
    }
    #[test]
    fn deleted_news_is_removed_from_read_and_saved_marks() {
        let mut local = NewsLocalState {
            read: ["keep".into(), "deleted".into()].into_iter().collect(),
            saved: ["keep".into(), "deleted".into()].into_iter().collect(),
        };
        local.sync(&NewsFeed {
            items: vec![NewsItem {
                id: "keep".into(),
                ..Default::default()
            }],
            ..Default::default()
        });
        assert_eq!(local.saved, ["keep".to_owned()].into_iter().collect());
        assert_eq!(local.read, local.saved);
        local.toggle_saved("keep");
        assert!(local.saved.is_empty());
        local.toggle_saved("keep");
        assert!(local.saved.contains("keep"));
    }
}
