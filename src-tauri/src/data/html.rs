//! Plain-text extraction from feed HTML. Never executes scripts or loads resources.

use regex::Regex;
use std::sync::OnceLock;

/// Rust regex has no backreferences, so build one pattern per tag.
fn strip_junk_blocks(html: &str) -> String {
    const TAGS: &[&str] = &[
        "script", "style", "noscript", "svg", "iframe", "nav", "header", "footer", "aside", "form",
    ];
    let mut s = html.to_string();
    for tag in TAGS {
        if let Ok(re) = Regex::new(&format!(r"(?is)<{tag}\b[^>]*>.*?</{tag}>")) {
            s = re.replace_all(&s, " ").to_string();
        }
    }
    s
}

fn block_re(name: &str) -> Regex {
    Regex::new(&format!(r"(?is)<{name}\b[^>]*>(.*?)</{name}>")).unwrap()
}

fn breaks_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?is)<br\s*/?>|</(p|div|li|h[1-6]|blockquote|tr)>").unwrap())
}

fn tags_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?s)<[^>]+>").unwrap())
}

fn spaces_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[ \t\r\f\v]+").unwrap())
}

fn newlines_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\n{3,}").unwrap())
}

pub fn to_text(html: Option<&str>, max_length: usize) -> String {
    let html = match html {
        Some(h) if !h.trim().is_empty() => h,
        _ => return String::new(),
    };

    let mut clean = strip_junk_blocks(html);

    let article = block_re("article");
    let main = block_re("main");
    let matched = article
        .captures(&clean)
        .or_else(|| main.captures(&clean));
    if let Some(caps) = matched {
        let inner = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        if inner.len() > 200 {
            clean = inner.to_string();
        }
    }

    clean = breaks_re().replace_all(&clean, "\n").to_string();
    clean = tags_re().replace_all(&clean, " ").to_string();
    clean = html_escape::decode_html_entities(&clean).to_string();
    clean = spaces_re().replace_all(&clean, " ").to_string();
    clean = newlines_re().replace_all(&clean, "\n\n").to_string();

    let lines: Vec<&str> = clean
        .split('\n')
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let text = lines.join("\n").trim().to_string();
    if text.chars().count() > max_length {
        let cut: String = text.chars().take(max_length).collect();
        format!("{cut}\n\n（正文过长，已截断；请打开原文阅读。）")
    } else {
        text
    }
}

pub fn snippet(html: Option<&str>, length: usize) -> String {
    let flat = to_text(html, 4000).replace('\n', " ");
    if flat.chars().count() <= length {
        flat
    } else {
        let cut: String = flat.chars().take(length).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_script_and_footer_keeps_paragraphs() {
        let html = "<html><head><script>alert(1)</script><style>a{}</style></head><body><article><h1>标题</h1><p>正文第一段。</p><p>正文第二段。</p></article><footer>页脚</footer></body></html>";
        let text = to_text(Some(html), 120000);
        assert!(text.contains("正文第一段"));
        assert!(text.contains("正文第二段"));
        assert!(!text.contains("alert(1)"));
        assert!(!text.contains("页脚"));
    }

    #[test]
    fn decodes_html_entities() {
        let text = to_text(Some("<![CDATA[<p>摘要文本 &amp; 更多</p>]]>".replace("<![CDATA[", "").replace("]]>", "").as_str()), 120000);
        assert!(text.contains("摘要文本 & 更多"));
    }
}
