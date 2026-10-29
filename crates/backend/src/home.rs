//! Cosmetic reads and immutable image cache, separate from signed installation.
use super::*;
use bridge::{
    home::{HomeNews, HomePresentation},
    instance::InstanceID,
};
use serde_json::Value;

#[derive(Serialize, Deserialize)]
struct CachedDocument {
    etag: Option<String>,
    value: Value,
    fetched_at: u64,
}
fn home_image_cache_suffix(kind: &str) -> &str {
    if kind == "news" { "news-tone-v1" } else { kind }
}
fn tone_home_thumbnail(image: &mut image::RgbaImage) {
    // Android Home: ColorMatrix saturation .75, followed by RGB scale .8.
    for pixel in image.pixels_mut() {
        let luma = 0.213 * pixel[0] as f32 + 0.715 * pixel[1] as f32 + 0.072 * pixel[2] as f32;
        for channel in 0..3 {
            pixel[channel] = ((luma + (pixel[channel] as f32 - luma) * 0.75) * 0.8).round().clamp(0., 255.) as u8;
        }
    }
}
impl DistributionClient {
    fn cached_cosmetic_document(&self, route: &str, root: &Path) -> Option<Value> {
        let key = hex::encode(Sha256::digest(format!("{}{}", self.base_url, route).as_bytes()));
        fs::read(root.join(format!("{key}.json")))
            .ok()
            .and_then(|b| serde_json::from_slice::<CachedDocument>(&b).ok())
            .map(|c| c.value)
    }

    async fn home_image_mode(&self, value: &Value, root: &Path, kind: &str, cache_only: bool) -> Option<PathBuf> {
        if !cache_only {
            return self.home_image(value, root, kind).await;
        }
        let key = if let Some(url) = value.get("url").and_then(Value::as_str) {
            hex::encode(Sha256::digest(Url::parse(url).ok()?.as_str().as_bytes()))
        } else {
            let object: ManifestObjectRef = serde_json::from_value(value.clone()).ok()?;
            if object.sha256.len() != 64 || !object.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            object.sha256
        };
        let path = root.join(format!("{key}-{}.png", home_image_cache_suffix(kind)));
        path.is_file().then_some(path)
    }
    pub(super) async fn cosmetic_document(
        &self,
        route: &str,
        root: &Path,
        interval: u64,
    ) -> Result<Value, DistributionError> {
        fs::create_dir_all(root)?;
        let key = hex::encode(Sha256::digest(format!("{}{}", self.base_url, route).as_bytes()));
        let path = root.join(format!("{key}.json"));
        let cached = fs::read(&path).ok().and_then(|b| serde_json::from_slice::<CachedDocument>(&b).ok());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if let Some(c) = &cached {
            if now.saturating_sub(c.fetched_at) < interval {
                return Ok(c.value.clone());
            }
        }
        let result = async {
            let mut request = self.client.get(self.join(route)?);
            if let Some(tag) = cached.as_ref().and_then(|c| c.etag.as_ref()) {
                request = request.header(reqwest::header::IF_NONE_MATCH, tag);
            }
            let mut response = request.send().await?;
            if response.status() == reqwest::StatusCode::NOT_MODIFIED {
                let cached = cached
                    .as_ref()
                    .ok_or_else(|| DistributionError::InvalidResponse("304 without cache".into()))?;
                let value = cached.value.clone();
                return Ok(CachedDocument {
                    etag: cached.etag.clone(),
                    value,
                    fetched_at: now,
                });
            }
            if !response.status().is_success() {
                return Err(DistributionError::HttpStatus(response.status()));
            }
            let etag = response
                .headers()
                .get(reqwest::header::ETAG)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await? {
                if bytes.len().saturating_add(chunk.len()) > 140 * 1024 * 1024 {
                    return Err(DistributionError::ResponseTooLarge);
                }
                bytes.extend_from_slice(&chunk);
            }
            let value =
                serde_json::from_slice(&bytes).map_err(|e| DistributionError::InvalidResponse(e.to_string()))?;
            Ok(CachedDocument {
                etag,
                value,
                fetched_at: now,
            })
        }
        .await;
        match result {
            Ok(doc) => {
                let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
                fs::write(&temporary, serde_json::to_vec(&doc).unwrap())?;
                fs::rename(temporary, &path)?;
                Ok(doc.value)
            },
            Err(error) => cached.map(|c| c.value).ok_or(error),
        }
    }

    async fn home_image(&self, value: &Value, root: &Path, kind: &str) -> Option<PathBuf> {
        let result: Result<PathBuf, DistributionError> = async {
            fs::create_dir_all(root)?;
            let (source, key) = if let Some(url) = value.get("url").and_then(Value::as_str) {
                let url = Url::parse(url).map_err(|_| DistributionError::InvalidBaseUrl)?;
                if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
                    return Err(DistributionError::InvalidBaseUrl);
                }
                let key = hex::encode(Sha256::digest(url.as_str().as_bytes()));
                let path = root.join(format!("{key}.external"));
                if !path.exists() {
                    let http = Client::builder()
                        .https_only(true)
                        .timeout(Duration::from_secs(30))
                        .redirect(reqwest::redirect::Policy::limited(3))
                        .build()?;
                    let mut response = http.get(url).send().await?;
                    if !response.status().is_success() {
                        return Err(DistributionError::HttpStatus(response.status()));
                    }
                    let mut bytes = Vec::new();
                    while let Some(chunk) = response.chunk().await? {
                        if bytes.len().saturating_add(chunk.len()) > 10 * 1024 * 1024 {
                            return Err(DistributionError::ResponseTooLarge);
                        }
                        bytes.extend_from_slice(&chunk);
                    }
                    fs::write(&path, bytes)?;
                }
                (path, key)
            } else {
                let object: ManifestObjectRef = serde_json::from_value(value.clone())
                    .map_err(|e| DistributionError::InvalidResponse(e.to_string()))?;
                let limit = if kind == "logo" {
                    16
                } else if kind == "artwork" {
                    6
                } else {
                    4
                };
                if object.size > limit * 1024 * 1024
                    || !["image/png", "image/jpeg"].contains(&object.media_type.as_str())
                {
                    return Err(DistributionError::ResponseTooLarge);
                }
                (self.fetch_verified_object(&object, root).await?, object.sha256)
            };
            let output = root.join(format!("{key}-{}.png", home_image_cache_suffix(kind)));
            if output.exists() {
                return Ok(output);
            }
            let reader = image::ImageReader::open(&source)?.with_guessed_format()?;
            let dimensions = reader.into_dimensions().map_err(|e| DistributionError::InvalidResponse(e.to_string()))?;
            let max = if kind == "logo" {
                8192
            } else if kind == "artwork" {
                1920
            } else {
                4096
            };
            if dimensions.0 == 0
                || dimensions.1 == 0
                || dimensions.0 > max
                || dimensions.1 > max
                || (kind == "artwork" && dimensions != (1920, 760))
            {
                return Err(DistributionError::ResponseTooLarge);
            }
            let mut decoded = image::ImageReader::open(&source)?
                .with_guessed_format()?
                .decode()
                .map_err(|e| DistributionError::InvalidResponse(e.to_string()))?;
            if kind == "logo" {
                decoded = decoded.thumbnail(740, 200);
            }
            if kind == "news" {
                let mut cover = decoded.resize_to_fill(488, 274, image::imageops::FilterType::Triangle).to_rgba8();
                tone_home_thumbnail(&mut cover);
                for y in 0..12 {
                    for x in 0..12 - y {
                        cover.get_pixel_mut(x, y)[3] = 0;
                        cover.get_pixel_mut(487 - x, y)[3] = 0;
                    }
                }
                decoded = image::DynamicImage::ImageRgba8(cover);
            }
            decoded
                .save_with_format(&output, image::ImageFormat::Png)
                .map_err(|e| DistributionError::InvalidResponse(e.to_string()))?;
            if kind == "artwork" {
                let crop = decoded
                    .resize_to_fill(1060, 440, image::imageops::FilterType::Triangle)
                    .crop_imm(380, 330, 250, 52);
                let mut glass = crop.blur(14.).to_rgba8();
                for p in glass.pixels_mut() {
                    let l = 0.213 * p[0] as f32 + 0.715 * p[1] as f32 + 0.072 * p[2] as f32;
                    for c in 0..3 {
                        p[c] = (l + (p[c] as f32 - l) * 1.5).round().clamp(0., 255.) as u8;
                    }
                }
                glass
                    .save(root.join(format!("{key}-glass.png")))
                    .map_err(|e| DistributionError::InvalidResponse(e.to_string()))?;
            }
            Ok(output)
        }
        .await;
        result.ok()
    }
}

impl crate::BackendState {
    pub async fn load_home_presentation(
        &self,
        id: Option<InstanceID>,
        cache_only: bool,
        include_news: bool,
    ) -> Result<HomePresentation, String> {
        let config = self.config.lock().get().distribution.clone();
        let include_tests = self.config.lock().get().channel.includes_test_profiles();
        let client = DistributionClient::new(&config).map_err(|e| e.to_string())?;
        let cache = self.directories.root_launcher_dir.join("home-presentation");
        let route = if include_tests {
            "/v1/profiles?include_test=1"
        } else {
            "/v1/profiles"
        };
        let catalog = if cache_only {
            client.cached_cosmetic_document(route, &cache).unwrap_or(Value::Null)
        } else {
            client.cosmetic_document(route, &cache, 300).await.unwrap_or(Value::Null)
        };
        let profiles = catalog.get("profiles").and_then(Value::as_array).cloned().unwrap_or_default();
        let root = id.and_then(|id| self.instance_state.read().instances.get(id).map(|i| i.root_path.clone()));
        let branch = root
            .as_ref()
            .and_then(|root| crate::profile_layout_flow::read_committed_branch(root).ok().flatten());
        let mod_count = branch
            .as_ref()
            .map(|b| b.entries.keys().filter(|p| p.starts_with("mods/") && p.ends_with(".jar")).count());
        let profile_id = branch.and_then(|b| b.lineage.global_ancestor.map(|p| p.profile_id));
        let profile = profiles
            .iter()
            .find(|p| p.get("profile_id").and_then(Value::as_str) == profile_id.as_deref());
        let last_destination = id.and_then(|id| {
            self.instance_state
                .write()
                .instances
                .get_mut(id)
                .and_then(|i| i.stats.get().last_destination.clone())
        });
        let last_world_icon = id.and_then(|id| {
            self.instance_state.write().instances.get_mut(id).and_then(|instance| {
                let folder = instance.stats.get().last_world_folder.clone()?;
                if !crate::fs::is_single_component_path_str(&folder) {
                    return None;
                }
                let icon = instance.saves_path.join(folder).join("icon.png");
                icon.is_file().then_some(icon)
            })
        });
        let mut out = HomePresentation {
            last_destination,
            last_world_icon,
            profile_id: profile_id.clone(),
            mod_count,
            recommended_memory_mib: id.map(|_| crate::launch::memory::recommendation().recommended_mib),
            ..Default::default()
        };
        if let Some(profile) = profile {
            if let Some(art) = profile.get("artwork") {
                if let Some(value) = art.get("image") {
                    out.artwork = client.home_image_mode(value, &cache, "artwork", cache_only).await;
                }
                out.artwork_author = art.get("author").and_then(Value::as_str).map(str::to_owned);
                out.glass = out
                    .artwork
                    .as_ref()
                    .map(|p| PathBuf::from(p.to_string_lossy().replace("-artwork.png", "-glass.png")))
                    .filter(|p| p.exists());
            }
            if let Some(logo) = profile.get("logo") {
                out.logo = client.home_image_mode(logo, &cache, "logo", cache_only).await;
            }
        }
        if let Some(id) = id {
            let overrides = self.instance_state.write().instances.get_mut(id).map(|i| {
                let c = i.configuration.get();
                (c.home_artwork.clone(), c.home_logo.clone())
            });
            if let Some((artwork, logo)) = overrides {
                if let Some(path) = artwork {
                    if let Some(value) = cache_local_image(&path, &cache, true) {
                        if let Some(path) = client.home_image_mode(&value, &cache, "artwork", cache_only).await {
                            out.artwork = Some(path);
                            out.artwork_author = None;
                        }
                        out.glass = out
                            .artwork
                            .as_ref()
                            .map(|p| PathBuf::from(p.to_string_lossy().replace("-artwork.png", "-glass.png")))
                            .filter(|p| p.exists());
                    }
                }
                if let Some(path) = logo {
                    if let Some(value) = cache_local_image(&path, &cache, false) {
                        if let Some(path) = client.home_image_mode(&value, &cache, "logo", cache_only).await {
                            out.logo = Some(path);
                        }
                    }
                }
            }
        }
        if !include_news {
            return Ok(out);
        }
        let news_root = self.directories.root_launcher_dir.join("news-state");
        #[cfg(feature = "ui-capture")]
        let capture_news = std::env::var_os("WBOMBO_UI_CAPTURE_NEWS").is_some();
        #[cfg(not(feature = "ui-capture"))]
        let capture_news = false;
        let feed = match if capture_news {
            self.load_news_feed().await
        } else if cache_only {
            client.cached_news_snapshot(&news_root).map_err(|e| e.to_string())
        } else {
            client.news_snapshot(&news_root).await.map_err(|e| e.to_string())
        } {
            Ok(v) => v,
            Err(_) => {
                out.news_failed = true;
                return Ok(out);
            },
        };
        out.news_origin_key = feed.origin_key.clone();
        out.news_failed = feed.load_failed;
        let news = serde_json::to_value(feed).map_err(|e| e.to_string())?;
        let lineage = home_lineage(&profiles, profile_id);
        let server_time = news["server_time"].as_str().and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok());
        if let Some(items) = news["items"].as_array() {
            for n in items {
                let target = n["target_profile_id"].as_str();
                if !home_news_visible(n, &lineage, server_time) {
                    continue;
                }
                let category = news["categories"]
                    .as_array()
                    .and_then(|c| c.iter().find(|c| c["id"] == n["category_id"]))
                    .and_then(|c| c["name"].as_str())
                    .unwrap_or("");
                out.news.push(HomeNews {
                    id: n["id"].as_str().unwrap_or("").into(),
                    title: n["title"].as_str().unwrap_or("").into(),
                    body: n["body"].as_str().unwrap_or("").into(),
                    author: n["author"].as_str().unwrap_or("").into(),
                    category: category.into(),
                    target_profile_id: target.map(str::to_owned),
                    pinned: n["pinned"].as_bool().unwrap_or(false),
                    published_at: n["published_at"].as_str().unwrap_or("").into(),
                    image: None,
                });
            }
            out.news
                .sort_by(|a, b| b.pinned.cmp(&a.pinned).then_with(|| b.published_at.cmp(&a.published_at)));
            // Keep enough items for the independent General toggle; render four at a time.

            let visible: BTreeSet<_> = out
                .news
                .iter()
                .take(4)
                .chain(out.news.iter().filter(|n| n.target_profile_id.is_some()).take(4))
                .map(|n| n.id.clone())
                .collect();
            for n in out.news.iter_mut().filter(|n| visible.contains(&n.id)) {
                if let Some(raw) = items.iter().find(|v| v["id"].as_str() == Some(&n.id)) {
                    n.image = client.home_image_mode(&raw["image"], &cache, "news", cache_only).await;
                }
            }
        }
        Ok(out)
    }
}

impl crate::BackendState {
    pub async fn home_player_heads(&self, uuids: Vec<String>) -> Vec<(String, PathBuf)> {
        let root = self.directories.root_launcher_dir.join("player-heads");
        let _ = fs::create_dir_all(&root);
        let Ok(client) = Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10))
            .build()
        else {
            return Vec::new();
        };
        let mut heads = Vec::new();
        for id in uuids.into_iter().take(12) {
            let Ok(uuid) = uuid::Uuid::parse_str(&id) else {
                continue;
            };
            if uuid.is_nil() {
                continue;
            }
            let path = root.join(format!("{}.png", uuid.simple()));
            if path.exists() {
                heads.push((id, path));
                continue;
            }
            let result = async {
                let bytes = bounded_get(
                    &client,
                    &format!("https://sessionserver.mojang.com/session/minecraft/profile/{}", uuid.simple()),
                    128 * 1024,
                )
                .await?;
                let body: Value = serde_json::from_slice(&bytes).ok()?;
                let encoded =
                    body["properties"].as_array()?.iter().find(|p| p["name"].as_str() == Some("textures"))?["value"]
                        .as_str()?;
                let decoded = base64::engine::general_purpose::STANDARD.decode(encoded).ok()?;
                let textures: Value = serde_json::from_slice(&decoded).ok()?;
                let mut url = Url::parse(textures["textures"]["SKIN"]["url"].as_str()?).ok()?;
                if url.host_str() != Some("textures.minecraft.net") {
                    return None;
                }
                url.set_scheme("https").ok()?;
                let bytes = bounded_get(&client, url.as_str(), 1024 * 1024).await?;
                let reader = image::ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format().ok()?;
                let dimensions = reader.into_dimensions().ok()?;
                if dimensions.0 != 64 || ![32, 64].contains(&dimensions.1) {
                    return None;
                }
                let skin = image::load_from_memory(&bytes).ok()?;
                let mut face = skin.crop_imm(8, 8, 8, 8).to_rgba8();
                image::imageops::overlay(&mut face, &skin.crop_imm(40, 8, 8, 8).to_rgba8(), 0, 0);
                let face = image::imageops::resize(&face, 24, 24, image::imageops::FilterType::Nearest);
                face.save(&path).ok()?;
                Some(())
            }
            .await;
            if result.is_some() {
                heads.push((id, path));
            }
        }
        heads
    }
}
async fn bounded_get(client: &Client, url: &str, max: usize) -> Option<Vec<u8>> {
    let mut response = client.get(url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if bytes.len().saturating_add(chunk.len()) > max {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    Some(bytes)
}

impl crate::BackendState {
    /// Validate and normalize outside the UI thread. Keep a launcher-owned copy
    /// so moving the selected source does not break the instance presentation.
    pub(crate) fn set_instance_home_image(
        &self,
        id: InstanceID,
        path: Option<&Path>,
        artwork: bool,
    ) -> Result<(), String> {
        let error = || "No se ha podido abrir la imagen.".to_owned();
        let root = self.instance_state.read().instances.get(id).ok_or_else(error)?.root_path.clone();
        let stored = if let Some(path) = path {
            let cache = self.directories.root_launcher_dir.join("home-presentation");
            let value = cache_local_image(path, &cache, artwork).ok_or_else(error)?;
            let sha = value["sha256"].as_str().ok_or_else(error)?;
            let directory = root.join(if artwork { "launcher-artwork" } else { "launcher-logo" });
            fs::create_dir_all(&directory).map_err(|_| error())?;
            let name = path.file_stem().ok_or_else(error)?;
            let mut target = directory.join(name);
            target.set_extension("png");
            let bytes = fs::read(cache.join("sha256").join(&sha[..2]).join(sha)).map_err(|_| error())?;
            crate::fs::write_safe(&target, &bytes).map_err(|_| error())?;
            Some(target)
        } else {
            None
        };
        let mut state = self.instance_state.write();
        let instance = state.instances.get_mut(id).ok_or_else(error)?;
        instance.configuration.modify(|configuration| {
            if artwork {
                configuration.home_artwork = stored;
            } else {
                configuration.home_logo = stored;
            }
        });
        self.send.send(instance.create_modify_message());
        Ok(())
    }
}

fn cache_local_image(path: &Path, root: &Path, artwork: bool) -> Option<Value> {
    if fs::metadata(path).ok()?.len() > 16 * 1024 * 1024 {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    if bytes.len() > 16 * 1024 * 1024 {
        return None;
    }
    // The input digest changes whenever the user's file changes. Reuse the
    // previously normalized output instead of decoding and rewriting a large
    // image on every launcher start.
    let kind = if artwork { "artwork" } else { "logo" };
    let input_hash = hex::encode(Sha256::digest(&bytes));
    let index = root.join(format!("local-v1-{input_hash}-{kind}.json"));
    if let Some(value) = fs::read(&index).ok().and_then(|data| serde_json::from_slice::<Value>(&data).ok()) {
        if let Some(hash) = value.get("sha256").and_then(Value::as_str) {
            if hash.len() == 64
                && hash.bytes().all(|b| b.is_ascii_hexdigit())
                && root.join(format!("{hash}-{kind}.png")).is_file()
            {
                return Some(value);
            }
        }
    }
    let dimensions = image::ImageReader::new(std::io::Cursor::new(&bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()?;
    if dimensions.0 == 0 || dimensions.1 == 0 || dimensions.0 > 8192 || dimensions.1 > 8192 {
        return None;
    }
    let mut image = image::load_from_memory(&bytes).ok()?;
    if artwork {
        image = image.resize_to_fill(1920, 760, image::imageops::FilterType::Triangle);
    } else {
        image = image.thumbnail(740, 200);
    }
    let mut encoded = std::io::Cursor::new(Vec::new());
    image.write_to(&mut encoded, image::ImageFormat::Png).ok()?;
    let encoded = encoded.into_inner();
    let sha = hex::encode(Sha256::digest(&encoded));
    let dir = root.join("sha256").join(&sha[..2]);
    fs::create_dir_all(&dir).ok()?;
    crate::fs::write_safe(&dir.join(&sha), &encoded).ok()?;
    // Local overrides are immediately usable by cache-only startup reads.
    crate::fs::write_safe(&root.join(format!("{sha}-{kind}.png")), &encoded).ok()?;
    let value = serde_json::json!({"sha256":sha,"size":encoded.len(),"media_type":"image/png"});
    // Failure to persist the acceleration index must not hide a valid image.
    if let Ok(data) = serde_json::to_vec(&value) {
        let _ = crate::fs::write_safe(&index, &data);
    }
    Some(value)
}

fn home_lineage(profiles: &[Value], mut current: Option<String>) -> BTreeSet<String> {
    let mut lineage = BTreeSet::new();
    while let Some(id) = current {
        let Some(profile) = profiles.iter().find(|p| p["profile_id"].as_str() == Some(&id)) else {
            break;
        };
        if lineage.len() >= 9 || !lineage.insert(id.clone()) {
            break;
        }
        current = profile["parent_profile_id"].as_str().map(str::to_owned);
    }
    lineage
}

fn home_news_visible(
    n: &Value,
    lineage: &BTreeSet<String>,
    server_time: Option<chrono::DateTime<chrono::FixedOffset>>,
) -> bool {
    let target = n["target_profile_id"].as_str();
    !target.is_some_and(|id| !lineage.contains(id))
        && !n["expires_at"]
            .as_str()
            .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
            .zip(server_time)
            .is_some_and(|(expiry, now)| expiry <= now)
}
#[cfg(test)]
mod home_tests {
    use super::*;
    #[test]
    fn home_news_thumbnail_uses_android_tone_and_preserves_alpha() {
        let mut image = image::RgbaImage::new(2, 1);
        image.put_pixel(0, 0, image::Rgba([255, 0, 0, 127]));
        image.put_pixel(1, 0, image::Rgba([128, 128, 128, 255]));
        tone_home_thumbnail(&mut image);
        assert_eq!(image.get_pixel(0, 0).0, [164, 11, 11, 127]);
        assert_eq!(image.get_pixel(1, 0).0, [102, 102, 102, 255]);
    }
    #[test]
    fn local_cosmetic_cache_reuses_unchanged_files_and_recovers_missing_output() {
        let root = std::env::temp_dir().join(format!("pandora-local-cosmetic-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let input = root.join("input.png");
        image::RgbaImage::from_pixel(8, 8, image::Rgba([1, 2, 3, 255])).save(&input).unwrap();
        let first = cache_local_image(&input, &root, false).unwrap();
        let hash = first["sha256"].as_str().unwrap();
        let object = root.join("sha256").join(&hash[..2]).join(hash);
        fs::remove_file(&object).unwrap();
        assert_eq!(cache_local_image(&input, &root, false), Some(first.clone()));
        assert!(!object.exists());
        fs::remove_file(root.join(format!("{hash}-logo.png"))).unwrap();
        assert_eq!(cache_local_image(&input, &root, false), Some(first.clone()));
        assert!(object.is_file());
        image::RgbaImage::from_pixel(8, 8, image::Rgba([4, 5, 6, 255])).save(&input).unwrap();
        assert_ne!(cache_local_image(&input, &root, false), Some(first));
        fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn startup_cosmetics_read_stale_or_missing_cache_without_contacting_server() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let client = DistributionClient {
            client: Client::builder().no_proxy().build().unwrap(),
            base_url: Url::parse(&format!("http://{address}")).unwrap(),
            trusted_keys: BTreeMap::new(),
            server_keys: Arc::new(tokio::sync::OnceCell::new()),
        };
        let root = std::env::temp_dir().join(format!("pandora-startup-cache-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let route = "/v1/profiles";
        assert!(client.cached_cosmetic_document(route, &root).is_none());
        let key = hex::encode(Sha256::digest(format!("{}{}", client.base_url, route).as_bytes()));
        let value = serde_json::json!({"profiles":[]});
        fs::write(
            root.join(format!("{key}.json")),
            serde_json::to_vec(&CachedDocument {
                value: value.clone(),
                etag: Some("old".into()),
                fetched_at: 0,
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(client.cached_cosmetic_document(route, &root), Some(value));
        let image = serde_json::json!({"url":format!("https://{address}/missing.png")});
        assert!(client.home_image_mode(&image, &root, "artwork", true).await.is_none());
        assert_eq!(listener.accept().unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn only_active_ancestors_and_general_not_daughters() {
        let lineage = ["profile_child".to_string(), "profile_parent".to_string()].into_iter().collect();
        for (target, expected) in [
            (Some("profile_child"), true),
            (Some("profile_parent"), true),
            (None, true),
            (Some("profile_daughter"), false),
        ] {
            assert_eq!(
                home_news_visible(&serde_json::json!({"target_profile_id":target}), &lineage, None),
                expected
            );
        }
    }
    #[test]
    fn expired_pin_is_hidden_at_server_time_including_offsets() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-23T10:00:00Z").ok();
        let lineage = BTreeSet::new();
        assert!(!home_news_visible(
            &serde_json::json!({"pinned":true,"expires_at":"2026-10-23T12:00:00+02:00"}),
            &lineage,
            now
        ));
        assert!(home_news_visible(
            &serde_json::json!({"expires_at":"2026-10-23T10:00:00.001Z"}),
            &lineage,
            now
        ));
    }
    #[test]
    fn missing_catalog_targets_are_not_visible() {
        let profiles = vec![serde_json::json!({"profile_id":"profile_child","parent_profile_id":"profile_deleted"})];
        let lineage = home_lineage(&profiles, Some("profile_child".into()));
        assert!(lineage.contains("profile_child"));
        assert!(!lineage.contains("profile_deleted"));
        assert!(!home_news_visible(
            &serde_json::json!({"target_profile_id":"profile_deleted"}),
            &lineage,
            None
        ));
    }
    #[test]
    fn server_player_sample_is_optional_and_parsed() {
        let a: schema::server_status::StatusPlayers =
            serde_json::from_value(serde_json::json!({"online":1,"max":20})).unwrap();
        assert!(a.sample.is_empty());
        let a:schema::server_status::StatusPlayers=serde_json::from_value(serde_json::json!({"online":1,"max":20,"sample":[{"id":"00000000-0000-0000-0000-000000000001","name":"Example"}]})).unwrap();
        assert_eq!(a.sample[0].name.as_ref(), "Example");
    }
}
