//! Domain models. AccountKind intentionally omits Fever (upstream id 2).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AccountKind {
    #[default]
    Local = 1,
    GoogleReader = 3,
    FreshRSS = 4,
}

impl AccountKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AccountKind::Local => "Local",
            AccountKind::GoogleReader => "GoogleReader",
            AccountKind::FreshRSS => "FreshRSS",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: i32,
    pub kind: AccountKind,
    pub name: String,
    pub endpoint: String,
    pub username: String,
    /// DPAPI-protected password, base64 encoded.
    pub password_protected: String,
    pub is_active: bool,
}

impl Default for Account {
    fn default() -> Self {
        Self {
            id: 0,
            kind: AccountKind::Local,
            name: "本地账户".to_string(),
            endpoint: String::new(),
            username: String::new(),
            password_protected: String::new(),
            is_active: false,
        }
    }
}

impl Account {
    pub fn display_name(&self) -> &str {
        if self.name.trim().is_empty() {
            self.kind.as_str()
        } else {
            &self.name
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedGroup {
    pub id: String,
    pub account_id: i32,
    pub name: String,
}

impl Default for FeedGroup {
    fn default() -> Self {
        Self {
            id: "1$default".to_string(),
            account_id: 1,
            name: "默认分组".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feed {
    pub id: String,
    pub account_id: i32,
    pub remote_id: String,
    pub title: String,
    pub url: String,
    pub website: Option<String>,
    pub group_id: Option<String>,
    pub last_synced: Option<String>,
    pub failure_count: i32,
}

impl Default for Feed {
    fn default() -> Self {
        Self {
            id: new_id(),
            account_id: 0,
            remote_id: String::new(),
            title: String::new(),
            url: String::new(),
            website: None,
            group_id: None,
            last_synced: None,
            failure_count: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Article {
    pub id: String,
    pub account_id: i32,
    pub feed_id: String,
    pub remote_id: String,
    pub title: String,
    pub author: Option<String>,
    pub link: Option<String>,
    /// Unix seconds (serialized deterministically, no timezone ambiguity).
    pub published: i64,
    pub summary_html: Option<String>,
    /// Cleaned full article HTML for CSS rendering.
    pub content_html: Option<String>,
    /// Plain text for search/fallback.
    pub content_text: Option<String>,
    pub is_read: bool,
    pub is_starred: bool,
    pub is_archived: bool,
    pub snippet: Option<String>,
    pub thumbnail_url: Option<String>,
}

impl Default for Article {
    fn default() -> Self {
        Self {
            id: new_id(),
            account_id: 0,
            feed_id: String::new(),
            remote_id: String::new(),
            title: String::new(),
            author: None,
            link: None,
            published: 0,
            summary_html: None,
            content_html: None,
            content_text: None,
            is_read: false,
            is_starred: false,
            is_archived: false,
            snippet: None,
            thumbnail_url: None,
        }
    }
}

impl Article {
    pub fn has_thumbnail(&self) -> bool {
        self.thumbnail_url
            .as_ref()
            .map(|u| !u.trim().is_empty())
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub dark_theme: bool,
    pub font_size: i32,
    pub feed_font_family: String,
    pub feed_font_size: f64,
    pub article_font_family: String,
    pub article_font_size: f64,
    pub reading_font_family: String,
    pub summary_lines: i32,
    /// 0 = off, 1 = small, 2 = medium, 3 = large.
    pub thumbnail_size: i32,
    pub mark_as_read_on_open: bool,
    pub mark_as_read_when_next: bool,
    pub sync_interval_minutes: i32,
    pub filter: String,
    pub feed_column_width: f64,
    pub article_column_width: f64,
    /// Reading pane: horizontal padding on both sides (px).
    #[serde(default = "default_reading_margin")]
    pub reading_margin: i32,
    /// Reading pane: line height multiplier (e.g. 1.85).
    #[serde(default = "default_reading_line_height")]
    pub reading_line_height: f64,
    /// Reading pane: first-line indent in Chinese characters (0 = none).
    #[serde(default = "default_reading_indent")]
    pub reading_indent: i32,
}

fn default_reading_margin() -> i32 { 34 }
fn default_reading_line_height() -> f64 { 1.85 }
fn default_reading_indent() -> i32 { 0 }

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            dark_theme: false,
            font_size: 15,
            feed_font_family: "Microsoft YaHei".to_string(),
            feed_font_size: 13.0,
            article_font_family: "Microsoft YaHei".to_string(),
            article_font_size: 13.0,
            reading_font_family: "Microsoft YaHei".to_string(),
            summary_lines: 2,
            thumbnail_size: 1,
            mark_as_read_on_open: true,
            mark_as_read_when_next: true,
            sync_interval_minutes: 15,
            filter: "all".to_string(),
            feed_column_width: 266.0,
            article_column_width: 404.0,
            reading_margin: 34,
            reading_line_height: 1.85,
            reading_indent: 0,
        }
    }
}

/// A local read/star change that could not yet be pushed to a remote account.
/// It is retried on the next sync; meanwhile the local flag stays authoritative
/// for that article so a server refresh cannot silently undo the user's click.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingMark {
    pub article_id: String,
    pub read: Option<bool>,
    pub starred: Option<bool>,
    /// Milliseconds since epoch; used to avoid dropping a newer click while a
    /// previous push is still in flight.
    pub updated_at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppData {
    pub accounts: Vec<Account>,
    pub groups: Vec<FeedGroup>,
    pub feeds: Vec<Feed>,
    pub articles: Vec<Article>,
    pub settings: AppSettings,
    #[serde(default)]
    pub pending_marks: Vec<PendingMark>,
}

/// 32 hex chars, equivalent to C# Guid.NewGuid().ToString("N").
pub fn new_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seed = nanos ^ ((std::process::id() as u128) << 64);
    let mut x = seed as u64 ^ 0x9E3779B97F4A7C15;
    let mut out = String::with_capacity(32);
    for _ in 0..4 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        out.push_str(&format!("{:016x}", x));
    }
    out.truncate(32);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_wpf_edition() {
        let s = AppSettings::default();
        assert_eq!(s.summary_lines, 2);
        assert_eq!(s.thumbnail_size, 1);
        assert_eq!(s.font_size, 15);
        assert!(s.mark_as_read_on_open && s.mark_as_read_when_next);
    }

    #[test]
    fn fever_is_absent() {
        // Only three account kinds exist; there is no numeric 2 variant.
        assert_eq!(AccountKind::Local as i32, 1);
        assert_eq!(AccountKind::GoogleReader as i32, 3);
        assert_eq!(AccountKind::FreshRSS as i32, 4);
    }

    #[test]
    fn new_id_is_32_hex_chars() {
        let id = new_id();
        assert_eq!(id.len(), 32);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
