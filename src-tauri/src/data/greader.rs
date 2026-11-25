//! Google Reader API compatible client, ported from GoogleReaderClient.cs.
//! FreshRSS uses the same protocol at /api/greader.php with /reader/api/0/ routes.
//! Fever is deliberately not implemented.

use crate::data::model::{Account, AccountKind};
use crate::data::store::SecretProtector;
use crate::data::thumbnail;
use chrono::{DateTime, Utc};
use reqwest::header::AUTHORIZATION;
use serde_json::Value;
use std::time::Duration;

const API_PREFIX: &str = "/reader/api/0";

pub const READ_TAG: &str = "user/-/state/com.google/read";
pub const STARRED_TAG: &str = "user/-/state/com.google/starred";

#[derive(Debug, Clone)]
pub struct GReaderSubscription {
    pub stream_id: String,
    pub title: String,
    pub website: String,
    pub category_label: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GReaderItem {
    pub id: String,
    pub title: String,
    pub link: Option<String>,
    pub published: i64,
    pub summary_html: Option<String>,
    pub author: Option<String>,
    pub feed_stream_id: String,
    pub feed_title: Option<String>,
    pub is_read: bool,
    pub is_starred: bool,
    pub thumbnail_url: Option<String>,
}

pub struct GoogleReaderClient {
    account: Account,
    http: reqwest::Client,
    auth: Option<String>,
    token: Option<String>,
}

impl GoogleReaderClient {
    pub fn new(account: Account) -> Result<Self, String> {
        Self::with_timeout(account, Duration::from_secs(40))
    }

    /// Short-timeout variant for interactive marks: a click must never hang
    /// the UI for the full sync timeout when the remote server is unreachable.
    pub fn with_timeout(account: Account, timeout: Duration) -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .gzip(true)
            .deflate(true)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            account,
            http,
            auth: None,
            token: None,
        })
    }

    /// FreshRSS endpoints end in /api/greader.php; other services use the URL as-is.
    pub fn api_root(&self) -> Result<String, String> {
        let mut endpoint = self.account.endpoint.trim().trim_end_matches('/').to_string();
        if endpoint.is_empty() {
            return Err("请先在账户设置中填写 API 地址。".to_string());
        }
        if self.account.kind == AccountKind::FreshRSS
            && !endpoint.to_ascii_lowercase().ends_with("/api/greader.php")
        {
            endpoint.push_str("/api/greader.php");
        }
        Ok(endpoint)
    }

    fn api(&self, relative: &str) -> Result<String, String> {
        Ok(format!("{}{}{}", self.api_root()?, API_PREFIX, relative))
    }

    async fn ensure_auth(&mut self) -> Result<(), String> {
        if self.auth.is_some() {
            return Ok(());
        }
        let password = SecretProtector::unprotect(Some(&self.account.password_protected));
        let form = [
            ("Email", self.account.username.as_str()),
            ("Passwd", password.as_str()),
            ("service", "reader"),
            ("accountType", "GOOGLE"),
        ];
        let url = format!("{}/accounts/ClientLogin", self.api_root()?);
        let response = self
            .http
            .post(&url)
            .form(&form)
            .send()
            .await
            .map_err(|e| format!("登录请求失败：{e}"))?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(format!(
                "登录失败 ({})：{}",
                status.as_u16(),
                shorten(&body)
            ));
        }
        let auth = body
            .lines()
            .find(|l| l.to_ascii_lowercase().starts_with("auth="))
            .map(|l| l["Auth=".len()..].trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                "未从服务器获得 Auth 令牌。FreshRSS 需在设置中启用 API 并使用 API 密码而非网页登录密码。".to_string()
            })?;
        // FreshRSS parses this header with parse_str(); a bare "GoogleLogin <token>" is rejected.
        self.auth = Some(format!("GoogleLogin auth={auth}"));
        self.token = None;
        Ok(())
    }

    fn apply_auth(&self, builder: reqwest::RequestBuilder) -> Result<reqwest::RequestBuilder, String> {
        let auth = self.auth.as_ref().ok_or_else(|| "尚未登录。".to_string())?;
        Ok(builder.header(AUTHORIZATION, auth.as_str()))
    }

    async fn get_token(&mut self) -> Result<String, String> {
        self.ensure_auth().await?;
        if let Some(t) = &self.token {
            return Ok(t.clone());
        }
        let url = self.api("/token")?;
        let response = self
            .apply_auth(self.http.get(&url))?
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let body = read_body(response, Some(&url)).await?;
        let token = body.trim().to_string();
        self.token = Some(token.clone());
        Ok(token)
    }

    pub async fn validate(&mut self) -> Result<(), String> {
        self.ensure_auth().await?;
        let url = self.api("/subscription/list?output=json")?;
        let response = self
            .apply_auth(self.http.get(&url))?
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(format!(
                "验证失败 ({status})：{}。请确认账户类型为 FreshRSS、API 地址正确且已启用 API 访问。",
                shorten(&body)
            ));
        }
        Ok(())
    }

    pub async fn get_subscriptions(&mut self) -> Result<Vec<GReaderSubscription>, String> {
        self.ensure_auth().await?;
        let url = self.api("/subscription/list?output=json")?;
        let response = self
            .apply_auth(self.http.get(&url))?
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let body = read_body(response, Some(&url)).await?;
        let doc: Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
        let mut result = Vec::new();
        if let Some(subs) = doc.get("subscriptions").and_then(Value::as_array) {
            for item in subs {
                let Some(id) = json_str(item, "id") else { continue };
                if id.is_empty() {
                    continue;
                }
                let category = item
                    .get("categories")
                    .and_then(Value::as_array)
                    .and_then(|c| c.first())
                    .and_then(|c| json_str(c, "label").or_else(|| json_str(c, "id")))
                    .map(str::to_string);
                result.push(GReaderSubscription {
                    stream_id: id.to_string(),
                    title: json_str(item, "title").unwrap_or(id).to_string(),
                    website: json_str(item, "website").unwrap_or("").to_string(),
                    category_label: category,
                });
            }
        }
        Ok(result)
    }

    /// Fetch one page of a stream. Prefer `get_stream_all` for full synchronization.
    pub async fn get_stream(&mut self, stream_id: &str, count: i32) -> Result<Vec<GReaderItem>, String> {
        let (items, _) = self.get_stream_page(stream_id, count, None).await?;
        Ok(items)
    }

    /// Fetch all items of a stream, following the GReader `continuation` cursor.
    /// Deduplicates by item id in case a server returns overlapping pages.
    pub async fn get_stream_all(
        &mut self,
        stream_id: &str,
        max_items: usize,
    ) -> Result<Vec<GReaderItem>, String> {
        let mut all = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut continuation: Option<String> = None;
        loop {
            if all.len() >= max_items {
                break;
            }
            let (items, next) = self
                .get_stream_page(stream_id, 500, continuation.as_deref())
                .await?;
            if items.is_empty() {
                break;
            }
            let mut fresh = 0;
            for item in items {
                if seen.insert(item.id.clone()) {
                    all.push(item);
                    fresh += 1;
                    if all.len() >= max_items {
                        break;
                    }
                }
            }
            if fresh == 0 {
                break;
            }
            match next {
                Some(cursor)
                    if !cursor.is_empty() && Some(cursor.as_str()) != continuation.as_deref() =>
                {
                    continuation = Some(cursor);
                }
                _ => break,
            }
        }
        Ok(all)
    }

    async fn get_stream_page(
        &mut self,
        stream_id: &str,
        count: i32,
        continuation: Option<&str>,
    ) -> Result<(Vec<GReaderItem>, Option<String>), String> {
        self.ensure_auth().await?;
        // Keep '/' as separator; encode each segment (feed/123 -> feed/123).
        let encoded = stream_id
            .split('/')
            .map(|seg| {
                url::form_urlencoded::byte_serialize(seg.as_bytes()).collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("/");
        let n = count.clamp(1, 500);
        let mut url = format!(
            "{}?n={n}&output=json",
            self.api(&format!("/stream/contents/{encoded}"))?
        );
        if let Some(cursor) = continuation {
            let encoded_cursor: String =
                url::form_urlencoded::byte_serialize(cursor.as_bytes()).collect();
            url.push_str(&format!("&c={encoded_cursor}"));
        }
        let response = self
            .apply_auth(self.http.get(&url))?
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let body = read_body(response, Some(&url)).await?;
        let doc: Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
        let mut result = Vec::new();
        if let Some(items) = doc.get("items").and_then(Value::as_array) {
            for item in items {
                let Some(id) = json_str(item, "id") else { continue };
                if id.is_empty() {
                    continue;
                }
                let link = item
                    .get("alternate")
                    .and_then(Value::as_array)
                    .and_then(|a| a.first())
                    .and_then(|a| json_str(a, "href"))
                    .unwrap_or("");
                let published = json_i64(item, "published")
                    .or_else(|| json_i64(item, "updated"))
                    .unwrap_or_else(|| Utc::now().timestamp());
                let summary = nested_string(item, "summary").or_else(|| nested_string(item, "content"));
                let origin = item.get("origin");
                let (read, starred) = item_read_starred(item);
                let feed_stream_id = origin
                    .and_then(|o| json_str(o, "streamId"))
                    .unwrap_or("")
                    .to_string();
                let feed_title = origin.and_then(|o| json_str(o, "title")).map(str::to_string);
                let base = origin
                    .and_then(|o| json_str(o, "htmlUrl"))
                    .unwrap_or(&self.account.endpoint);
                result.push(GReaderItem {
                    id: id.to_string(),
                    title: json_str(item, "title").unwrap_or("(无标题)").to_string(),
                    link: if link.is_empty() { None } else { Some(link.to_string()) },
                    published: DateTime::from_timestamp(published, 0)
                        .map(|d| d.timestamp())
                        .unwrap_or(published),
                    summary_html: summary,
                    author: json_str(item, "author").map(str::to_string),
                    feed_stream_id,
                    feed_title,
                    is_read: read,
                    is_starred: starred,
                    thumbnail_url: thumbnail::from_greader_item(item, Some(base)),
                });
            }
        }
        let continuation = doc
            .get("continuation")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        Ok((result, continuation))
    }

    pub async fn mark(&mut self, item_id: &str, add_tag: Option<&str>, remove_tag: Option<&str>) -> Result<(), String> {
        let token = self.get_token().await?;
        let mut fields = vec![("i", item_id.to_string()), ("T", token)];
        if let Some(tag) = add_tag {
            fields.push(("a", tag.to_string()));
        }
        if let Some(tag) = remove_tag {
            fields.push(("r", tag.to_string()));
        }
        // FreshRSS uses "edit-tag", not "editTag".
        let url = self.api("/edit-tag")?;
        let response = self
            .apply_auth(self.http.post(&url).form(&fields))?
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("标记状态失败 ({status})：{}", shorten(&body)));
        }
        Ok(())
    }

    /// Mark many items in one request. The Reader API accepts repeated `i`
    /// fields, so flushing a page of queued marks costs one round-trip instead
    /// of one per article. Chunked to keep proxies happy.
    pub async fn mark_many(
        &mut self,
        item_ids: &[String],
        add_tag: Option<&str>,
        remove_tag: Option<&str>,
    ) -> Result<(), String> {
        if item_ids.is_empty() {
            return Ok(());
        }
        let token = self.get_token().await?;
        let url = self.api("/edit-tag")?;
        for chunk in item_ids.chunks(200) {
            let body = {
                let mut form = url::form_urlencoded::Serializer::new(String::new());
                for id in chunk {
                    form.append_pair("i", id);
                }
                form.append_pair("T", &token);
                if let Some(tag) = add_tag {
                    form.append_pair("a", tag);
                }
                if let Some(tag) = remove_tag {
                    form.append_pair("r", tag);
                }
                form.finish()
            };
            let response = self
                .apply_auth(
                    self.http
                        .post(&url)
                        .header("Content-Type", "application/x-www-form-urlencoded")
                        .body(body),
                )?
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if !response.status().is_success() {
                let status = response.status().as_u16();
                let body = response.text().await.unwrap_or_default();
                return Err(format!("批量标记状态失败 ({status})：{}", shorten(&body)));
            }
        }
        Ok(())
    }

    pub async fn subscribe(&mut self, feed_url: &str) -> Result<Option<String>, String> {
        self.ensure_auth().await?;
        let encoded: String = url::form_urlencoded::byte_serialize(feed_url.as_bytes()).collect();
        let url = format!("{}?quickadd={encoded}", self.api("/subscription/quickadd")?);
        let response = self
            .apply_auth(self.http.get(&url))?
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let body = read_body(response, Some(&url)).await?;
        let doc: Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
        Ok(json_str(&doc, "streamId").map(str::to_string))
    }

    pub async fn unsubscribe(&mut self, stream_id: &str) -> Result<(), String> {
        let token = self.get_token().await?;
        let fields = [
            ("s", stream_id.to_string()),
            ("ac", "unsubscribe".to_string()),
            ("T", token),
        ];
        // FreshRSS has no /subscription/unsubscribe.
        let url = self.api("/subscription/edit")?;
        let response = self
            .apply_auth(self.http.post(&url).form(&fields))?
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("退订失败 ({status})：{}", shorten(&body)));
        }
        Ok(())
    }
}

/// Read/starred state from a GReader item. FreshRSS returns categories as an
/// array of plain strings (`"user/-/state/com.google/read"`); some services
/// wrap them as objects (`{"id": "..."}`). Both are accepted.
fn item_read_starred(item: &Value) -> (bool, bool) {
    let mut read = false;
    let mut starred = false;
    if let Some(categories) = item.get("categories").and_then(Value::as_array) {
        for category in categories {
            let tag = category.as_str().or_else(|| json_str(category, "id"));
            if let Some(tag) = tag {
                let lower = tag.to_ascii_lowercase();
                if lower.ends_with("/state/com.google/read") {
                    read = true;
                }
                if lower.ends_with("/state/com.google/starred") {
                    starred = true;
                }
            }
        }
    }
    (read, starred)
}

fn json_str<'a>(value: &'a Value, name: &str) -> Option<&'a str> {
    value.get(name).and_then(Value::as_str)
}

fn json_i64(value: &Value, name: &str) -> Option<i64> {
    value.get(name).and_then(Value::as_i64)
}

fn nested_string(value: &Value, name: &str) -> Option<String> {
    let nested = value.get(name)?;
    match nested {
        Value::String(s) => Some(s.clone()),
        _ => json_str(nested, "content").map(str::to_string),
    }
}

async fn read_body(response: reqwest::Response, url: Option<&str>) -> Result<String, String> {
    let status = response.status().as_u16();
    let body = response.text().await.unwrap_or_default();
    if status < 200 || status >= 300 {
        return Err(format!(
            "服务返回 {status}：{}{}",
            shorten(&body),
            url.map(|u| format!(" [{u}]")).unwrap_or_default()
        ));
    }
    Ok(body)
}

fn shorten(text: &str) -> String {
    let flat = text.replace('\r', " ").replace('\n', " ").trim().to_string();
    let chars: Vec<char> = flat.chars().collect();
    if chars.len() > 300 {
        format!("{}…", chars[..300].iter().collect::<String>())
    } else {
        flat
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(endpoint: &str) -> Account {
        Account {
            id: 99,
            kind: AccountKind::FreshRSS,
            name: "协议测试".to_string(),
            endpoint: endpoint.to_string(),
            username: "tester".to_string(),
            password_protected: SecretProtector::protect(Some("api-pass")),
            is_active: true,
        }
    }

    #[test]
    fn api_root_appends_freshrss_path() {
        let client = GoogleReaderClient::new(account("http://127.0.0.1:18765")).unwrap();
        assert_eq!(client.api_root().unwrap(), "http://127.0.0.1:18765/api/greader.php");
        assert_eq!(
            client.api("/subscription/list").unwrap(),
            "http://127.0.0.1:18765/api/greader.php/reader/api/0/subscription/list"
        );
    }

    #[test]
    fn api_root_preserves_explicit_endpoint() {
        let client = GoogleReaderClient::new(account(
            "https://reader.example.com/api/greader.php/",
        ))
        .unwrap();
        assert_eq!(client.api_root().unwrap(), "https://reader.example.com/api/greader.php");
    }

    #[test]
    fn tags_match_csharp() {
        assert_eq!(READ_TAG, "user/-/state/com.google/read");
        assert_eq!(STARRED_TAG, "user/-/state/com.google/starred");
    }

    #[test]
    fn read_state_from_string_categories() {
        // The exact shape FreshRSS returns at /stream/contents.
        let item: Value = serde_json::from_str(
            r#"{"id":"x","categories":["user/-/state/com.google/reading-list","user/-/label/0.Priority","user/-/state/com.google/read","user/-/state/com.google/starred"]}"#,
        )
        .unwrap();
        assert_eq!(item_read_starred(&item), (true, true));
    }

    #[test]
    fn read_state_from_object_categories() {
        let item: Value = serde_json::from_str(
            r#"{"id":"x","categories":[{"id":"user/1/state/com.google/read"}]}"#,
        )
        .unwrap();
        assert_eq!(item_read_starred(&item), (true, false));
    }

    #[test]
    fn unread_item_has_no_read_state() {
        let item: Value = serde_json::from_str(
            r#"{"id":"x","categories":["user/-/state/com.google/reading-list","user/-/label/0.Priority"]}"#,
        )
        .unwrap();
        assert_eq!(item_read_starred(&item), (false, false));
    }
}
