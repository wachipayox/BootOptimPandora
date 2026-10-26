//! Incremental cosmetic news cache. It never participates in the signed launch gate.
use super::*;
use schema::news::{NewsCategory, NewsFeed, NewsItem, NewsProfile};
use std::{sync::LazyLock, time::Instant};

#[derive(Clone, Default, Serialize, Deserialize)]
struct NewsState {
    cursor: Option<String>,
    etag: Option<String>,
    server_time: String,
    categories: Vec<NewsCategory>,
    items: BTreeMap<String, NewsItem>,
}
#[derive(Deserialize)]
struct NewsDelta {
    schema_version: u32,
    server_time: String,
    cursor: String,
    full: bool,
    categories: Vec<NewsCategory>,
    items: Vec<NewsItem>,
    deleted: Vec<String>,
}
impl NewsState {
    fn merge(&mut self, delta: NewsDelta) -> Result<(), DistributionError> {
        if delta.schema_version != 1
            || delta.cursor.is_empty()
            || chrono::DateTime::parse_from_rfc3339(&delta.server_time).is_err()
            || (!delta.full && self.cursor.is_none())
        {
            return Err(DistributionError::InvalidResponse("Invalid news snapshot".into()));
        }
        let mut next = self.clone();
        if delta.full {
            next.items.clear();
        }
        for item in delta.items {
            if item.id.is_empty()
                || item.body.len() > 65536
                || chrono::DateTime::parse_from_rfc3339(&item.published_at).is_err()
                || chrono::DateTime::parse_from_rfc3339(&item.updated_at).is_err()
                || item.expires_at.as_ref().is_some_and(|s| chrono::DateTime::parse_from_rfc3339(s).is_err())
            {
                return Err(DistributionError::InvalidResponse("Invalid news item".into()));
            }
            next.items.insert(item.id.clone(), item);
        }
        for id in delta.deleted {
            next.items.remove(&id);
        }
        if next.items.len() > 2000 {
            return Err(DistributionError::ResponseTooLarge);
        }
        next.categories = delta.categories;
        next.categories.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.id.cmp(&b.id)));
        next.server_time = delta.server_time;
        next.cursor = Some(delta.cursor);
        *self = next;
        Ok(())
    }
    fn feed(self, load_failed: bool, origin_key: &str) -> NewsFeed {
        NewsFeed {
            origin_key: origin_key.to_owned(),
            server_time: self.server_time,
            cursor: self.cursor.unwrap_or_default(),
            categories: self.categories,
            items: self.items.into_values().collect(),
            load_failed,
            ..Default::default()
        }
    }
}
#[derive(Default)]
struct FetchGate {
    attempted: Option<Instant>,
    failed: bool,
}
static FETCH_GATES: LazyLock<parking_lot::Mutex<BTreeMap<PathBuf, Arc<tokio::sync::Mutex<FetchGate>>>>> =
    LazyLock::new(|| parking_lot::Mutex::new(BTreeMap::new()));

impl DistributionClient {
    pub(super) fn cached_news_snapshot(&self, root: &Path) -> Result<NewsFeed, DistributionError> {
        let key = hex::encode(Sha256::digest(self.base_url.as_str().as_bytes()));
        let state: NewsState = serde_json::from_slice(&fs::read(root.join(format!("{key}.json")))?)
            .map_err(|e| DistributionError::InvalidResponse(e.to_string()))?;
        Ok(state.feed(false, &key))
    }
    pub(super) async fn news_snapshot(&self, root: &Path) -> Result<NewsFeed, DistributionError> {
        fs::create_dir_all(root)?;
        let key = hex::encode(Sha256::digest(self.base_url.as_str().as_bytes()));
        let path = root.join(format!("{key}.json"));
        let gate = FETCH_GATES.lock().entry(path.clone()).or_default().clone();
        let mut gate = gate.lock().await;
        let cached = fs::read(&path).ok().and_then(|b| serde_json::from_slice::<NewsState>(&b).ok());
        // The first request in each process always synchronizes. The gate prevents
        // Home and News from issuing duplicate requests or polling faster than 5 min.
        if gate.attempted.is_some_and(|t| t.elapsed() < Duration::from_secs(300)) {
            return cached
                .map(|s| s.feed(gate.failed, &key))
                .ok_or_else(|| DistributionError::InvalidResponse("News unavailable".into()));
        }
        let result = async {
            let mut state = cached.clone().unwrap_or_default();
            let mut request = self.client.get(self.join("/v1/news")?);
            if let Some(cursor) = &state.cursor {
                request = request.query(&[("since", cursor)]);
            }
            if let Some(etag) = &state.etag {
                request = request.header(reqwest::header::IF_NONE_MATCH, etag);
            }
            let mut response = request.send().await?;
            if response.status() == reqwest::StatusCode::NOT_MODIFIED {
                if state.cursor.is_none() {
                    return Err(DistributionError::InvalidResponse("News 304 without cache".into()));
                }
                if let Some(now) = response
                    .headers()
                    .get(reqwest::header::DATE)
                    .and_then(|h| h.to_str().ok())
                    .and_then(|s| chrono::DateTime::parse_from_rfc2822(s).ok())
                {
                    state.server_time = now.with_timezone(&chrono::Utc).to_rfc3339();
                }
            } else {
                if !response.status().is_success() {
                    return Err(DistributionError::HttpStatus(response.status()));
                }
                let etag = response
                    .headers()
                    .get(reqwest::header::ETAG)
                    .and_then(|h| h.to_str().ok())
                    .map(str::to_owned);
                let mut body = Vec::new();
                while let Some(chunk) = response.chunk().await? {
                    if body.len().saturating_add(chunk.len()) > 140 * 1024 * 1024 {
                        return Err(DistributionError::ResponseTooLarge);
                    }
                    body.extend_from_slice(&chunk);
                }
                let delta =
                    serde_json::from_slice(&body).map_err(|e| DistributionError::InvalidResponse(e.to_string()))?;
                state.merge(delta)?;
                state.etag = etag;
            }
            let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
            fs::write(&temporary, serde_json::to_vec(&state).unwrap())?;
            fs::rename(temporary, &path)?;
            Ok::<_, DistributionError>(state)
        }
        .await;
        gate.attempted = Some(Instant::now());
        gate.failed = result.is_err();
        match result {
            Ok(state) => Ok(state.feed(false, &key)),
            Err(error) => cached.map(|s| s.feed(true, &key)).ok_or(error),
        }
    }
}
impl crate::BackendState {
    pub async fn load_profile_changelog(
        &self,
        profile_id: String,
        query: schema::changelog::ChangelogQuery,
    ) -> Result<schema::changelog::ProfileChangelog, String> {
        if !valid_identifier(&profile_id, "profile_")
            || [&query.after, &query.until, &query.revision]
                .into_iter()
                .flatten()
                .any(|id| !valid_identifier(id, "rev_"))
        {
            return Err("Invalid changelog identifiers".into());
        }
        let config = self.config.lock().get().distribution.clone();
        let client = DistributionClient::new(&config).map_err(|e| e.to_string())?;
        let mut url = client.join(&format!("/v1/profiles/{profile_id}/changelog")).map_err(|e| e.to_string())?;
        {
            let mut pairs = url.query_pairs_mut();
            for (key, value) in [
                ("after", query.after),
                ("until", query.until),
                ("revision", query.revision),
                ("cursor", query.cursor),
            ] {
                if let Some(value) = value {
                    pairs.append_pair(key, &value);
                }
            }
        }
        let response = client.client.get(url).send().await.map_err(|e| e.to_string())?;
        let value: schema::changelog::ProfileChangelog =
            decode_json_response(response).await.map_err(|e| e.to_string())?;
        if value.schema_version != 1 || value.profile_id != profile_id {
            return Err("Invalid changelog response".into());
        }
        Ok(value)
    }
    pub async fn load_news_feed(&self) -> Result<NewsFeed, String> {
        // Explicit local input for the existing UI capture tool. Cosmetic only:
        // it cannot supply catalogs, manifests, installation objects or launch authority.
        if std::env::var("WBOMBO_UI_CAPTURE_PAGE").is_ok_and(|p| p == "home-news" || p.starts_with("news-")) {
            if let Some(path) = std::env::var_os("WBOMBO_UI_CAPTURE_NEWS") {
                let bytes = fs::read(path).map_err(|_| "Cannot read news capture fixture".to_owned())?;
                if bytes.len() > 140 * 1024 * 1024 {
                    return Err("News capture fixture too large".into());
                }
                let mut feed: NewsFeed =
                    serde_json::from_slice(&bytes).map_err(|_| "Invalid news capture fixture".to_owned())?;
                feed.origin_key = "capture-fixture".into();
                return Ok(feed);
            }
        }
        let config = self.config.lock().get().distribution.clone();
        let include_tests = self.config.lock().get().channel.includes_test_profiles();
        let client = DistributionClient::new(&config).map_err(|e| e.to_string())?;
        let root = &self.directories.root_launcher_dir;
        let mut feed = client.news_snapshot(&root.join("news-state")).await.map_err(|e| e.to_string())?;
        let route = if include_tests {
            "/v1/profiles?include_test=1"
        } else {
            "/v1/profiles"
        };
        let catalog = client.cosmetic_document(route, &root.join("home-presentation"), 300).await;
        feed.load_failed |= catalog.is_err();
        let catalog = catalog.unwrap_or(serde_json::Value::Null);
        feed.profiles = catalog["profiles"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| serde_json::from_value::<NewsProfile>(v.clone()).ok())
            .collect();
        Ok(feed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn item(id: &str, title: &str) -> NewsItem {
        NewsItem {
            id: id.into(),
            title: title.into(),
            published_at: "2026-10-23T10:00:00Z".into(),
            updated_at: "2026-10-23T10:00:00Z".into(),
            ..Default::default()
        }
    }
    fn delta(full: bool, cursor: &str, items: Vec<NewsItem>, deleted: Vec<&str>) -> NewsDelta {
        NewsDelta {
            schema_version: 1,
            server_time: "2026-10-23T10:00:00Z".into(),
            cursor: cursor.into(),
            full,
            categories: vec![],
            items,
            deleted: deleted.into_iter().map(str::to_owned).collect(),
        }
    }
    #[test]
    fn full_replaces_cache_and_incremental_updates_and_deletes() {
        let mut state = NewsState::default();
        state
            .merge(delta(true, "n1.opaque", vec![item("one", "old"), item("two", "keep")], vec![]))
            .unwrap();
        state
            .merge(delta(false, "n1.next", vec![item("one", "edited"), item("three", "new")], vec!["two"]))
            .unwrap();
        assert_eq!(state.items["one"].title, "edited");
        assert!(!state.items.contains_key("two"));
        assert!(state.items.contains_key("three"));
        assert_eq!(state.cursor.as_deref(), Some("n1.next"));
        state.merge(delta(true, "n1.reset", vec![item("only", "full")], vec![])).unwrap();
        assert_eq!(state.items.keys().cloned().collect::<Vec<_>>(), vec!["only"]);
    }
    #[test]
    fn invalid_delta_does_not_mutate_last_good_cache() {
        let mut state = NewsState::default();
        assert!(state.merge(delta(false, "cursor", vec![], vec![])).is_err());
        state.merge(delta(true, "good", vec![item("one", "keep")], vec![])).unwrap();
        let mut invalid = item("two", "bad");
        invalid.updated_at = "invalid".into();
        assert!(state.merge(delta(false, "bad", vec![item("one", "edited"), invalid], vec![])).is_err());
        assert_eq!(state.items["one"].title, "keep");
        assert_eq!(state.cursor.as_deref(), Some("good"));
    }
    #[tokio::test]
    async fn etag_cursor_clock_throttle_and_offline_cache_roundtrip() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            sync::atomic::{AtomicUsize, Ordering},
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let recorded = requests.clone();
        let body = serde_json::json!({"schema_version":1,"server_time":"2026-10-23T10:00:00Z","cursor":"n1.opaque","full":true,"categories":[],"items":[item("one","cached")],"deleted":[]}).to_string();
        let server = std::thread::spawn(move || {
            let mut headers = Vec::new();
            for index in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut request = Vec::new();
                loop {
                    let mut buffer = [0; 1024];
                    let count = stream.read(&mut buffer).unwrap();
                    assert!(count > 0 && request.len() < 16384);
                    request.extend_from_slice(&buffer[..count]);
                    if request.windows(4).any(|b| b == b"\r\n\r\n") {
                        break;
                    }
                }
                headers.push(String::from_utf8(request).unwrap());
                recorded.fetch_add(1, Ordering::SeqCst);
                let response = if index == 0 {
                    format!(
                        "HTTP/1.1 200 OK\r\nETag: \"n1.opaque\"\r\nDate: Fri, 23 Oct 2026 10:00:00 GMT\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                } else {
                    "HTTP/1.1 304 Not Modified\r\nETag: \"n1.opaque\"\r\nDate: Fri, 23 Oct 2026 10:01:00 GMT\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned()
                };
                stream.write_all(response.as_bytes()).unwrap();
            }
            headers
        });
        // The loopback-only client is constructed inside the test. Production
        // continues to require certificate-verified HTTPS in DistributionClient::new.
        let client = DistributionClient {
            client: Client::builder().no_proxy().timeout(Duration::from_secs(5)).build().unwrap(),
            base_url: Url::parse(&format!("http://{address}")).unwrap(),
            trusted_keys: BTreeMap::new(),
            server_keys: Arc::new(tokio::sync::OnceCell::new()),
        };
        let root = std::env::temp_dir().join(format!("pandora-news-cache-{}", uuid::Uuid::new_v4()));
        let path = root.join(format!("{}.json", hex::encode(Sha256::digest(client.base_url.as_str().as_bytes()))));
        let first = client.news_snapshot(&root).await.unwrap();
        assert_eq!(first.items[0].title, "cached");
        assert_eq!(first.server_time, "2026-10-23T10:00:00Z");
        let cached = client.news_snapshot(&root).await.unwrap();
        assert_eq!(cached.cursor, "n1.opaque");
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        // Removing only the process gate simulates reopening the launcher with
        // the existing disk cache, without sleeping five minutes in a test.
        FETCH_GATES.lock().remove(&path);
        let revalidated = client.news_snapshot(&root).await.unwrap();
        assert_eq!(revalidated.server_time, "2026-10-23T10:01:00+00:00");
        let headers = server.join().unwrap();
        assert!(headers[1].starts_with("GET /v1/news?since=n1.opaque "));
        assert!(headers[1].to_ascii_lowercase().contains("if-none-match: \"n1.opaque\""));
        FETCH_GATES.lock().remove(&path);
        let offline = client.news_snapshot(&root).await.unwrap();
        assert!(offline.load_failed);
        assert_eq!(offline.items, revalidated.items);
        assert_eq!(offline.server_time, revalidated.server_time);
        FETCH_GATES.lock().remove(&path);
        fs::remove_file(path).unwrap();
        fs::remove_dir(root).unwrap();
    }
}
