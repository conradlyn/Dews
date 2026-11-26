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

/// Scrollbar and outline styling. Kept out of the `format!` template so the
/// braces do not have to be escaped.
const EXTRA_CSS: &str = r#"
/* The thumb stays out of the way until the reader actually scrolls, then fades
   back out — the overlay behaviour macOS and Windows both use natively.
   `scrollbar-color` is set on the reveal state as well: current WebKit and
   Chromium ignore the ::-webkit-scrollbar rules as soon as it is specified,
   so styling only the pseudo-elements leaves the thumb invisible. */
html { scroll-behavior: smooth; scrollbar-width: thin; scrollbar-color: transparent transparent; }
html.is-scrolling { scrollbar-color: var(--scroll-thumb) transparent; }
html::-webkit-scrollbar { width: 10px; height: 10px; }
html::-webkit-scrollbar-track { background: transparent; }
html::-webkit-scrollbar-thumb {
  background: transparent;
  border: 2px solid transparent;
  border-radius: 6px;
  background-clip: content-box;
}
html.is-scrolling::-webkit-scrollbar-thumb { background: var(--scroll-thumb); }
html.is-scrolling::-webkit-scrollbar-thumb:hover { background: var(--scroll-thumb-hover); }

/* Anchored headings keep clear of the top edge when jumped to. */
h1, h2, h3, h4, h5, h6 { scroll-margin-top: 18px; }

/* Floating outline built by the script below. */
.dews-toc {
  position: fixed;
  top: 12px;
  right: 20px;
  z-index: 20;
  width: 208px;
  max-height: calc(100vh - 24px);
  display: flex;
  flex-direction: column;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--toc-bg);
  box-shadow: var(--toc-shadow);
  -webkit-backdrop-filter: blur(10px);
  backdrop-filter: blur(10px);
  overflow: hidden;
  font-size: 12.5px;
  line-height: 1.5;
}
.dews-toc.collapsed { width: auto; }
.dews-toc.collapsed .dews-toc-list,
.dews-toc.collapsed .dews-toc-title { display: none; }
.dews-toc.collapsed .dews-toc-toggle { padding: 6px 9px; }
.dews-toc-toggle {
  display: flex;
  align-items: center;
  gap: 6px;
  width: 100%;
  padding: 7px 10px;
  border: 0;
  background: transparent;
  color: var(--muted);
  font: inherit;
  cursor: pointer;
}
.dews-toc-toggle:hover { color: var(--fg); }
.dews-toc-icon { font-size: 14px; line-height: 1; }
.dews-toc-list {
  margin: 0;
  padding: 0 8px 8px;
  list-style: none;
  overflow-y: auto;
  max-height: calc(100vh - 76px);
  scrollbar-width: thin;
  scrollbar-color: var(--scroll-thumb) transparent;
}
.dews-toc-item { margin: 1px 0; }
.dews-toc-item a {
  display: block;
  padding: 3px 8px;
  border: 0;
  border-radius: 5px;
  color: var(--muted);
  text-decoration: none;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.dews-toc-item a:hover { color: var(--fg); background: var(--code-bg); }
.dews-toc-item a.on { color: var(--fg); background: var(--code-bg); font-weight: 600; }
.dews-toc-item.lvl1 a { font-weight: 600; }
.dews-toc-item.lvl3 a { padding-left: 18px; }
.dews-toc-item.lvl4 a { padding-left: 28px; }
"#;

/// Runs inside the article frame. It is appended by us, after `clean` has
/// stripped every script the article itself carried, so it is the only active
/// content in the document.
const EXTRA_JS: &str = r##"<script>
(function () {
  "use strict";
  var root = document.documentElement;
  var hideTimer = 0;

  // Reveal the scrollbar while the reader scrolls, then let it fade out.
  function flashScrollbar() {
    root.classList.add("is-scrolling");
    clearTimeout(hideTimer);
    hideTimer = setTimeout(function () { root.classList.remove("is-scrolling"); }, 900);
  }
  window.addEventListener("scroll", flashScrollbar, { passive: true });

  var heads = Array.prototype.slice
    .call(document.querySelectorAll("h1, h2, h3, h4"))
    .filter(function (h) { return (h.textContent || "").trim() !== ""; });
  // Short pieces do not need an outline.
  if (heads.length < 3) return;

  var seen = Object.create(null);
  heads.forEach(function (h, i) {
    var id = h.getAttribute("id");
    if (!id || seen[id]) {
      id = "dews-h" + (i + 1);
      h.setAttribute("id", id);
    }
    seen[id] = true;
  });

  var nav = document.createElement("nav");
  nav.className = "dews-toc";
  nav.setAttribute("aria-label", "目录");

  var toggle = document.createElement("button");
  toggle.type = "button";
  toggle.className = "dews-toc-toggle";
  var icon = document.createElement("span");
  icon.className = "dews-toc-icon";
  icon.textContent = "≡";
  var label = document.createElement("span");
  label.className = "dews-toc-title";
  label.textContent = "目录";
  toggle.appendChild(icon);
  toggle.appendChild(label);
  nav.appendChild(toggle);

  var list = document.createElement("ol");
  list.className = "dews-toc-list";
  var links = [];
  heads.forEach(function (h) {
    var li = document.createElement("li");
    li.className = "dews-toc-item lvl" + h.tagName.slice(1);
    var a = document.createElement("a");
    a.href = "#" + h.id;
    a.textContent = (h.textContent || "").trim().slice(0, 80);
    a.addEventListener("click", function (ev) {
      ev.preventDefault();
      h.scrollIntoView({ behavior: "smooth", block: "start" });
      setCollapsed(true);
    });
    li.appendChild(a);
    list.appendChild(li);
    links.push(a);
  });
  nav.appendChild(list);
  document.body.appendChild(nav);

  function setCollapsed(value) {
    nav.classList.toggle("collapsed", value);
  }
  toggle.addEventListener("click", function () {
    setCollapsed(!nav.classList.contains("collapsed"));
  });
  // Expanded only when the outline fits beside the 800px text column
  // (208px panel + 20px inset + a 12px gap).
  setCollapsed(window.innerWidth - 800 < 480);

  var active = -1;
  function highlightCurrent() {
    var current = 0;
    for (var i = 0; i < heads.length; i++) {
      if (heads[i].getBoundingClientRect().top > 90) break;
      current = i;
    }
    if (current === active) return;
    active = current;
    links.forEach(function (a, i) { a.classList.toggle("on", i === current); });
  }
  window.addEventListener("scroll", highlightCurrent, { passive: true });
  highlightCurrent();
})();
</script>"##;

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
    let (scroll_thumb, scroll_thumb_hover, toc_bg, toc_shadow) = if dark {
        (
            "rgba(230,237,243,0.26)",
            "rgba(230,237,243,0.48)",
            "rgba(21,25,31,0.94)",
            "0 8px 24px rgba(0,0,0,0.55)",
        )
    } else {
        (
            "rgba(36,41,47,0.24)",
            "rgba(36,41,47,0.46)",
            "rgba(255,255,255,0.94)",
            "0 8px 24px rgba(15,23,42,0.14)",
        )
    };

    let extra_css = EXTRA_CSS;
    let extra_js = EXTRA_JS;

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
  --scroll-thumb: {scroll_thumb};
  --scroll-thumb-hover: {scroll_thumb_hover};
  --toc-bg: {toc_bg};
  --toc-shadow: {toc_shadow};
}}
* {{ box-sizing: border-box; }}
html {{
  background: var(--bg);
}}
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
{extra_css}
</style>
</head>
<body>
{body}
{extra_js}
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
    fn scrollbar_thumb_is_transparent_until_the_reader_scrolls() {
        let out = render(Some(ARTICLE), None, false, 16, None, ReadingStyle::default());
        // Hidden by default, revealed through the class the script toggles.
        assert!(out.contains("scrollbar-color: transparent transparent"));
        assert!(out.contains("html.is-scrolling { scrollbar-color: var(--scroll-thumb) transparent; }"));
        assert!(out.contains("html.is-scrolling::-webkit-scrollbar-thumb { background: var(--scroll-thumb); }"));
        assert!(out.contains("--scroll-thumb: rgba(36,41,47,0.24);"));
        // The old rule revealed the thumb on hover only, which current WebKit
        // ignores once `scrollbar-color` is set.
        assert!(!out.contains("html:hover::-webkit-scrollbar-thumb"));
    }

    #[test]
    fn dark_theme_scrollbar_and_outline_colours() {
        let out = render(Some(ARTICLE), None, true, 16, None, ReadingStyle::default());
        assert!(out.contains("--scroll-thumb: rgba(230,237,243,0.26);"));
        assert!(out.contains("--toc-bg: rgba(21,25,31,0.94);"));
    }

    #[test]
    fn outline_script_is_appended_after_the_article_body() {
        let out = render(Some(ARTICLE), None, false, 16, None, ReadingStyle::default());
        let script = out.find("<script>").expect("outline script missing");
        let body = out.find("正文内容").expect("article body missing");
        assert!(script > body, "the script must come after the article body");
        assert!(out[script..].contains("dews-toc"));
        // The article's own script is still stripped; ours is the only one left.
        assert_eq!(out.matches("<script>").count(), 1);
        assert!(out.contains("h1, h2, h3, h4"));
    }

    #[test]
    fn outline_needs_at_least_three_headings() {
        let out = render(Some(ARTICLE), None, false, 16, None, ReadingStyle::default());
        // The threshold lives in the script so short pieces stay clean.
        assert!(out.contains("heads.length < 3"));
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
