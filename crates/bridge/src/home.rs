//! Mutable Home presentation; never installation authority.
use std::path::PathBuf;

#[derive(Clone, Debug, Default)]
pub struct HomePresentation {
    pub profile_id: Option<String>,
    pub mod_count: Option<usize>,
    pub last_destination: Option<String>,
    pub artwork: Option<PathBuf>,
    pub artwork_author: Option<String>,
    pub logo: Option<PathBuf>,
    pub glass: Option<PathBuf>,
    pub news: Vec<HomeNews>,
    pub news_origin_key: String,
    pub news_failed: bool,
}
#[derive(Clone, Debug)]
pub struct HomeNews {
    pub id: String,
    pub title: String,
    pub body: String,
    pub author: String,
    pub category: String,
    pub target_profile_id: Option<String>,
    pub pinned: bool,
    pub published_at: String,
    pub image: Option<PathBuf>,
}
