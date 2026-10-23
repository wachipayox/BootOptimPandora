//! Bounded news image downloads and prepared PNGs for native GPUI rendering.
use super::*;
use schema::news::{ImageCorners, NewsImage, NewsImageRequest, NewsImageSource, NewsLoadedImage};
use std::sync::LazyLock;
type ImageGate = Arc<tokio::sync::Mutex<()>>;
static IMAGE_GATES: LazyLock<parking_lot::Mutex<BTreeMap<PathBuf, ImageGate>>> =
    LazyLock::new(|| parking_lot::Mutex::new(BTreeMap::new()));
const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;

impl DistributionClient {
    async fn news_media(&self, request: &NewsImageRequest, root: &Path) -> Option<(PathBuf, u32, u32)> {
        let result: Result<_, DistributionError> = async {
            fs::create_dir_all(root)?;
            let key = match &request.source {
                NewsImageSource::Reference(NewsImage::External { url }) => {
                    if !schema::news_markdown::https_url(url) {
                        return Err(DistributionError::InvalidBaseUrl);
                    }
                    format!("url-{}", hex::encode(Sha256::digest(url.as_bytes())))
                },
                NewsImageSource::Reference(NewsImage::Cas {
                    sha256,
                    size,
                    media_type,
                }) => {
                    if *size > MAX_IMAGE_BYTES as u64 || !["image/png", "image/jpeg"].contains(&media_type.as_str()) {
                        return Err(DistributionError::ResponseTooLarge);
                    }
                    if !valid_digest(sha256) {
                        return Err(DistributionError::InvalidResponse("Invalid image digest".into()));
                    }
                    format!("cas-{sha256}")
                },
                NewsImageSource::CasDigest(sha256) => {
                    if !valid_digest(sha256) {
                        return Err(DistributionError::InvalidResponse("Invalid image digest".into()));
                    }
                    format!("cas-{sha256}")
                },
            };
            let gate = IMAGE_GATES.lock().entry(root.join(&key)).or_default().clone();
            let _guard = gate.lock().await;
            let width = request.width.clamp(1, 2048);
            let height = request.height.clamp(1, 2048);
            let corner_key = match request.corners {
                ImageCorners::None => "none",
                ImageCorners::Top => "top",
                ImageCorners::All => "all",
            };
            let output = root.join(format!("news-{key}-{width}x{height}-{}-{corner_key}.png", request.cover));
            if let Ok((w, h)) = image::ImageReader::open(&output)
                .and_then(|r| r.with_guessed_format())
                .and_then(|r| r.into_dimensions().map_err(std::io::Error::other))
            {
                if w > 0 && h > 0 && w <= 2048 && h <= 2048 {
                    return Ok((output, w, h));
                }
            }
            let source = match &request.source {
                NewsImageSource::Reference(NewsImage::External { url }) => {
                    let path = root.join(format!("{key}.external"));
                    if !path.exists() {
                        // No cookie store, default credentials or inherited origin headers.
                        let http = Client::builder()
                            .https_only(true)
                            .redirect(reqwest::redirect::Policy::limited(3))
                            .timeout(Duration::from_secs(30))
                            .build()?;
                        let bytes = image_bytes(http.get(url).send().await?).await?;
                        atomic_bytes(&path, &bytes)?;
                    }
                    path
                },
                NewsImageSource::Reference(NewsImage::Cas {
                    sha256,
                    size,
                    media_type,
                }) => {
                    if *size > MAX_IMAGE_BYTES as u64 || !["image/png", "image/jpeg"].contains(&media_type.as_str()) {
                        return Err(DistributionError::ResponseTooLarge);
                    }
                    self.fetch_verified_object(
                        &ManifestObjectRef {
                            sha256: sha256.clone(),
                            size: *size,
                            media_type: media_type.clone(),
                        },
                        root,
                    )
                    .await?
                },
                NewsImageSource::CasDigest(sha) => {
                    let path = root.join("sha256").join(&sha[..2]).join(sha);
                    let valid = fs::metadata(&path).is_ok_and(|m| m.len() <= MAX_IMAGE_BYTES as u64)
                        && fs::read(&path).is_ok_and(|b| hex::encode(Sha256::digest(&b)) == *sha);
                    if !valid {
                        let bytes = image_bytes(self.client.get(self.object_url(sha)?).send().await?).await?;
                        if hex::encode(Sha256::digest(&bytes)) != *sha {
                            return Err(DistributionError::InvalidObject(sha.clone()));
                        }
                        fs::create_dir_all(path.parent().unwrap())?;
                        atomic_bytes(&path, &bytes)?;
                    }
                    path
                },
            };
            if fs::metadata(&source)?.len() > MAX_IMAGE_BYTES as u64 {
                return Err(DistributionError::ResponseTooLarge);
            }
            let dimensions = image::ImageReader::open(&source)?
                .with_guessed_format()?
                .into_dimensions()
                .map_err(|e| DistributionError::InvalidResponse(e.to_string()))?;
            if dimensions.0 == 0 || dimensions.1 == 0 || dimensions.0 > 4096 || dimensions.1 > 4096 {
                return Err(DistributionError::ResponseTooLarge);
            }
            let decoded = image::ImageReader::open(&source)?
                .with_guessed_format()?
                .decode()
                .map_err(|e| DistributionError::InvalidResponse(e.to_string()))?;
            let mut prepared = if request.cover {
                decoded.resize_to_fill(width, height, image::imageops::FilterType::Triangle)
            } else {
                decoded.thumbnail(width, height)
            }
            .to_rgba8();
            mask_corners(&mut prepared, request.corners);
            let (w, h) = prepared.dimensions();
            let temporary = output.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
            prepared
                .save_with_format(&temporary, image::ImageFormat::Png)
                .map_err(|e| DistributionError::InvalidResponse(e.to_string()))?;
            fs::rename(temporary, &output)?;
            Ok((output, w, h))
        }
        .await;
        result.ok()
    }
}
fn atomic_bytes(path: &Path, bytes: &[u8]) -> Result<(), DistributionError> {
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    fs::write(&temporary, bytes)?;
    fs::rename(temporary, path)?;
    Ok(())
}
async fn image_bytes(mut response: reqwest::Response) -> Result<Vec<u8>, DistributionError> {
    if !response.status().is_success() {
        return Err(DistributionError::HttpStatus(response.status()));
    }
    if response.content_length().is_some_and(|n| n > MAX_IMAGE_BYTES as u64) {
        return Err(DistributionError::ResponseTooLarge);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len().saturating_add(chunk.len()) > MAX_IMAGE_BYTES {
            return Err(DistributionError::ResponseTooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
fn mask_corners(image: &mut image::RgbaImage, corners: ImageCorners) {
    if matches!(corners, ImageCorners::None) {
        return;
    }
    let (w, h) = image.dimensions();
    let notch = 12.min(w / 2).min(h / 2);
    for y in 0..notch {
        for x in 0..notch - y {
            image.get_pixel_mut(x, y)[3] = 0;
            image.get_pixel_mut(w - 1 - x, y)[3] = 0;
            if matches!(corners, ImageCorners::All) {
                image.get_pixel_mut(x, h - 1 - y)[3] = 0;
                image.get_pixel_mut(w - 1 - x, h - 1 - y)[3] = 0;
            }
        }
    }
}
impl crate::BackendState {
    pub async fn load_news_images(&self, requests: Vec<NewsImageRequest>) -> Vec<NewsLoadedImage> {
        use futures::{StreamExt, stream};
        let config = self.config.lock().get().distribution.clone();
        let client = DistributionClient::new(&config).ok();
        let root = self.directories.root_launcher_dir.join("home-presentation");
        stream::iter(requests.into_iter().take(16).map(|request| {
            let client = client.clone();
            let root = root.clone();
            async move {
                let result = match client {
                    Some(client) => client.news_media(&request, &root).await,
                    None => None,
                };
                let (path, width, height) = match result {
                    Some((p, w, h)) => (Some(p), w, h),
                    None => (None, 0, 0),
                };
                NewsLoadedImage {
                    key: request.key,
                    path,
                    width,
                    height,
                }
            }
        }))
        .buffer_unordered(4)
        .collect()
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn client() -> DistributionClient {
        DistributionClient {
            client: Client::builder().no_proxy().timeout(Duration::from_secs(1)).build().unwrap(),
            base_url: Url::parse("https://example.com").unwrap(),
            trusted_keys: BTreeMap::new(),
            server_keys: Arc::new(tokio::sync::OnceCell::new()),
        }
    }
    fn request(source: NewsImageSource) -> NewsImageRequest {
        NewsImageRequest {
            key: "fixture".into(),
            source,
            width: 16,
            height: 16,
            cover: true,
            corners: ImageCorners::All,
        }
    }
    #[tokio::test]
    async fn verified_cas_is_prepared_and_reference_budgets_are_enforced_before_cache_hits() {
        let root = std::env::temp_dir().join(format!("pandora-news-media-{}", uuid::Uuid::new_v4()));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(8, 8, image::Rgba([30, 60, 90, 255])))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        let bytes = bytes.into_inner();
        let sha = hex::encode(Sha256::digest(&bytes));
        let directory = root.join("sha256").join(&sha[..2]);
        fs::create_dir_all(&directory).unwrap();
        let source = directory.join(&sha);
        fs::write(&source, &bytes).unwrap();
        let client = client();
        let prepared = client.news_media(&request(NewsImageSource::CasDigest(sha.clone())), &root).await.unwrap();
        assert_eq!((prepared.1, prepared.2), (16, 16));
        let image = image::open(&prepared.0).unwrap().to_rgba8();
        assert_eq!(image.get_pixel(0, 0)[3], 0);
        assert_eq!(image.get_pixel(15, 15)[3], 0);
        assert_eq!(image.get_pixel(8, 8)[3], 255);
        let oversized = request(NewsImageSource::Reference(NewsImage::Cas {
            sha256: sha,
            size: MAX_IMAGE_BYTES as u64 + 1,
            media_type: "image/png".into(),
        }));
        assert!(client.news_media(&oversized, &root).await.is_none());
        fs::remove_file(prepared.0).unwrap();
        fs::remove_file(source).unwrap();
        fs::remove_dir(directory).unwrap();
        fs::remove_dir(root.join("sha256")).unwrap();
        fs::remove_dir(root).unwrap();
    }
    #[tokio::test]
    async fn unsafe_urls_and_oversized_dimensions_never_reach_gpui() {
        let root = std::env::temp_dir().join(format!("pandora-news-media-{}", uuid::Uuid::new_v4()));
        let client = client();
        assert!(
            client
                .news_media(
                    &request(NewsImageSource::Reference(NewsImage::External {
                        url: "http://example.com/x".into()
                    })),
                    &root
                )
                .await
                .is_none()
        );
        assert!(
            client
                .news_media(&request(NewsImageSource::CasDigest("../../outside".into())), &root)
                .await
                .is_none()
        );
        let url = "https://example.com/oversized.png";
        let cached = root.join(format!("url-{}.external", hex::encode(Sha256::digest(url.as_bytes()))));
        image::RgbaImage::from_pixel(4097, 1, image::Rgba([0, 0, 0, 255]))
            .save_with_format(&cached, image::ImageFormat::Png)
            .unwrap();
        assert!(
            client
                .news_media(&request(NewsImageSource::Reference(NewsImage::External { url: url.into() })), &root)
                .await
                .is_none()
        );
        fs::remove_file(cached).unwrap();
        fs::remove_dir(root).unwrap();
    }
}
