//! Distribution's restricted news Markdown. The result contains no HTML or code.
use markdown::{Constructs, ParseOptions, mdast::Node};
use once_cell::sync::Lazy;
use regex::Regex;

#[derive(Clone, Debug, PartialEq)]
pub enum Inline {
    Text(String),
    Emphasis(Vec<Inline>),
    Strong(Vec<Inline>),
    Link {
        content: Vec<Inline>,
        url: String,
    },
    Image {
        alt: String,
        src: String,
    },
}
#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Paragraph(Vec<Inline>),
    Heading {
        level: u8,
        content: Vec<Inline>,
    },
    List {
        ordered: bool,
        start: u32,
        items: Vec<Vec<Block>>,
    },
    Quote(Vec<Block>),
    Rule,
    Image {
        alt: String,
        src: String,
        size: String,
        align: String,
    },
    Youtube {
        id: String,
    },
}
#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    pub blocks: Vec<Block>,
    pub images: usize,
    pub videos: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyError {
    TooLarge,
    ControlCharacter,
    Syntax,
    Unsupported,
    Heading,
    ListDepth,
    Url,
    MissingImage,
    TooManyImages,
    TooManyVideos,
}
static YOUTUBE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^https://(?:(?:www\.|m\.)?youtube\.com/(?:watch\?(?:[^#\s]*&)?v=([A-Za-z0-9_-]{11})(?:&[^#\s]*)?|shorts/([A-Za-z0-9_-]{11})(?:\?[^#\s]*)?)|youtu\.be/([A-Za-z0-9_-]{11})(?:\?[^#\s]*)?)$").unwrap()
});

fn image_hints(title: Option<&str>) -> (String, String) {
    let (mut size, mut align) = (String::new(), String::new());
    for token in title.unwrap_or_default().split_whitespace() {
        if let Some(value) = token.strip_prefix("size=") {
            size = if matches!(value, "small" | "medium" | "large" | "full") {
                value.into()
            } else {
                String::new()
            };
        } else if let Some(value) = token.strip_prefix("align=") {
            align = if matches!(value, "left" | "center") {
                value.into()
            } else {
                String::new()
            };
        }
    }
    (size, align)
}

pub fn https_url(raw: &str) -> bool {
    if raw.len() > 2048 || !raw.starts_with("https://") || raw.chars().any(|c| c <= ' ' || c == '\u{7f}') {
        return false;
    }
    let authority = raw[8..].split(['/', '?', '#']).next().unwrap_or("");
    !authority.contains('@') && url::Url::parse(raw).is_ok_and(|u| u.scheme() == "https" && u.host_str().is_some())
}
fn youtube_id(raw: &str) -> Option<String> {
    YOUTUBE.captures(raw)?.iter().skip(1).flatten().next().map(|m| m.as_str().to_owned())
}
pub fn plain(content: &[Inline]) -> String {
    content
        .iter()
        .map(|n| match n {
            Inline::Text(s) => s.clone(),
            Inline::Emphasis(c) | Inline::Strong(c) | Inline::Link { content: c, .. } => plain(c),
            Inline::Image { alt, .. } => alt.clone(),
        })
        .collect()
}

/// `cas_exists` is supplied by the shared vectors. For an accepted server news
/// item, clients may treat CAS references as present and verify bytes on download.
pub fn parse(body: &str, cas_exists: Option<&dyn Fn(&str) -> bool>) -> Result<Document, BodyError> {
    if body.len() > 65536 {
        return Err(BodyError::TooLarge);
    }
    if body.chars().any(|c| (c < ' ' && !matches!(c, '\n' | '\r' | '\t')) || c == '\u{7f}') {
        return Err(BodyError::ControlCharacter);
    }
    let options = ParseOptions {
        constructs: Constructs {
            gfm_table: true,
            ..Constructs::default()
        },
        ..ParseOptions::default()
    };
    let tree = markdown::to_mdast(body, &options).map_err(|_| BodyError::Syntax)?;
    let mut parser = Parser {
        cas_exists,
        images: 0,
        videos: 0,
    };
    let blocks = parser.blocks(tree.children().ok_or(BodyError::Syntax)?, 0)?;
    if parser.images > 50 {
        return Err(BodyError::TooManyImages);
    }
    if parser.videos > 10 {
        return Err(BodyError::TooManyVideos);
    }
    Ok(Document {
        blocks,
        images: parser.images,
        videos: parser.videos,
    })
}
struct Parser<'a> {
    cas_exists: Option<&'a dyn Fn(&str) -> bool>,
    images: usize,
    videos: usize,
}
impl Parser<'_> {
    fn image(&mut self, src: &str) -> Result<(), BodyError> {
        self.images += 1;
        if let Some(sha) = src.strip_prefix("cas:") {
            if sha.len() != 64 || !sha.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
                return Err(BodyError::Url);
            }
            if self.cas_exists.is_some_and(|exists| !exists(sha)) {
                return Err(BodyError::MissingImage);
            }
            return Ok(());
        }
        if https_url(src) { Ok(()) } else { Err(BodyError::Url) }
    }
    fn inlines(&mut self, nodes: &[Node]) -> Result<Vec<Inline>, BodyError> {
        nodes
            .iter()
            .map(|n| {
                Ok(match n {
                    Node::Text(t) => Inline::Text(t.value.clone()),
                    Node::Break(_) => Inline::Text("\n".into()),
                    Node::Emphasis(e) => Inline::Emphasis(self.inlines(&e.children)?),
                    Node::Strong(e) => Inline::Strong(self.inlines(&e.children)?),
                    Node::Link(l) => {
                        if !https_url(&l.url) {
                            return Err(BodyError::Url);
                        }
                        Inline::Link {
                            content: self.inlines(&l.children)?,
                            url: l.url.clone(),
                        }
                    },
                    Node::Image(i) => {
                        self.image(&i.url)?;
                        Inline::Image {
                            alt: i.alt.clone(),
                            src: i.url.clone(),
                        }
                    },
                    _ => return Err(BodyError::Unsupported),
                })
            })
            .collect()
    }
    fn blocks(&mut self, nodes: &[Node], depth: usize) -> Result<Vec<Block>, BodyError> {
        nodes
            .iter()
            .map(|n| {
                Ok(match n {
                    Node::Paragraph(p) => {
                        let candidate = if let [Node::Link(l)] = p.children.as_slice() {
                            l.url.clone()
                        } else {
                            n.to_string().trim().to_owned()
                        };
                        if let Some(id) = youtube_id(&candidate) {
                            if candidate.len() > 2048 {
                                return Err(BodyError::Url);
                            }
                            self.videos += 1;
                            Block::Youtube { id }
                        } else if let [Node::Image(i)] = p.children.as_slice() {
                            self.image(&i.url)?;
                            let (size, align) = image_hints(i.title.as_deref());
                            Block::Image {
                                size,
                                align,
                                alt: i.alt.clone(),
                                src: i.url.clone(),
                            }
                        } else {
                            Block::Paragraph(self.inlines(&p.children)?)
                        }
                    },
                    Node::Heading(h) => {
                        if ![2, 3].contains(&h.depth) {
                            return Err(BodyError::Heading);
                        }
                        Block::Heading {
                            level: h.depth,
                            content: self.inlines(&h.children)?,
                        }
                    },
                    Node::List(l) => {
                        if depth >= 2 {
                            return Err(BodyError::ListDepth);
                        }
                        let items = l
                            .children
                            .iter()
                            .map(|item| {
                                if let Node::ListItem(i) = item {
                                    self.blocks(&i.children, depth + 1)
                                } else {
                                    Err(BodyError::Unsupported)
                                }
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        Block::List {
                            ordered: l.ordered,
                            start: l.start.unwrap_or(1),
                            items,
                        }
                    },
                    Node::Blockquote(q) => Block::Quote(self.blocks(&q.children, depth)?),
                    Node::ThematicBreak(_) => Block::Rule,
                    _ => return Err(BodyError::Unsupported),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    fn normalized(blocks: &[Block]) -> Vec<Value> {
        blocks.iter().map(|b| match b {
            Block::Paragraph(c) => json!({"type":"paragraph","text":plain(c)}),
            Block::Heading { level, content } => json!({"type":"heading","level":level,"text":plain(content)}),
            Block::List { ordered, items, .. } => {
                let mut value = json!({"type":"list","items":items.iter().map(|b| json!({"blocks":normalized(b)})).collect::<Vec<_>>()});
                if *ordered { value["ordered"] = json!(true); }
                value
            },
            Block::Quote(b) => json!({"type":"quote","blocks":normalized(b)}),
            Block::Rule => json!({"type":"rule"}),
            Block::Image { alt, src, size, align } => {
                let mut value = json!({"type":"image","alt":alt,"src":src});
                if !size.is_empty() { value["size"] = json!(size); }
                if !align.is_empty() { value["align"] = json!(align); }
                value
            },
            Block::Youtube { id } => json!({"type":"youtube","id":id}),
        }).collect()
    }
    #[test]
    fn distribution_shared_vectors() {
        let fixture: Value = serde_json::from_str(include_str!("../tests/news-markdown-vectors.json")).unwrap();
        let exists = |sha: &str| fixture["cas_existing"].as_array().unwrap().iter().any(|v| v.as_str() == Some(sha));
        for case in fixture["cases"].as_array().unwrap() {
            let result = parse(case["markdown"].as_str().unwrap(), Some(&exists));
            assert_eq!(result.is_ok(), case["accepted"].as_bool().unwrap(), "{}: {result:?}", case["name"]);
            if let Ok(doc) = result {
                assert_eq!(doc.images, case["images"].as_u64().unwrap() as usize, "{}", case["name"]);
                assert_eq!(doc.videos, case["videos"].as_u64().unwrap() as usize, "{}", case["name"]);
                assert_eq!(
                    json!(normalized(&doc.blocks)),
                    case.get("blocks").cloned().unwrap_or(json!([])),
                    "{}",
                    case["name"]
                );
            }
        }
    }
    #[test]
    fn body_and_media_limits() {
        assert_eq!(parse(&"a".repeat(65537), None), Err(BodyError::TooLarge));
        assert_eq!(
            parse(&"![x](https://example.com/x.png)\n\n".repeat(51), None),
            Err(BodyError::TooManyImages)
        );
        assert_eq!(parse(&"https://youtu.be/dQw4w9WgXcQ\n\n".repeat(11), None), Err(BodyError::TooManyVideos));
        assert!(!https_url("https://@example.com"));
        assert!(!https_url(&format!("https://example.com/{}", "x".repeat(2048))));
        assert_eq!(
            parse(&format!("https://youtu.be/dQw4w9WgXcQ?q={}", "x".repeat(2048)), None),
            Err(BodyError::Url)
        );
        assert!(parse(&"![x](https://example.com/x.png)\n\n".repeat(50), None).is_ok());
        assert!(parse(&"https://youtu.be/dQw4w9WgXcQ\n\n".repeat(10), None).is_ok());
    }
}
