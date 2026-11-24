//! OPML import/export, ported from OpmlService.cs.

use crate::data::model::{Feed, FeedGroup};
use crate::data::xml::{self, XmlNode};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpmlSubscription {
    pub group_name: String,
    pub title: String,
    pub xml_url: String,
    pub html_url: String,
}

pub fn import(xml: &str) -> Result<Vec<OpmlSubscription>, String> {
    let doc = xml::parse(xml)?;
    let body = doc.find("body").ok_or_else(|| "OPML 缺少 body 节点。".to_string())?;

    let mut result = Vec::new();
    for group in body.all("outline") {
        let group_name = group
            .attr("title")
            .or_else(|| group.attr("text"))
            .unwrap_or("默认分组")
            .to_string();
        let children: Vec<&XmlNode> = group.all("outline").collect();
        if children.is_empty() {
            add_if_valid(&mut result, &group_name, group);
        } else {
            for child in children {
                add_if_valid(&mut result, &group_name, child);
            }
        }
    }
    Ok(result)
}

fn add_if_valid(target: &mut Vec<OpmlSubscription>, group_name: &str, node: &XmlNode) {
    let Some(url) = node.attr("xmlUrl").or_else(|| node.attr("xmlurl")) else {
        return;
    };
    if url.trim().is_empty() {
        return;
    }
    let title = node.attr("title").or_else(|| node.attr("text")).unwrap_or(url);
    let html = node.attr("htmlUrl").or_else(|| node.attr("htmlurl")).unwrap_or("");
    target.push(OpmlSubscription {
        group_name: group_name.to_string(),
        title: title.to_string(),
        xml_url: url.to_string(),
        html_url: html.to_string(),
    });
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub fn export(feeds: &[Feed], groups: &[FeedGroup]) -> String {
    let group_map: HashMap<&str, &str> = groups
        .iter()
        .map(|g| (g.id.as_str(), g.name.as_str()))
        .collect();

    let mut grouped: HashMap<&str, Vec<&Feed>> = HashMap::new();
    for feed in feeds {
        grouped
            .entry(feed.group_id.as_deref().unwrap_or(""))
            .or_default()
            .push(feed);
    }

    let mut body = String::from("<body>");
    for (key, group_feeds) in &grouped {
        let name = group_map.get(*key).copied().unwrap_or("默认分组");
        body.push_str(&format!(
            "<outline text=\"{}\" title=\"{}\">",
            escape(name),
            escape(name)
        ));
        for feed in group_feeds {
            let website = feed.website.as_deref().unwrap_or(&feed.url);
            body.push_str(&format!(
                "<outline type=\"rss\" text=\"{}\" title=\"{}\" xmlUrl=\"{}\" htmlUrl=\"{}\"/>",
                escape(&feed.title),
                escape(&feed.title),
                escape(&feed.url),
                escape(website)
            ));
        }
        body.push_str("</outline>");
    }
    body.push_str("</body>");

    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?><opml version=\"2.0\"><head><title>dews 订阅导出</title></head>{body}</opml>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_nested_and_flat_outlines() {
        let xml = r#"
<?xml version="1.0"?>
<opml version="2.0"><head><title>x</title></head><body>
<outline text="科技" title="科技">
  <outline type="rss" text="示例" xmlUrl="https://example.com/feed" htmlUrl="https://example.com"/>
</outline>
<outline type="rss" text="独立订阅" xmlUrl="https://solo.example.com/rss"/>
</body></opml>
"#;
        let subs = import(xml).unwrap();
        assert_eq!(subs.len(), 2);
        assert_eq!(subs[0].group_name, "科技");
        assert_eq!(subs[0].xml_url, "https://example.com/feed");
        // Flat outlines use their own text/title as the group name (matches C#).
        assert_eq!(subs[1].group_name, "独立订阅");
    }

    #[test]
    fn export_roundtrips() {
        let feeds = vec![Feed {
            id: "f1".to_string(),
            title: "示例".to_string(),
            url: "https://example.com/feed".to_string(),
            website: Some("https://example.com".to_string()),
            group_id: Some("g1".to_string()),
            ..Feed::default()
        }];
        let groups = vec![FeedGroup {
            id: "g1".to_string(),
            name: "科技".to_string(),
            account_id: 1,
        }];
        let exported = export(&feeds, &groups);
        assert!(exported.contains("xmlUrl=\"https://example.com/feed\""));
        assert_eq!(import(&exported).unwrap().len(), 1);
    }
}
