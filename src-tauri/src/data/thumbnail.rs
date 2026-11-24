//! Best-effort extraction of one preview image from feed data.
//! Only http(s) URLs are returned; data URIs, tracking pixels and avatars ignored.

use crate::data::xml::XmlNode;
use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;
use url::Url;

const JUNK: &[&str] = &[
    "avatar", "gravatar", "emoji", "smiley", "spacer", "pixel", "tracking", "beacon", "1x1",
];

fn img_tag_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)<img\b[^>]*>").unwrap())
}

fn src_attr_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?i)\b(?:src|data-src|data-original)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))"#).unwrap()
    })
}

fn dimension_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?i)\b(?:width|height)\s*=\s*(?:"(\d+)"|'(\d+)'|(\d+))"#).unwrap()
    })
}

/// First usable image in an HTML fragment.
pub fn from_html(html_fragment: Option<&str>, base_url: Option<&str>) -> Option<String> {
    let html = html_fragment?;
    if html.trim().is_empty() {
        return None;
    }
    for tag in img_tag_re().find_iter(html) {
        let caps = src_attr_re().captures(tag.as_str())?;
        let raw = caps
            .get(1)
            .or_else(|| caps.get(2))
            .or_else(|| caps.get(3))?
            .as_str();

        let mut tiny = false;
        for dim in dimension_re().captures_iter(tag.as_str()) {
            let num = dim.get(1).or_else(|| dim.get(2)).or_else(|| dim.get(3));
            if let Some(size) = num.and_then(|m| m.as_str().parse::<i32>().ok()) {
                if size > 0 && size <= 16 {
                    tiny = true;
                }
            }
        }
        if tiny {
            continue;
        }
        if let Some(normalized) = normalize(Some(raw), base_url) {
            return Some(normalized);
        }
    }
    None
}

/// Extract a thumbnail from an RSS 2.0 item or Atom entry.
pub fn from_feed_item(item: &XmlNode, base_url: Option<&str>) -> Option<String> {
    // media:thumbnail / media:content
    for child in &item.children {
        let local = child.local_name.as_str();
        if !local.eq_ignore_ascii_case("thumbnail") && !local.eq_ignore_ascii_case("content") {
            continue;
        }
        let medium = child.attr("medium").unwrap_or("");
        let mime = child.attr("type").unwrap_or("");
        let is_image = local.eq_ignore_ascii_case("thumbnail")
            || medium.eq_ignore_ascii_case("image")
            || mime.to_ascii_lowercase().starts_with("image/");
        if !is_image {
            continue;
        }
        if let Some(v) = normalize(child.attr("url"), base_url) {
            return Some(v);
        }
    }

    // itunes:image (href attribute)
    for child in &item.children {
        if child.local_name.eq_ignore_ascii_case("image") {
            if let Some(v) = normalize(child.attr("href"), base_url) {
                return Some(v);
            }
        }
    }

    // enclosure with image MIME type
    for child in &item.children {
        if child.local_name.eq_ignore_ascii_case("enclosure") {
            if let Some(mime) = child.attr("type") {
                if !mime.to_ascii_lowercase().starts_with("image/") {
                    continue;
                }
            }
            if let Some(v) = normalize(child.attr("url"), base_url) {
                return Some(v);
            }
        }
    }

    // Atom link rel="image"
    for child in &item.children {
        if child.local_name.eq_ignore_ascii_case("link")
            && child.attr("rel").map(|r| r.eq_ignore_ascii_case("image")).unwrap_or(false)
        {
            if let Some(v) = normalize(child.attr("href"), base_url) {
                return Some(v);
            }
        }
    }

    // First image in description / content:encoded / summary / content
    for child in &item.children {
        let local = child.local_name.as_str();
        if !["description", "encoded", "summary", "content"]
            .iter()
            .any(|n| local.eq_ignore_ascii_case(n))
        {
            continue;
        }
        if let Some(v) = from_html(Some(&child.content()), base_url) {
            return Some(v);
        }
    }
    None
}

/// Extract a thumbnail from a Google Reader compatible JSON item.
pub fn from_greader_item(item: &Value, base_url: Option<&str>) -> Option<String> {
    let direct = json_str(item, "thumbnail").or_else(|| json_str(item, "image"));
    if let Some(v) = normalize(direct, base_url) {
        return Some(v);
    }
    if let Some(enclosures) = item.get("enclosure").and_then(Value::as_array) {
        for enc in enclosures {
            if let Some(mime) = json_str(enc, "type") {
                if !mime.to_ascii_lowercase().starts_with("image/") {
                    continue;
                }
            }
            let url = json_str(enc, "href").or_else(|| json_str(enc, "url"));
            if let Some(v) = normalize(url, base_url) {
                return Some(v);
            }
        }
    }
    let html = nested_string(item, "summary").or_else(|| nested_string(item, "content"));
    from_html(html.as_deref(), base_url)
}

fn json_str<'a>(value: &'a Value, name: &str) -> Option<&'a str> {
    value.get(name).and_then(Value::as_str)
}

fn nested_string(value: &Value, name: &str) -> Option<String> {
    let nested = value.get(name)?;
    match nested {
        Value::String(s) => Some(s.clone()),
        _ => json_str(nested, "content").map(str::to_string),
    }
}

fn normalize(raw: Option<&str>, base_url: Option<&str>) -> Option<String> {
    let raw = raw?;
    if raw.trim().is_empty() {
        return None;
    }
    let mut value = html_escape::decode_html_entities(raw.trim()).to_string();
    if value.chars().count() < 8 || value.chars().count() > 2048 {
        return None;
    }
    if value.to_ascii_lowercase().starts_with("data:") {
        return None;
    }
    if value.starts_with("//") {
        value = format!("https:{value}");
    }
    let lowered = value.to_ascii_lowercase();
    if JUNK.iter().any(|m| lowered.contains(m)) {
        return None;
    }

    let parsed = Url::parse(&value)
        .ok()
        .or_else(|| base_url.and_then(|b| Url::parse(b).ok()).and_then(|root| root.join(&value).ok()))?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return None;
    }
    Some(parsed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn skips_tracking_pixel_and_resolves_relative() {
        let got = from_html(
            Some("<img width='1' height='1' src='track.gif'><img src='/img/cover.jpg'>"),
            Some("https://news.example.com/a"),
        );
        assert_eq!(got.as_deref(), Some("https://news.example.com/img/cover.jpg"));
    }

    #[test]
    fn rejects_data_uri_and_avatar() {
        assert!(from_html(Some("<img src='data:image/png;base64,AAAAAAAA'>"), None).is_none());
        assert!(from_html(Some("<img class='avatar' src='https://example.com/avatar_big.jpg'>"), None).is_none());
    }

    #[test]
    fn extracts_from_greader_json() {
        let item = json!({"title":"GReader 条目","summary":{"content":"<img src='https://cdn.example.com/g.jpg'>"}});
        assert_eq!(
            from_greader_item(&item, None).as_deref(),
            Some("https://cdn.example.com/g.jpg")
        );
    }
}
