//! Download images found in an extracted article body and inline them as
//! data URLs. Many sites reject hotlinked images without a Referer, and the
//! reading pane renders via `srcDoc`, so remote URLs would otherwise break
//! (or vanish once the app is offline).

use base64::Engine;
use regex::Regex;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::task::JoinSet;
use url::Url;

const MAX_IMAGES: usize = 40;
const MAX_PER_IMAGE: usize = 8 * 1024 * 1024;
const MAX_TOTAL: usize = 32 * 1024 * 1024;

fn img_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?is)<img\b[^>]*>").unwrap())
}

fn src_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?i)\bsrc\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+)"#).unwrap()
    })
}

fn srcset_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?i)\ssrcset\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+)"#).unwrap()
    })
}

fn unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let pattern = format!(
        r#"(?i)\b{name}\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))"#
    );
    let re = Regex::new(&pattern).ok()?;
    let caps = re.captures(tag)?;
    let value = caps
        .get(1)
        .or_else(|| caps.get(2))
        .or_else(|| caps.get(3))?
        .as_str();
    Some(unescape(value).trim().to_string())
}

fn is_placeholder(value: &str) -> bool {
    let v = value.to_ascii_lowercase();
    v.starts_with("data:image/") && value.len() < 512
}

/// Pick the best image URL from an <img> tag, including common lazy-loading
/// attributes. Returns the resolved absolute URL.
fn image_url(tag: &str, page_url: &str) -> Option<String> {
    let src = attr(tag, "src").filter(|s| !s.is_empty());
    let lazy = ["data-src", "data-original", "data-lazy-src", "data-echo"]
        .iter()
        .find_map(|name| attr(tag, name).filter(|s| !s.is_empty()));

    let prefer_lazy = lazy.is_some() && src.as_deref().is_none_or(is_placeholder);
    let mut candidate = if prefer_lazy {
        lazy.unwrap()
    } else if let Some(s) = src {
        s
    } else {
        // srcset: "small.jpg 320w, large.jpg 800w" -> first URL
        let srcset = attr(tag, "srcset")?;
        srcset
            .split(',')
            .next()?
            .split_whitespace()
            .next()?
            .to_string()
    };
    if candidate.starts_with("//") {
        candidate = format!("https:{candidate}");
    }
    let base = Url::parse(page_url).ok()?;
    let resolved = base.join(&candidate).ok()?;
    if !matches!(resolved.scheme(), "http" | "https") {
        return None;
    }
    Some(resolved.to_string())
}

fn sniff_mime(bytes: &[u8], url: &str, header: Option<&str>) -> Option<String> {
    if let Some(raw) = header {
        let mime = raw.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
        if mime.starts_with("image/")
            && mime
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '/' | '+' | '-' | '.'))
        {
            return Some(mime);
        }
    }
    let bytes_mime = if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else if bytes.starts_with(b"BM") {
        Some("image/bmp")
    } else {
        None
    };
    if let Some(mime) = bytes_mime {
        return Some(mime.to_string());
    }
    let lower = url.to_ascii_lowercase();
    let ext = lower.rsplit(['?', '#']).next().unwrap_or("");
    let guessed = if ext.ends_with(".png") {
        "image/png"
    } else if ext.ends_with(".jpg") || ext.ends_with(".jpeg") {
        "image/jpeg"
    } else if ext.ends_with(".gif") {
        "image/gif"
    } else if ext.ends_with(".webp") {
        "image/webp"
    } else {
        return None;
    };
    Some(guessed.to_string())
}

async fn download_data_url(
    client: &reqwest::Client,
    url: &str,
    referer: &str,
    budget: &AtomicUsize,
) -> Option<String> {
    if budget.load(Ordering::Relaxed) >= MAX_TOTAL {
        return None;
    }
    let response = client
        .get(url)
        .header(reqwest::header::REFERER, referer)
        .header(
            reqwest::header::ACCEPT,
            "image/avif,image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8",
        )
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?;
    let mime_header = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let bytes = response.bytes().await.ok()?;
    if bytes.is_empty() || bytes.len() > MAX_PER_IMAGE {
        return None;
    }
    if budget.fetch_add(bytes.len(), Ordering::Relaxed) + bytes.len() > MAX_TOTAL {
        return None;
    }
    let mime = sniff_mime(&bytes, url, mime_header.as_deref())?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Some(format!("data:{mime};base64,{encoded}"))
}

fn rewrite_tag(tag: &str, data_url: &str) -> String {
    let replacement = format!("src=\"{data_url}\"");
    let mut out = if src_re().is_match(tag) {
        src_re().replace(tag, replacement.as_str()).into_owned()
    } else {
        let head = tag.trim_end_matches('>').trim_end();
        format!("{head} {replacement}>")
    };
    // srcset would make the browser fetch the remote copy again.
    out = srcset_re().replace(&out, "").into_owned();
    out
}

/// Inline every supported image in `html` as a data URL. Images that fail to
/// download (or exceed the size budget) keep their original URL.
pub async fn embed_images(client: &reqwest::Client, page_url: &str, html: &str) -> String {
    let re = img_re();
    let tags: Vec<(usize, String)> = re
        .find_iter(html)
        .map(|m| (m.start(), m.as_str().to_string()))
        .collect();
    if tags.is_empty() {
        return html.to_string();
    }

    let budget = Arc::new(AtomicUsize::new(0));
    let mut jobs = JoinSet::new();
    for (idx, tag) in tags.iter().take(MAX_IMAGES).enumerate() {
        let Some(url) = image_url(&tag.1, page_url) else {
            continue;
        };
        let client = client.clone();
        let referer = page_url.to_string();
        let budget = budget.clone();
        jobs.spawn(async move {
            let data = download_data_url(&client, &url, &referer, &budget).await;
            (idx, data)
        });
    }
    let mut replacements: HashMap<usize, String> = HashMap::new();
    while let Some(Ok((idx, Some(data_url)))) = jobs.join_next().await {
        if let Some(tag) = tags.get(idx) {
            replacements.insert(idx, rewrite_tag(&tag.1, &data_url));
        }
    }
    let mut out = html.to_string();
    for (idx, (start, tag)) in tags.iter().enumerate().rev() {
        if let Some(new_tag) = replacements.get(&idx) {
            out.replace_range(*start..*start + tag.len(), new_tag);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_lazy_src_and_resolves_relative_url() {
        let tag = r#"<img class="lazy" src="data:image/gif;base64,R0lGODlhAQABAAAAACw=" data-src="/uploads/pic.jpg" alt="x">"#;
        let url = image_url(tag, "https://example.com/thread/123.html").unwrap();
        assert_eq!(url, "https://example.com/uploads/pic.jpg");
    }

    #[test]
    fn skips_non_http_sources() {
        assert_eq!(image_url(r#"<img src="javascript:alert(1)">"#, "https://example.com/a"), None);
        assert_eq!(image_url(r#"<img src="data:image/png;base64,AAAA">"#, "https://example.com/a"), None);
    }

    #[test]
    fn rewrites_src_and_drops_srcset() {
        let tag = r#"<img src="old.jpg" srcset="old.jpg 1x, big.jpg 2x" alt="a">"#;
        let out = rewrite_tag(tag, "data:image/png;base64,QUJD");
        assert!(out.contains("src=\"data:image/png;base64,QUJD\""));
        assert!(!out.contains("srcset"));
        assert!(out.contains("alt=\"a\""));
    }

    #[test]
    fn sniffs_magic_bytes() {
        let png = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0];
        assert_eq!(sniff_mime(&png, "no-ext", None).as_deref(), Some("image/png"));
    }

    #[tokio::test]
    async fn downloads_with_referer_and_inlines_png() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let page = format!("http://127.0.0.1:{port}/post/1");
        let expected_referer = page.clone();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 8192];
            let n = socket.read(&mut buf).await.unwrap();
            let request = String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase();
            assert!(
                request.contains(&format!("referer: {expected_referer}")),
                "missing referer: {request}"
            );
            let png = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3, 4];
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                png.len()
            );
            socket.write_all(head.as_bytes()).await.unwrap();
            socket.write_all(&png).await.unwrap();
        });
        let html = format!("<p>正文内容</p><img src=\"http://127.0.0.1:{port}/pic.png\" alt=\"p\">");
        let out = embed_images(&reqwest::Client::new(), &page, &html).await;
        assert!(out.contains("data:image/png;base64,"), "got: {out}");
        assert!(out.contains("alt=\"p\""));
        server.await.unwrap();
    }
}
