//! Self-contained styled HTML for the reading pane, ported from ArticleHtmlRenderer.cs.
//! Scripts and event handlers are stripped; only presentation CSS is injected.

use regex::Regex;
use std::sync::OnceLock;

/// Rust regex has no backreferences, so build one pattern per tag.
fn strip_junk_blocks(html: &str) -> String {
    const TAGS: &[&str] = &[
        "script", "style", "noscript", "svg", "iframe", "nav", "header", "footer", "aside",
        "form", "button", "object", "embed",
    ];
    let mut s = html.to_string();
    for tag in TAGS {
        if let Ok(re) = Regex::new(&format!(r"(?is)<{tag}\b[^>]*>.*?</{tag}>")) {
            s = re.replace_all(&s, " ").to_string();
        }
    }
    s
}

fn event_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?is)\son[a-z]+\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+)"#).unwrap()
    })
}

fn javascript_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?is)\b(href|src|action)\s*=\s*(?:"\s*javascript:[^"]*"|'\s*javascript:[^']*')"#).unwrap()
    })
}

fn article_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?is)<article\b[^>]*>(.*?)</article>").unwrap())
}

fn main_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?is)<main\b[^>]*>(.*?)</main>").unwrap())
}

fn anchor_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)<a\s").unwrap())
}

fn font_unsafe_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[;{}<>\r\n]").unwrap())
}

pub fn clean(html: Option<&str>) -> String {
    let html = match html {
        Some(h) if !h.trim().is_empty() => h,
        _ => return String::new(),
    };
    let mut s = strip_junk_blocks(html);
    s = event_re().replace_all(&s, "").to_string();
    s = javascript_re().replace_all(&s, "").to_string();

    let matched = article_re().captures(&s).or_else(|| main_re().captures(&s));
    if let Some(caps) = matched {
        let inner = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        if inner.len() > 200 {
            s = inner.to_string();
        }
    }
    s.trim().to_string()
}

pub fn sanitize_font_family(family: Option<&str>) -> String {
    let family = match family {
        Some(f) if !f.trim().is_empty() => f,
        _ => return "Microsoft YaHei".to_string(),
    };
    let s = font_unsafe_re()
        .replace_all(family, "")
        .replace('"', "")
        .replace('\'', "")
        .trim()
        .to_string();
    if s.is_empty() || s.chars().count() > 120 {
        "Microsoft YaHei".to_string()
    } else {
        s
    }
}

fn html_encode(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Typography controls for the reading pane. Values are clamped in `render`.
#[derive(Debug, Clone, Copy)]
pub struct ReadingStyle {
    pub margin_px: i32,
    pub line_height: f64,
    pub indent_chars: i32,
}

impl Default for ReadingStyle {
    fn default() -> Self {
        Self { margin_px: 34, line_height: 1.85, indent_chars: 0 }
    }
}

pub fn render(html: Option<&str>, base_url: Option<&str>, dark: bool, font_size: i32, font_family: Option<&str>, style: ReadingStyle) -> String {
    let mut body = clean(html);
    if body.trim().is_empty() {
        body = "<p style=\"color:#888\">（该文章没有可显示的正文，请打开原文阅读。）</p>".to_string();
    }
    body = anchor_re()
        .replace_all(&body, "<a target=\"_blank\" rel=\"noopener noreferrer\" ")
        .to_string();

    let base_tag = match base_url {
        Some(b) if !b.trim().is_empty() => format!("<base href=\"{}\">", html_encode(b)),
        _ => String::new(),
    };

    let size = font_size.clamp(12, 26);
    let font = sanitize_font_family(font_family);
    let margin = style.margin_px.clamp(0, 200);
    let line_height = style.line_height.clamp(1.0, 3.0);
    let indent = style.indent_chars.clamp(0, 8);
    // Images are centered and inset by ~2 Chinese characters on BOTH sides
    // relative to the body text: 2em per side = 4em total width reduction.
    let image_inset_em = 4;
    let (bg, fg, muted, link, border, code_bg, quote_bg) = if dark {
        ("#15191f", "#e6edf3", "#9aa6b6", "#79c0ff", "#30363d", "#21262d", "#1b2129")
    } else {
        ("#ffffff", "#24292f", "#6e7781", "#0969da", "#d8dee4", "#f4f6f8", "#f6f8fa")
    };

    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
{base_tag}
<style>
:root {{
  --bg: {bg};
  --fg: {fg};
  --muted: {muted};
  --link: {link};
  --border: {border};
  --code-bg: {code_bg};
  --quote-bg: {quote_bg};
  --font-size: {size}px;
  --reading-font: "{font}";
  --margin: {margin}px;
  --line-height: {line_height};
  --indent: {indent}em;
}}
* {{ box-sizing: border-box; }}
html {{
  background: var(--bg);
  scrollbar-width: thin;
  scrollbar-color: transparent transparent;
}}
html::-webkit-scrollbar, body::-webkit-scrollbar {{ width: 9px; }}
html::-webkit-scrollbar-track, body::-webkit-scrollbar-track {{ background: transparent; }}
html::-webkit-scrollbar-thumb, body::-webkit-scrollbar-thumb {{
  background: transparent;
  border: 2px solid transparent;
  border-radius: 6px;
  background-clip: content-box;
}}
html:hover::-webkit-scrollbar-thumb, body:hover::-webkit-scrollbar-thumb {{ background: var(--border); }}
html::-webkit-scrollbar-thumb:hover, body::-webkit-scrollbar-thumb:hover {{ background: var(--muted); }}
body {{
  margin: 0 auto;
  padding: 26px var(--margin) 80px;
  max-width: 800px;
  font-family: var(--reading-font), -apple-system, BlinkMacSystemFont, "Segoe UI", "Microsoft YaHei", "PingFang SC", "Hiragino Sans GB", sans-serif;
  font-size: var(--font-size);
  line-height: var(--line-height);
  color: var(--fg);
  background: var(--bg);
  overflow-wrap: break-word;
}}
h1, h2, h3, h4, h5, h6 {{ line-height: 1.4; font-weight: 650; margin: 1.7em 0 0.7em; }}
h1 {{ font-size: 1.75em; }}
h2 {{ font-size: 1.5em; }}
h3 {{ font-size: 1.28em; }}
h4 {{ font-size: 1.12em; }}
h5, h6 {{ font-size: 1em; }}
p {{ margin: 0 0 1.15em; text-indent: var(--indent); }}
a {{ color: var(--link); text-decoration: none; border-bottom: 1px solid var(--border); }}
a:hover {{ border-bottom-color: var(--link); }}
/* Centered, and inset ~2 Chinese characters on each side so images stay
   narrower than the text column. */
img, video {{ max-width: calc(100% - {image_inset_em}em); height: auto; display: block; margin: 1.4em auto; border-radius: 4px; }}
figure {{ margin: 1.4em 0; }}
figcaption {{ color: var(--muted); font-size: 0.86em; text-align: center; margin-top: 0.5em; }}
pre {{ background: var(--code-bg); padding: 14px 18px; border-radius: 6px; overflow-x: auto; font-size: 0.88em; line-height: 1.6; }}
code, kbd, samp {{ font-family: Consolas, "Courier New", monospace; background: var(--code-bg); padding: 2px 5px; border-radius: 3px; font-size: 0.9em; }}
pre code {{ background: transparent; padding: 0; font-size: 1em; }}
blockquote {{ margin: 1.3em 0; padding: 6px 16px; border-left: 3px solid var(--link); background: var(--quote-bg); color: var(--muted); }}
blockquote p:last-child {{ margin-bottom: 0; }}
ul, ol {{ padding-left: 1.9em; margin: 0 0 1.15em; }}
li {{ margin: 0.35em 0; }}
table {{ border-collapse: collapse; width: 100%; margin: 1.4em 0; font-size: 0.94em; }}
th, td {{ border: 1px solid var(--border); padding: 8px 12px; text-align: left; }}
th {{ background: var(--code-bg); }}
hr {{ border: 0; border-top: 1px solid var(--border); margin: 2em 0; }}
sup, sub {{ line-height: 0; }}
</style>
</head>
<body>
{body}
</body>
</html>
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARTICLE: &str = r#"
<!DOCTYPE html><html><head><script>alert('xss')</script><style>.x{color:red}</style></head>
<body><article><h1>测试标题</h1><p>正文内容</p>
<img src="pic.jpg" onerror="alert(1)">
<a href="javascript:alert(2)">恶意链接</a>
<a href="/relative">正常链接</a></article><footer>页脚</footer></body></html>
"#;

    #[test]
    fn injects_css_and_strips_active_content() {
        let out = render(Some(ARTICLE), Some("https://example.com/post/1"), false, 18, None, ReadingStyle::default());
        assert!(out.contains("<style>"));
        assert!(out.contains("line-height: 1.85"));
        assert!(out.contains("--font-size: 18px;"));
        assert!(out.contains("--bg: #ffffff"));
        assert!(out.contains("<base href=\"https://example.com/post/1\">"));
        assert!(out.contains("target=\"_blank\""));
        assert!(!out.contains("alert('xss')"));
        assert!(!out.contains("onerror="));
        assert!(!out.contains("javascript:"));
        assert!(out.contains("测试标题"));
        assert!(out.contains("正文内容"));
        assert!(!out.contains("页脚"));
    }

    #[test]
    fn reading_style_controls_margin_lineheight_indent_and_image_inset() {
        let out = render(
            Some(ARTICLE),
            None,
            false,
            16,
            None,
            ReadingStyle { margin_px: 64, line_height: 2.1, indent_chars: 2 },
        );
        assert!(out.contains("--margin: 64px;"));
        assert!(out.contains("--line-height: 2.1"));
        assert!(out.contains("--indent: 2em;"));
        assert!(out.contains("max-width: calc(100% - 4em)"));
        assert!(out.contains("margin: 1.4em auto"));
    }

    #[test]
    fn dark_theme_and_no_base() {
        let out = render(Some(ARTICLE), None, true, 15, None, ReadingStyle::default());
        assert!(out.contains("--bg: #15191f"));
        assert!(out.contains("--fg: #e6edf3"));
        assert!(!out.contains("<base"));
    }

    #[test]
    fn font_sanitization() {
        let out = render(Some(ARTICLE), None, false, 15, Some("Source Han Serif SC"), ReadingStyle::default());
        assert!(out.contains("--reading-font: \"Source Han Serif SC\""));
        assert!(out.contains("font-family: var(--reading-font)"));
        assert_eq!(sanitize_font_family(Some("Arial\"; color:red")), "Arial color:red");
        assert_eq!(sanitize_font_family(Some("  ")), "Microsoft YaHei");
    }
}
