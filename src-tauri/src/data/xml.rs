//! Minimal DTD-free XML tree. Mirrors the LINQ-to-XML behaviour the WPF
//! edition relied on: local names only, case-insensitive lookup, CDATA/text
//! concatenation, and attribute access by local name.

use quick_xml::events::Event;
use quick_xml::Reader;
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct XmlNode {
    pub local_name: String,
    pub attrs: HashMap<String, String>,
    pub text: String,
    pub children: Vec<XmlNode>,
}

impl XmlNode {
    /// First direct child matching `name` (case-insensitive, namespace stripped).
    pub fn find(&self, name: &str) -> Option<&XmlNode> {
        self.children
            .iter()
            .find(|c| c.local_name.eq_ignore_ascii_case(name))
    }

    /// All direct children matching `name`.
    pub fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a XmlNode> + 'a {
        self.children
            .iter()
            .filter(move |c| c.local_name.eq_ignore_ascii_case(name))
    }

    /// Attribute by local name (case-insensitive key match).
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Trimmed text of the first matching child.
    pub fn child_text(&self, name: &str) -> Option<String> {
        self.find(name).map(|c| c.text.trim().to_string())
    }

    /// C# `ContentOf`: inner markup when the node has element children, else text.
    pub fn content(&self) -> String {
        if self.children.is_empty() {
            self.text.clone()
        } else {
            self.inner_xml()
        }
    }

    /// Reconstruct inner XML markup (used for HTML-in-XML bodies).
    pub fn inner_xml(&self) -> String {
        let mut s = String::new();
        if !self.text.trim().is_empty() {
            s.push_str(&self.text);
        }
        for child in &self.children {
            s.push_str(&child.to_xml());
        }
        s
    }

    pub fn to_xml(&self) -> String {
        let mut s = String::new();
        s.push('<');
        s.push_str(&self.local_name);
        for (k, v) in &self.attrs {
            s.push(' ');
            s.push_str(k);
            s.push_str("=\"");
            s.push_str(&escape_attr(v));
            s.push('"');
        }
        s.push('>');
        s.push_str(&self.inner_xml());
        s.push_str("</");
        s.push_str(&self.local_name);
        s.push('>');
        s
    }
}

fn escape_attr(v: &str) -> String {
    v.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn local(name: &[u8]) -> String {
    let raw = String::from_utf8_lossy(name);
    // strip namespace prefix: "media:thumbnail" -> "thumbnail"
    raw.rsplit(':').next().unwrap_or(&raw).to_string()
}

/// Parse XML into a tree. DTD is prohibited (no external entities resolved).
fn attach(stack: &mut Vec<XmlNode>, root: &mut Option<XmlNode>, node: XmlNode) {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(node);
    } else if root.is_none() {
        *root = Some(node);
    }
}

fn build_node(e: &quick_xml::events::BytesStart<'_>) -> Result<XmlNode, String> {
    let name = local(e.name().as_ref());
    let mut attrs = HashMap::new();
    for a in e.attributes() {
        let a = a.map_err(|e| e.to_string())?;
        let key = local(a.key.as_ref());
        let val = a.unescape_value().map_err(|e| e.to_string())?.to_string();
        attrs.insert(key, val);
    }
    Ok(XmlNode {
        local_name: name,
        attrs,
        text: String::new(),
        children: Vec::new(),
    })
}

pub fn parse(xml: &str) -> Result<XmlNode, String> {
    let mut reader = Reader::from_str(xml);
    reader.expand_empty_elements(false);
    reader.check_end_names(false);
    reader.trim_text(false);

    let mut stack: Vec<XmlNode> = Vec::new();
    let mut root: Option<XmlNode> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Eof) => break,
            Ok(Event::Text(t)) => {
                if let Some(node) = stack.last_mut() {
                    node.text.push_str(&html_escape::decode_html_entities(
                        &t.unescape().map_err(|e| e.to_string())?,
                    ));
                }
            }
            Ok(Event::CData(t)) => {
                if let Some(node) = stack.last_mut() {
                    node.text.push_str(&String::from_utf8_lossy(&t));
                }
            }
            Ok(Event::Start(e)) => {
                if local(e.name().as_ref()).eq_ignore_ascii_case("doctype") {
                    return Err("DTD 不被允许。".to_string());
                }
                let node = build_node(&e)?;
                stack.push(node);
            }
            Ok(Event::Empty(e)) => {
                if local(e.name().as_ref()).eq_ignore_ascii_case("doctype") {
                    return Err("DTD 不被允许。".to_string());
                }
                let node = build_node(&e)?;
                attach(&mut stack, &mut root, node);
            }
            Ok(Event::End(_)) => {
                if let Some(node) = stack.pop() {
                    attach(&mut stack, &mut root, node);
                }
            }
            Ok(Event::DocType(_)) => return Err("DTD 不被允许。".to_string()),
            Ok(Event::Decl(_)) | Ok(Event::PI(_)) | Ok(Event::Comment(_)) => {}
            Err(e) => return Err(format!("XML 解析失败：{}", e)),
        }
    }

    root.ok_or_else(|| "空的 XML 文档。".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cdata_and_local_names() {
        let doc = parse(
            r#"<rss xmlns:media="x"><channel><title>T</title><item><media:thumbnail url="https://a.b/c.jpg"/><description><![CDATA[<p>hi &amp; bye</p>]]></description></item></channel></rss>"#,
        )
        .unwrap();
        assert_eq!(doc.local_name, "rss");
        let channel = doc.find("channel").unwrap();
        assert_eq!(channel.child_text("title").as_deref(), Some("T"));
        let item = channel.find("item").unwrap();
        assert_eq!(
            item.find("thumbnail").unwrap().attr("url"),
            Some("https://a.b/c.jpg")
        );
        assert!(item.find("description").unwrap().text.contains("<p>hi &amp; bye</p>"));
    }

    #[test]
    fn rejects_dtd() {
        assert!(parse("<!DOCTYPE rss [<!ENTITY x SYSTEM 'file:///etc/passwd'>]><rss/>").is_err());
    }
}
