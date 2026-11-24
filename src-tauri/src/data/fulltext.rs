//! Full-text extraction from a downloaded article web page.
//!
//! Ported in spirit from ReadYou's full-text feature: download the original
//! page, then pick the most likely content block using a Readability-style
//! text-density score rather than returning the whole page chrome.
//!
//! No external DOM dependency: a small tolerant tokenizer builds a tree, each
//! block node is scored, and the best-scoring subtree is serialized back to
//! sanitized HTML for the existing CSS renderer.

use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Clone)]
enum Node {
    Element {
        tag: String,
        attrs: Vec<(String, String)>,
        children: Vec<Node>,
    },
    Text(String),
}

const VOID_TAGS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param",
    "source", "track", "wbr",
];

const SKIP_TAGS: &[&str] = &[
    "script", "style", "noscript", "template", "svg", "canvas", "form", "button", "select",
    "iframe", "object", "video", "audio",
];

const BLOCK_TAGS: &[&str] = &["article", "main", "div", "section", "td", "body", "figure"];

const NEGATIVE_HINTS: &[&str] = &[
    "comment", "footer", "header", "nav", "sidebar", "menu", "related", "recommend", "share",
    "advert", "promo", "breadcrumb", "pagination", "widget", "toolbar", "cookie", "popup",
    "modal", "login", "subscribe",
];

const POSITIVE_HINTS: &[&str] = &[
    "article", "content", "post", "entry", "main", "story", "detail", "news",
];

fn tag_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?is)<!--.*?-->|<(/?)([a-zA-Z][a-zA-Z0-9:-]*)([^>]*)>").unwrap())
}

fn attr_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?i)([a-zA-Z_:][-a-zA-Z0-9_:.]*)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))"#)
            .unwrap()
    })
}

fn event_attr_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?i)^on[a-z]+$"#).unwrap())
}

fn dangerous_url_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?i)^\s*(javascript|data|vbscript):"#).unwrap())
}

fn parse_attrs(raw: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for caps in attr_re().captures_iter(raw) {
        let name = caps.get(1).map(|m| m.as_str()).unwrap_or("").to_ascii_lowercase();
        if name.is_empty() || event_attr_re().is_match(&name) {
            continue;
        }
        let value = caps
            .get(2)
            .or_else(|| caps.get(3))
            .or_else(|| caps.get(4))
            .map(|m| m.as_str())
            .unwrap_or("");
        if (name == "href" || name == "src" || name == "srcset") && dangerous_url_re().is_match(value) {
            continue;
        }
        out.push((name, value.to_string()));
    }
    out
}

/// A frame on the open-element stack. `skip_depth` marks subtrees we discard.
struct Frame {
    tag: String,
    attrs: Vec<(String, String)>,
    children: Vec<Node>,
    skip: bool,
}

fn add_text(stack: &mut [Frame], text: &str) {
    if text.trim().is_empty() {
        return;
    }
    if let Some(frame) = stack.last_mut() {
        if !frame.skip {
            frame.children.push(Node::Text(text.to_string()));
        }
    }
}

/// Tolerant tokenizer: unbalanced tags close implicitly at the end, and
/// mismatched closing tags pop to the nearest matching open element.
fn parse_tree(html: &str) -> Node {
    let mut stack: Vec<Frame> = vec![Frame {
        tag: "#root".to_string(),
        attrs: Vec::new(),
        children: Vec::new(),
        skip: false,
    }];
    let mut pos = 0usize;

    for caps in tag_re().captures_iter(html) {
        let whole = caps.get(0).unwrap();
        let start = whole.start();
        if start > pos {
            add_text(&mut stack, &html[pos..start]);
        }
        pos = whole.end();
        if whole.as_str().starts_with("<!--") {
            continue;
        }
        let closing = caps.get(1).map(|m| m.as_str() == "/").unwrap_or(false);
        let tag = caps
            .get(2)
            .map(|m| m.as_str().to_ascii_lowercase())
            .unwrap_or_default();
        if tag.is_empty() {
            continue;
        }
        if closing {
            if let Some(idx) = stack
                .iter()
                .rposition(|f| f.tag == tag && f.tag != "#root")
            {
                let drained: Vec<Frame> = stack.drain(idx..).collect();
                // Innermost first: attach each frame as a child of the one below it.
                let mut node: Option<Node> = None;
                for frame in drained.into_iter().rev() {
                    let Frame { tag, attrs, children, skip } = frame;
                    if skip {
                        node = None;
                        continue;
                    }
                    let mut kids = children;
                    if let Some(n) = node.take() {
                        kids.push(n);
                    }
                    node = Some(Node::Element { tag, attrs, children: kids });
                }
                if let Some(n) = node {
                    if let Some(parent) = stack.last_mut() {
                        if !parent.skip {
                            parent.children.push(n);
                        }
                    }
                }
            }
            continue;
        }

        let attrs = parse_attrs(caps.get(3).map(|m| m.as_str()).unwrap_or(""));
        if SKIP_TAGS.contains(&tag.as_str()) {
            stack.push(Frame {
                tag,
                attrs: Vec::new(),
                children: Vec::new(),
                skip: true,
            });
            continue;
        }
        if VOID_TAGS.contains(&tag.as_str()) {
            if let Some(parent) = stack.last_mut() {
                if !parent.skip {
                    parent.children.push(Node::Element {
                        tag,
                        attrs,
                        children: Vec::new(),
                    });
                }
            }
            continue;
        }
        stack.push(Frame {
            tag,
            attrs,
            children: Vec::new(),
            skip: false,
        });
    }
    if pos < html.len() {
        add_text(&mut stack, &html[pos..]);
    }

    // Collapse all still-open frames into the root.
    let mut children = Vec::new();
    let mut node: Option<Node> = None;
    for frame in stack.into_iter().rev() {
        let Frame { tag, attrs, children: kids, skip } = frame;
        if skip {
            continue;
        }
        let mut kids = kids;
        if let Some(n) = node.take() {
            kids.push(n);
        }
        if tag == "#root" {
            children = kids;
            break;
        }
        node = Some(Node::Element { tag, attrs, children: kids });
    }
    Node::Element {
        tag: "#root".to_string(),
        attrs: Vec::new(),
        children,
    }
}

fn attr_value<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

fn node_text(node: &Node, out: &mut String) {
    match node {
        Node::Text(t) => out.push_str(t),
        Node::Element { children, .. } => {
            for c in children {
                node_text(c, out);
            }
        }
    }
}

fn count_tag(node: &Node, tag: &str, out: &mut usize) {
    if let Node::Element { tag: t, children, .. } = node {
        if t == tag {
            *out += 1;
        }
        for c in children {
            count_tag(c, tag, out);
        }
    }
}

fn link_text_len(node: &Node) -> usize {
    fn walk(node: &Node, in_link: bool, out: &mut String) {
        match node {
            Node::Text(t) => {
                if in_link {
                    out.push_str(t);
                }
            }
            Node::Element { tag, children, .. } => {
                let now_link = in_link || tag == "a";
                for c in children {
                    walk(c, now_link, out);
                }
            }
        }
    }
    let mut s = String::new();
    walk(node, false, &mut s);
    s.trim().chars().count()
}

fn node_score(node: &Node) -> i64 {
    let mut text = String::new();
    node_text(node, &mut text);
    let len = text.trim().chars().count() as i64;
    if len < 40 {
        return 0;
    }
    let mut paragraphs = 0usize;
    let mut breaks = 0usize;
    let mut anchors = 0usize;
    count_tag(node, "p", &mut paragraphs);
    count_tag(node, "br", &mut breaks);
    count_tag(node, "a", &mut anchors);
    let link_chars = link_text_len(node) as i64;
    // Text that is NOT inside a link is the strongest signal of article body:
    // navigation / menu / related blocks are almost entirely anchor text.
    let prose = len - link_chars;
    if prose < 40 {
        return 0;
    }
    let mut bonus = (paragraphs as i64) * 30 + (breaks as i64) * 3;

    if let Node::Element { tag, attrs, .. } = node {
        let mut hints = String::new();
        if let Some(id) = attr_value(attrs, "id") {
            hints.push_str(&id.to_ascii_lowercase());
        }
        if let Some(class) = attr_value(attrs, "class") {
            hints.push(' ');
            hints.push_str(&class.to_ascii_lowercase());
        }
        if POSITIVE_HINTS.iter().any(|h| hints.contains(h)) {
            bonus += 150;
        }
        if NEGATIVE_HINTS.iter().any(|h| hints.contains(h)) {
            bonus -= 500;
        }
        if tag == "article" || tag == "main" {
            bonus += 300;
        }
    }
    // Each link is penalized heavily: a page-wide container full of nav links
    // must lose to the deep, prose-heavy article block inside it.
    prose + bonus - (anchors as i64) * 40
}

fn best_node<'a>(node: &'a Node, best: &mut Option<(i64, &'a Node)>) {
    match node {
        Node::Text(_) => {}
        Node::Element { tag, children, .. } => {
            if BLOCK_TAGS.contains(&tag.as_str()) {
                let score = node_score(node);
                if score > 0 && best.as_ref().map(|(s, _)| score > *s).unwrap_or(true) {
                    *best = Some((score, node));
                }
            }
            for c in children {
                best_node(c, best);
            }
        }
    }
}

fn serialize(node: &Node, out: &mut String, depth: usize) {
    if depth > 80 {
        return;
    }
    match node {
        Node::Text(t) => out.push_str(t),
        Node::Element { tag, attrs, children } => {
            out.push('<');
            out.push_str(tag);
            for (k, v) in attrs {
                if matches!(
                    k.as_str(),
                    "href" | "src" | "srcset" | "class" | "id" | "alt" | "title" | "width"
                        | "height" | "style" | "data-src" | "data-original"
                        | "data-lazy-src" | "data-echo"
                ) {
                    out.push(' ');
                    out.push_str(k);
                    out.push_str("=\"");
                    out.push_str(&v.replace('"', "&quot;"));
                    out.push('"');
                }
            }
            out.push('>');
            for c in children {
                serialize(c, out, depth + 1);
            }
            if !VOID_TAGS.contains(&tag.as_str()) {
                out.push_str("</");
                out.push_str(tag);
                out.push('>');
            }
        }
    }
}

/// Decode downloaded bytes into text using the HTTP charset, falling back to
/// a `<meta charset=...>` declaration in the first part of the document.
pub fn decode_html(bytes: &[u8], content_type: Option<&str>) -> String {
    let head_len = bytes.len().min(8192);
    let head = String::from_utf8_lossy(&bytes[..head_len]);

    // Prefer the HTTP Content-Type charset; many sites omit it and declare
    // GBK/GB18030 only in a <meta> tag, so check that as a fallback.
    let mut labels: Vec<String> = Vec::new();
    if let Some(ct) = content_type {
        let lowered = ct.to_ascii_lowercase();
        if let Some(pos) = lowered.find("charset=") {
            let label: String = ct[pos + "charset=".len()..]
                .chars()
                .skip_while(|c| c.is_whitespace() || *c == '"' || *c == '\'')
                .take_while(|c| !matches!(c, ';' | '"' | '\'' | ' '))
                .collect();
            if !label.is_empty() {
                labels.push(label);
            }
        }
    }
    let lowered_head = head.to_ascii_lowercase();
    if let Some(pos) = lowered_head.find("charset=") {
        let label: String = head[pos + "charset=".len()..]
            .chars()
            .skip_while(|c| c.is_whitespace() || *c == '"' || *c == '\'')
            .take_while(|c| !matches!(c, '"' | '\'' | ';' | '>' | ' '))
            .collect();
        if !label.is_empty() {
            labels.push(label);
        }
    }
    for label in labels {
        if let Some(enc) = encoding_rs::Encoding::for_label(label.as_bytes()) {
            return enc.decode(bytes).0.into_owned();
        }
    }
    String::from_utf8_lossy(bytes).into_owned()
}

/// Extract the article body from a downloaded HTML page.
/// Returns an empty string when no plausible content block is found.
pub fn extract(page_html: &str) -> String {
    let tree = parse_tree(page_html);
    let mut best: Option<(i64, &Node)> = None;
    best_node(&tree, &mut best);
    let Some((_, node)) = best else {
        return String::new();
    };
    let mut out = String::new();
    // Serialize the chosen block's children (not the wrapper) so its layout
    // class/id doesn't leak into the reading pane.
    if let Node::Element { children, .. } = node {
        for c in children {
            serialize(c, &mut out, 0);
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r#"
    <!DOCTYPE html><html><head><title>x</title><script>var a=1;</script><style>.x{}</style></head>
    <body>
      <nav class="nav-menu"><a href="/">首页</a><a href="/a">栏目一</a><a href="/b">栏目二</a></nav>
      <div class="page">
        <article class="post-content">
          <h1>标题：这是一篇测试文章</h1>
          <p>第一段文字内容，包含足够多的中文字符以便通过文本密度评分的阈值检查。</p>
          <p>第二段文字内容，继续讲述事件的来龙去脉和一些细节信息，让正文显得更加充实完整。</p>
          <blockquote>引用的一段话。</blockquote>
          <p>第三段内容。</p>
        </article>
        <div class="related-posts"><a href="/r1">相关文章一</a><a href="/r2">相关文章二</a></div>
        <footer class="site-footer">版权所有 联系我们</footer>
      </div>
    </body></html>"#;

    #[test]
    fn picks_article_and_drops_chrome() {
        let body = extract(PAGE);
        assert!(body.contains("这是一篇测试文章"), "got: {body}");
        assert!(body.contains("第二段文字内容"));
        assert!(!body.contains("var a=1"));
        assert!(!body.contains("栏目一"), "nav must be dropped, got: {body}");
        assert!(!body.contains("相关文章一"), "related must be dropped, got: {body}");
        assert!(!body.contains("版权所有"), "footer must be dropped, got: {body}");
    }

    #[test]
    fn empty_page_gives_empty_body() {
        assert_eq!(extract("").trim(), "");
        assert_eq!(extract("<html><body></body></html>").trim(), "");
    }

    #[test]
    fn forum_layout_picks_post_body_not_navigation() {
        // Mimics a Discuz forum thread: a huge link-heavy nav plus a deep
        // prose-heavy post body. The post body must win.
        let mut nav = String::new();
        for i in 0..40 {
            nav.push_str(&format!("<a href='/f{i}'>论坛栏目{i}</a>"));
        }
        let mut post = String::from("<td class=\"t_f\" id=\"postmessage_1\">");
        for i in 0..15 {
            post.push_str(&format!("<p>这是正文第{i}段，讲述九寨沟的彩林与湖水景色，内容足够充实。</p>"));
        }
        post.push_str("</td>");
        let page = format!(
            "<html><body><div class=\"page\">{nav}<div class=\"post\">{post}</div><div class=\"related\"><a href='/r1'>相关一</a></div></div></body></html>"
        );
        let body = extract(&page);
        assert!(body.contains("正文第7段"), "got: {}", &body[..body.len().min(300)]);
        assert!(!body.contains("论坛栏目0"), "nav must be dropped");
        assert!(!body.contains("相关一"), "related must be dropped");
    }

    #[test]
    fn decodes_gbk_meta_charset() {
        // GBK bytes for a Chinese title, declared in a <meta> tag.
        let (gbk, _, _) = encoding_rs::GBK.encode("<html><head><meta charset=\"gbk\"></head><body>中文正文</body></html>");
        let text = decode_html(&gbk, None);
        assert!(text.contains("中文正文"), "got: {text}");
    }

    #[test]
    fn skips_script_content() {
        let body = extract(r#"<div class="content"><p>正文内容很多很多很多很多很多字，足够长度用于通过文本密度评分的阈值检查，确保这段正文可以被正确提取出来并保留。</p><script>alert(1)</script></div>"#);
        assert!(body.contains("正文内容"));
        assert!(!body.contains("alert(1)"));
    }
}

#[cfg(test)]
mod real_page_check {
    use super::*;

    #[test]
    fn chiphell_real_page() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/data/fixtures/chiphell_page.html");
        let Ok(html) = std::fs::read_to_string(path) else { return; };
        let body = extract(&html);
        let text = crate::data::html::to_text(Some(&body), 100000);
        println!("BODY LEN {} TEXT LEN {}", body.len(), text.len());
        println!("HEAD: {}", &text[..text.len().min(500)]);
        assert!(text.contains("七月底看到了九寨沟"), "post body missing");
        assert!(!text.contains("设为首页"), "top nav leaked");
        assert!(!text.contains("收藏本站"), "top nav leaked");
    }
}
