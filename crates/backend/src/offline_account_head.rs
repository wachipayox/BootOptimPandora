//! Cosmetic offline-account faces; never changes the account's login identity.
use crate::BackendState;
use image::{DynamicImage, ImageFormat};
use schema::unique_bytes::UniqueBytes;
use serde::{Deserialize, Serialize};
use std::{
    io::Cursor,
    path::Path,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

const FRESH_SECONDS: u64 = 24 * 60 * 60;
const RETRY_SECONDS: u64 = 15 * 60;

#[derive(Serialize, Deserialize)]
struct CachedHead {
    username: String,
    checked_at: u64,
    success: bool,
    head: Option<UniqueBytes>,
}
impl CachedHead {
    fn fresh(&self, username: &str, now: u64) -> bool {
        self.username == username
            && now.saturating_sub(self.checked_at)
                < if self.success && self.head.is_some() {
                    FRESH_SECONDS
                } else {
                    RETRY_SECONDS
                }
    }
}
fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}
fn read_cache(path: &Path, username: &str) -> Option<CachedHead> {
    if std::fs::metadata(path).ok()?.len() > 64 * 1024 {
        return None;
    }
    let cache: CachedHead = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    (cache.username == username).then_some(cache)
}
fn write_cache(path: &Path, cache: &CachedHead) -> bool {
    serde_json::to_vec(cache)
        .ok()
        .is_some_and(|bytes| crate::fs::write_safe(path, &bytes).is_ok())
}
async fn bounded_body(response: reqwest::Response, limit: usize) -> Option<Vec<u8>> {
    let mut response = response.error_for_status().ok()?;
    if response.content_length().is_some_and(|n| n > limit as u64) {
        return None;
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    Some(bytes)
}
fn grayscale_head(mut skin: DynamicImage) -> Option<UniqueBytes> {
    if !crate::skin_manager::SkinManager::is_valid_size(&skin) {
        return None;
    }
    let mut head = skin.crop(8, 8, 8, 8);
    let overlay = skin.crop(40, 8, 8, 8);
    image::imageops::overlay(&mut head, &overlay, 0, 0);
    let mut encoded = Cursor::new(Vec::new());
    head.grayscale().write_to(&mut encoded, ImageFormat::Png).ok()?;
    Some(UniqueBytes::new(&encoded.into_inner()))
}

impl BackendState {
    fn apply_offline_head(&self, uuid: Uuid, username: &str, head: UniqueBytes) {
        let mut info = self.account_info.write();
        if info.get().selected_account != Some(uuid) {
            return;
        }
        let valid = info.get().accounts.get(&uuid).is_some_and(|a| a.offline && &*a.username == username);
        if !valid {
            return;
        }
        info.modify(|info| {
            if let Some(account) = info.accounts.get_mut(&uuid) {
                account.head = Some(head);
            }
        });
        self.send.send(info.get().create_update_message());
    }

    /// Called only when the UI first receives or changes its selected account.
    /// Cached bytes can be shown immediately; network never gates startup.
    pub(crate) fn refresh_selected_offline_head(&self) {
        #[cfg(feature = "ui-capture")]
        if std::env::var_os("WBOMBO_UI_CAPTURE_PAGE").is_some() {
            return;
        }
        let (uuid, username) = {
            let mut info = self.account_info.write();
            let Some(uuid) = info.get().selected_account else {
                return;
            };
            let Some(account) = info.get().accounts.get(&uuid).filter(|a| a.offline) else {
                return;
            };
            (uuid, account.username.clone())
        };
        let directory = self.directories.root_launcher_dir.join("offline-heads");
        let path = directory.join(format!("{uuid}.json"));
        let cached = read_cache(&path, &username);
        if let Some(head) = cached.as_ref().and_then(|c| c.head.clone()) {
            self.apply_offline_head(uuid, &username, head);
        }
        if cached.as_ref().is_some_and(|c| c.fresh(&username, now())) {
            return;
        }
        let attempt = CachedHead {
            username: username.to_string(),
            checked_at: now(),
            success: false,
            head: cached.and_then(|c| c.head),
        };
        if std::fs::create_dir_all(&directory).is_err() || !write_cache(&path, &attempt) {
            log::warn!("Unable to cache an offline account face; keeping its current face");
            return;
        }
        let backend = self.clone();
        tokio::spawn(async move {
            let head = tokio::time::timeout(Duration::from_secs(8), backend.fetch_offline_head(&username))
                .await
                .ok()
                .flatten();
            let success = head.is_some();
            let cache = CachedHead {
                username: username.to_string(),
                checked_at: now(),
                success,
                head: head.clone().or(attempt.head),
            };
            if !write_cache(&path, &cache) {
                log::warn!("Unable to save an offline account face cache");
            }
            if let Some(head) = head {
                backend.apply_offline_head(uuid, &username, head);
            }
        });
    }

    async fn fetch_offline_head(&self, username: &str) -> Option<UniqueBytes> {
        let client = self.http_client_provider.client();
        let mut lookup = url::Url::parse("https://api.mojang.com/minecraft/profile/lookup/name/").ok()?;
        lookup.path_segments_mut().ok()?.pop_if_empty().push(username);
        let lookup: serde_json::Value =
            serde_json::from_slice(&bounded_body(client.get(lookup).send().await.ok()?, 64 * 1024).await?).ok()?;
        let uuid = Uuid::parse_str(lookup["id"].as_str()?).ok()?;
        let profile = bounded_body(
            client
                .get(format!("https://sessionserver.mojang.com/session/minecraft/profile/{}", uuid.simple()))
                .send()
                .await
                .ok()?,
            64 * 1024,
        )
        .await?;
        let texture: Arc<str> = Self::extract_skin_url_from_profile(std::str::from_utf8(&profile).ok()?)?;
        let mut texture = url::Url::parse(&texture).ok()?;
        if texture.host_str() != Some("textures.minecraft.net") || !matches!(texture.scheme(), "http" | "https") {
            return None;
        }
        texture.set_scheme("https").ok()?;
        let bytes = bounded_body(client.get(texture).send().await.ok()?, 1024 * 1024).await?;
        let dimensions = image::ImageReader::new(Cursor::new(&bytes))
            .with_guessed_format()
            .ok()?
            .into_dimensions()
            .ok()?;
        if !matches!(dimensions, (64, 64) | (64, 32)) {
            return None;
        }
        grayscale_head(image::load_from_memory(&bytes).ok()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_reuses_successes_and_backs_off_failures_without_crossing_names() {
        let mut cache = CachedHead {
            username: "Player".into(),
            checked_at: 10_000,
            success: true,
            head: Some(UniqueBytes::new(b"cached")),
        };
        assert!(cache.fresh("Player", 10_001));
        assert!(cache.fresh("Player", 10_000 + FRESH_SECONDS - 1));
        assert!(!cache.fresh("Player", 10_000 + FRESH_SECONDS));
        assert!(!cache.fresh("SomeoneElse", 10_001));
        assert!(cache.fresh("Player", 9_000));
        cache.success = false;
        assert!(cache.fresh("Player", 10_000 + RETRY_SECONDS - 1));
        assert!(!cache.fresh("Player", 10_000 + RETRY_SECONDS));
        let encoded = serde_json::to_vec(&cache).unwrap();
        let decoded: CachedHead = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded.head.unwrap().as_ref(), b"cached");
    }
    #[test]
    fn face_is_monochrome_with_overlay_and_keeps_its_pixel_size() {
        let mut skin = image::RgbaImage::new(64, 64);
        for y in 8..16 {
            for x in 8..16 {
                skin.put_pixel(x, y, image::Rgba([255, 0, 0, 255]));
            }
        }
        skin.put_pixel(40, 8, image::Rgba([255, 255, 255, 255]));
        let bytes = grayscale_head(DynamicImage::ImageRgba8(skin)).unwrap();
        let head = image::load_from_memory(bytes.as_ref()).unwrap().into_rgba8();
        assert_eq!(head.dimensions(), (8, 8));
        assert_eq!(head.get_pixel(0, 0).0, [255; 4]);
        let p = head.get_pixel(1, 0).0;
        assert_eq!(p[0], p[1]);
        assert_eq!(p[1], p[2]);
        assert_eq!(p[3], 255);
        assert!(p[0] < 255);
        assert!(grayscale_head(DynamicImage::new_rgba8(128, 128)).is_none());
    }
}
