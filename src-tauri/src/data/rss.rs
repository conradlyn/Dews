//! Minimal DTD-free RSS 2.0 / Atom 1.0 parser, ported from RssParser.cs.

use crate::data::model::new_id;
use crate::data::thumbnail;
use crate::data::xml::{self, XmlNode};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct ParsedArticle {
    pub id: String,
    pub title: String,
    pub link: Option<String>,
    pub published: i64, // unix seconds
    pub summary_html: Option<String>,
    pub author: Option<String>,
    pub thumbnail_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ParsedFeed {
    pub title: String,
    pub site_url: Option<String>,
    pub items: Vec<ParsedArticle>,
}

pub fn parse(xml: &str, base_url: Option<&str>) -> Result<ParsedFeed, String> {
    let root = xml::parse(xml)?;
    match root.local_name.as_str() {
        "rss" => parse_rss(&root, base_url),
        "feed" => parse_atom(&root, base_url),
        other => Err(format!("不是受支持的订阅格式：{other}")),
    }
}

fn parse_rss(root: &XmlNode, base_url: Option<&str>) -> Result<ParsedFeed, String> {
    let channel = root.find("channel").unwrap_or(root);
    let title = channel.child_text("title").unwrap_or_else(|| "(无标题订阅)".to_string());
    let site = channel.child_text("link");

    let mut items = Vec::new();
    for item in channel.all("item") {
        let link = item.child_text("link");
        let id = item
            .child_text("guid")
            .or_else(|| link.clone())
            .unwrap_or_else(new_id);
        let summary = item
            .find("description")
            .or_else(|| item.find("encoded"))
            .map(XmlNode::content);
        let author = item.child_text("creator").or_else(|| item.child_text("author"));
        let published = parse_date(
            &item.child_text("pubDate").or_else(|| item.child_text("date")),
        );
        items.push(ParsedArticle {
            id,
            title: item.child_text("title").unwrap_or_else(|| "(无标题)".to_string()),
            link,
            published,
            summary_html: summary,
            author,
            thumbnail_url: thumbnail::from_feed_item(item, base_url.or(site.as_deref())),
        });
    }

    Ok(ParsedFeed { title, site_url: site, items })
}

fn parse_atom(root: &XmlNode, base_url: Option<&str>) -> Result<ParsedFeed, String> {
    let title = root.find("title").map(XmlNode::content).unwrap_or_else(|| "(无标题订阅)".to_string());
    let mut site: Option<String> = None;
    for link in root.all("link") {
        let rel = link.attr("rel");
        if rel.is_none() || rel.map(|r| r.eq_ignore_ascii_case("alternate")).unwrap_or(false) {
            site = link.attr("href").map(str::to_string);
            break;
        }
    }

    let mut items = Vec::new();
    for entry in root.all("entry") {
        let id = entry.child_text("id").unwrap_or_else(new_id);
        let mut link: Option<String> = None;
        for candidate in entry.all("link") {
            let rel = candidate.attr("rel");
            let href = candidate.attr("href");
            let Some(href) = href else { continue };
            if rel.is_none() || rel.map(|r| r.eq_ignore_ascii_case("alternate")).unwrap_or(false) {
                link = Some(href.to_string());
                break;
            }
            if link.is_none() {
                link = Some(href.to_string());
            }
        }
        let summary = entry
            .find("summary")
            .or_else(|| entry.find("content"))
            .map(XmlNode::content);
        let author = entry.find("author").and_then(|a| a.child_text("name"));
        let published = parse_date(
            &entry.child_text("published").or_else(|| entry.child_text("updated")),
        );
        items.push(ParsedArticle {
            id,
            title: entry.find("title").map(XmlNode::content).unwrap_or_else(|| "(无标题)".to_string()),
            link,
            published,
            summary_html: summary,
            author,
            thumbnail_url: thumbnail::from_feed_item(entry, base_url.or(site.as_deref())),
        });
    }

    Ok(ParsedFeed { title, site_url: site, items })
}

fn parse_date(value: &Option<String>) -> i64 {
    let Some(value) = value else { return now_secs() };
    if value.trim().is_empty() {
        return now_secs();
    }
    DateTime::parse_from_rfc2822(value.trim())
        .or_else(|_| DateTime::parse_from_rfc3339(value.trim()))
        .or_else(|_| DateTime::parse_from_str(value.trim(), "%Y-%m-%dT%H:%M:%S%z"))
        .or_else(|_| DateTime::parse_from_str(value.trim(), "%Y-%m-%d %H:%M:%S"))
        .map(|dt| dt.with_timezone(&Utc).timestamp())
        .unwrap_or_else(|_| now_secs())
}

fn now_secs() -> i64 {
    Utc::now().timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::html;

    const RSS: &str = r#"
<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0" xmlns:content="http://purl.org/rss/1.0/modules/content/" xmlns:dc="http://purl.org/dc/elements/1.1/">
  <channel>
    <title>测试 RSS</title>
    <link>https://example.com</link>
    <item>
      <title>第一篇文章</title>
      <link>https://example.com/1</link>
      <guid>https://example.com/1</guid>
      <pubDate>Mon, 23 Nov 2026 08:00:00 +0800</pubDate>
      <dc:creator>Conrad</dc:creator>
      <description><![CDATA[<p>摘要文本 &amp; 更多</p>]]></description>
      <content:encoded><![CDATA[<p>正文内容</p>]]></content:encoded>
    </item>
  </channel>
</rss>
"#;

    const ATOM: &str = r#"
<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>测试 Atom</title>
  <link rel="alternate" href="https://atom.example.com"/>
  <entry>
    <title>Atom 条目</title>
    <id>urn:uuid:1234</id>
    <link rel="alternate" href="https://atom.example.com/entry"/>
    <updated>2026-11-22T10:00:00Z</updated>
    <summary type="html">&lt;p&gt;Atom 摘要&lt;/p&gt;</summary>
  </entry>
</feed>
"#;

    #[test]
    fn parses_rss_core_fields() {
        let feed = parse(RSS, None).unwrap();
        assert_eq!(feed.title, "测试 RSS");
        assert_eq!(feed.items.len(), 1);
        assert_eq!(feed.items[0].title, "第一篇文章");
        assert_eq!(feed.items[0].author.as_deref(), Some("Conrad"));
        let text = html::to_text(feed.items[0].summary_html.as_deref(), 120000);
        assert!(text.contains("摘要文本 & 更多"));
    }

    #[test]
    fn parses_atom_core_fields() {
        let feed = parse(ATOM, None).unwrap();
        assert_eq!(feed.title, "测试 Atom");
        assert_eq!(feed.items.len(), 1);
        assert_eq!(feed.items[0].id, "urn:uuid:1234");
        assert_eq!(feed.items[0].link.as_deref(), Some("https://atom.example.com/entry"));
    }

    #[test]
    fn extracts_media_thumbnail() {
        let xml = r#"
<rss version="2.0" xmlns:media="http://search.yahoo.com/mrss/">
  <channel><title>缩略图订阅</title><link>https://feed.example.com</link>
    <item><title>带图文章</title><link>https://feed.example.com/1</link>
      <media:thumbnail url="https://cdn.example.com/pic.webp" width="640" height="360"/>
    </item>
  </channel>
</rss>
"#;
        let feed = parse(xml, None).unwrap();
        assert_eq!(
            feed.items[0].thumbnail_url.as_deref(),
            Some("https://cdn.example.com/pic.webp")
        );
    }
}
