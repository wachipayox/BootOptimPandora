use gpui::{ImageSource, SharedUri};

const NUMBERED_PREFIXES_1: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const NUMBERED_PREFIXES_2: &str = "abcdefghijklmnopqrstuvwxyz";

const BULLETS: [&str; 5] = ["•", "◦", "▪", "‣", "⁃"];

/// Returns the prefix for a list item.
pub(super) fn list_item_prefix(ix: usize, ordered: bool, depth: usize) -> String {
    if ordered {
        if depth == 0 {
            return format!("{}. ", ix + 1);
        }

        if depth == 1 {
            return format!("{}. ", NUMBERED_PREFIXES_1.chars().nth(ix % NUMBERED_PREFIXES_1.len()).unwrap());
        } else {
            return format!("{}. ", NUMBERED_PREFIXES_2.chars().nth(ix % NUMBERED_PREFIXES_2.len()).unwrap());
        }
    } else {
        let depth = depth.min(BULLETS.len() - 1);
        let bullet = BULLETS[depth];
        return format!("{} ", bullet);
    }
}

/// Converts a document image URL into an [`ImageSource`] without granting
/// implicit filesystem access.
///
/// Document-provided values remain URI-backed, including `file://` and
/// scheme-less strings.
pub(super) fn image_source(url: &SharedUri) -> ImageSource {
    url.clone().into()
}

/// Resolve only images explicitly authorized by the application, when a map is supplied.
pub(super) fn resolved_image_source(url: &SharedUri, style: &super::TextViewStyle) -> Option<ImageSource> {
    match &style.image_sources {
        Some(sources) => sources
            .get(url.as_ref())
            .map(|path| ImageSource::Resource(gpui::Resource::Path(std::sync::Arc::from(path.as_path())))),
        None => Some(image_source(url)),
    }
}

#[cfg(test)]
mod tests {
    use gpui::{ImageSource, Resource};

    use crate::text::utils::{image_source, list_item_prefix};

    #[test]
    fn explicit_images_deny_unmapped_urls_and_only_authorize_supplied_paths() {
        let uri = gpui::SharedUri::from("https://example.com/image.png".to_string());
        let mut style = super::super::TextViewStyle::default();
        style.image_sources = Some(std::sync::Arc::new(std::collections::BTreeMap::new()));
        assert!(super::resolved_image_source(&uri, &style).is_none());
        let path = std::path::PathBuf::from("verified/image.png");
        style.image_sources =
            Some(std::sync::Arc::new(std::collections::BTreeMap::from([(uri.to_string(), path.clone())])));
        match super::resolved_image_source(&uri, &style).unwrap() {
            ImageSource::Resource(Resource::Path(actual)) => assert_eq!(actual.as_ref(), path.as_path()),
            _ => panic!("expected explicitly supplied path"),
        }
        assert!(super::resolved_image_source(&"file:///other.png".to_string().into(), &style).is_none());
    }

    #[test]
    fn test_image_source() {
        fn source(url: &str) -> Resource {
            match image_source(&url.to_string().into()) {
                ImageSource::Resource(resource) => resource,
                _ => panic!("expected a resource for {url:?}"),
            }
        }
        fn assert_uri(url: &str) {
            match source(url) {
                Resource::Uri(uri) => assert_eq!(uri.as_ref(), url),
                other => panic!("expected Uri for {url:?}, got {other:?}"),
            }
        }
        assert_uri("https://example.com/logo.png");
        assert_uri("http://example.com/logo.png");
        assert_uri("data:image/png;base64,iVBORw0KGgo=");

        assert_uri("website/public/logo.svg");
        assert_uri("./images/a.png");
        assert_uri("../images/a.png");
        assert_uri("/absolute/path/logo.svg");
        assert_uri("file:///absolute/path/logo.svg");
        assert_uri(r"C:\images\logo.png");
        assert_uri("docs/a:b.png");
    }

    #[test]
    fn test_list_item_prefix() {
        assert_eq!(list_item_prefix(0, true, 0), "1. ");
        assert_eq!(list_item_prefix(1, true, 0), "2. ");
        assert_eq!(list_item_prefix(2, true, 0), "3. ");
        assert_eq!(list_item_prefix(10, true, 0), "11. ");
        assert_eq!(list_item_prefix(0, true, 1), "A. ");
        assert_eq!(list_item_prefix(1, true, 1), "B. ");
        assert_eq!(list_item_prefix(2, true, 1), "C. ");
        assert_eq!(list_item_prefix(0, true, 2), "a. ");
        assert_eq!(list_item_prefix(1, true, 2), "b. ");
        assert_eq!(list_item_prefix(6, true, 2), "g. ");
        assert_eq!(list_item_prefix(0, true, 1), "A. ");
        assert_eq!(list_item_prefix(0, true, 2), "a. ");
        assert_eq!(list_item_prefix(0, false, 0), "• ");
        assert_eq!(list_item_prefix(0, false, 1), "◦ ");
        assert_eq!(list_item_prefix(0, false, 2), "▪ ");
        assert_eq!(list_item_prefix(0, false, 3), "‣ ");
        assert_eq!(list_item_prefix(0, false, 4), "⁃ ");
    }
}
