//! Tauri commands bridging the React UI to the ported data layer.

use crate::data::article_html;
use crate::data::fulltext;
use crate::data::fonts;
use crate::data::greader::{self, GReaderItem, GoogleReaderClient};
use crate::data::html;
use crate::data::images;
use crate::data::model::{
    Account, AccountKind, AppData, AppSettings, Article, Feed, new_id,
};
use crate::data::opml;
use crate::data::rss::{self, ParsedFeed};
use crate::data::store::{JsonAppStore, SecretProtector};
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, MutexGuard};
use tauri::State;

pub struct AppState {
    pub store: JsonAppStore,
    pub data: Mutex<AppData>,
}

impl AppState {
    pub fn new() -> Self {
        let store = JsonAppStore::new(None);
        let data = store.load();
        Self {
            store,
            data: Mutex::new(data),
        }
    }

    fn lock(&self) -> Result<MutexGuard<'_, AppData>, String> {
        self.data.lock().map_err(|e| format!("数据访问失败：{e}"))
    }

    fn persist(&self, data: &AppData) -> Result<(), String> {
        self.store.save(data)
    }
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct SyncReport {
    pub feeds: usize,
    pub articles: usize,
    pub errors: Vec<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncFeedInfo {
    pub id: String,
    pub title: String,
    pub account_name: String,
}

/// Result of phase 1. The UI refreshes immediately after receiving it.
#[derive(Debug, Clone, Serialize)]
pub struct SyncPlan {
    pub feeds: Vec<SyncFeedInfo>,
    pub errors: Vec<String>,
    pub message: String,
}

/// Result of synchronizing one feed (phase 2 step).
#[derive(Debug, Clone, Serialize)]
pub struct FeedSyncResult {
    pub feed_id: String,
    pub title: String,
    pub articles: usize,
    pub total: usize,
    pub error: Option<String>,
}

/// Lightweight article row sent to the UI. Article bodies stay in Rust:
/// the reading pane is rendered here, and shipping 100+ MB over IPC on every
/// refresh was the main source of UI stalls during sync.
#[derive(Debug, Clone, Serialize)]
pub struct ArticleView {
    id: String,
    account_id: i32,
    feed_id: String,
    remote_id: String,
    title: String,
    author: Option<String>,
    link: Option<String>,
    published: i64,
    is_read: bool,
    is_starred: bool,
    is_archived: bool,
    snippet: Option<String>,
    thumbnail_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppDataView {
    accounts: Vec<Account>,
    groups: Vec<crate::data::model::FeedGroup>,
    feeds: Vec<crate::data::model::Feed>,
    articles: Vec<ArticleView>,
    settings: AppSettings,
}

#[derive(Debug, Deserialize)]
pub struct AccountInput {
    pub id: i32,
    pub kind: AccountKind,
    pub name: String,
    pub endpoint: String,
    pub username: String,
    /// Empty means "keep the previously stored password".
    pub password: String,
    pub is_active: bool,
}

const EXTERNAL_LINK_SCRIPT: &str = r#"<script>
document.addEventListener('click', function (e) {
  var a = e.target && e.target.closest ? e.target.closest('a[href]') : null;
  if (!a) return;
  var href = a.getAttribute('href');
  if (!href || href.charAt(0) === '#') return;
  e.preventDefault();
  window.parent.postMessage({ __readyou_open: href }, '*');
});
</script>"#;

// ---------------------------------------------------------------------------
// Bootstrap / settings / fonts
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn bootstrap(state: State<'_, AppState>) -> Result<AppDataView, String> {
    let data = state.lock()?;
    let articles = data
        .articles
        .iter()
        .map(|a| {
            let snippet = if let Some(s) = a.snippet.as_deref().filter(|s| !s.trim().is_empty()) {
                Some(s.to_string())
            } else if let Some(t) = a.content_text.as_deref().filter(|t| !t.trim().is_empty()) {
                Some(t.chars().take(240).collect())
            } else {
                let text = html::to_text(a.summary_html.as_deref(), 240);
                if text.is_empty() { None } else { Some(text) }
            };
            ArticleView {
                id: a.id.clone(),
                account_id: a.account_id,
                feed_id: a.feed_id.clone(),
                remote_id: a.remote_id.clone(),
                title: a.title.clone(),
                author: a.author.clone(),
                link: a.link.clone(),
                published: a.published,
                is_read: a.is_read,
                is_starred: a.is_starred,
                is_archived: a.is_archived,
                snippet,
                thumbnail_url: a.thumbnail_url.clone(),
            }
        })
        .collect();
    Ok(AppDataView {
        accounts: data.accounts.clone(),
        groups: data.groups.clone(),
        feeds: data.feeds.clone(),
        articles,
        settings: data.settings.clone(),
    })
}

/// Full-text search runs in Rust so the UI never has to receive article bodies.
#[tauri::command]
pub fn search_articles(query: String, state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let term = query.trim().to_ascii_lowercase();
    if term.is_empty() {
        return Ok(Vec::new());
    }
    let data = state.lock()?;
    Ok(data
        .articles
        .iter()
        .filter(|a| {
            a.title.to_ascii_lowercase().contains(&term)
                || a
                    .snippet
                    .as_deref()
                    .is_some_and(|s| s.to_ascii_lowercase().contains(&term))
                || a
                    .content_text
                    .as_deref()
                    .is_some_and(|t| t.to_ascii_lowercase().contains(&term))
        })
        .map(|a| a.id.clone())
        .collect())
}

#[tauri::command]
pub fn save_settings(settings: AppSettings, state: State<'_, AppState>) -> Result<(), String> {
    let mut data = state.lock()?;
    data.settings = settings;
    state.persist(&data)
}

#[tauri::command]
pub fn get_fonts() -> Vec<String> {
    fonts::system_fonts()
}

#[tauri::command]
pub fn is_portable() -> bool {
    crate::data::store::JsonAppStore::is_portable()
}

// ---------------------------------------------------------------------------
// Reading pane
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_article_html(id: String, state: State<'_, AppState>) -> Result<String, String> {
    let data = state.lock()?;
    let article = data
        .articles
        .iter()
        .find(|a| a.id == id)
        .ok_or_else(|| "找不到这篇文章".to_string())?;
    let source = article
        .content_html
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| article.summary_html.as_deref().filter(|s| !s.trim().is_empty()));
    let base = article.link.as_deref().filter(|s| !s.trim().is_empty());
    let settings = &data.settings;
    let mut page = article_html::render(
        source,
        base,
        settings.dark_theme,
        settings.font_size,
        Some(&settings.reading_font_family),
    );
    if page.contains("</body>") {
        page = page.replacen("</body>", &format!("{EXTERNAL_LINK_SCRIPT}</body>"), 1);
    } else {
        page.push_str(EXTERNAL_LINK_SCRIPT);
    }
    Ok(page)
}

/// Fetch the original web page for an article and extract its full text.
/// The result replaces `content_html` and is rendered with the reading-pane
/// CSS so the UI can show it immediately. Only http(s) links are followed.
#[tauri::command]
pub async fn fetch_full_text(id: String, state: State<'_, AppState>) -> Result<String, String> {
    // Snapshot everything needed under the lock, then release before awaiting.
    let (link, settings_dark, font_size, reading_font) = {
        let data = state.lock()?;
        let article = data
            .articles
            .iter()
            .find(|a| a.id == id)
            .ok_or_else(|| "找不到这篇文章".to_string())?;
        let link = article
            .link
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "这篇文章没有原文链接，无法获取全文".to_string())?
            .to_string();
        if !(link.to_ascii_lowercase().starts_with("http://")
            || link.to_ascii_lowercase().starts_with("https://"))
        {
            return Err("原文链接不是有效的 http(s) 地址".to_string());
        }
        let s = &data.settings;
        (link, s.dark_theme, s.font_size, s.reading_font_family.clone())
    };

    let client = http_client()?;
    // Some sites require a Referer even for the article page itself; use its
    // site root rather than a self-referential URL.
    let page_referer = reqwest::Url::parse(&link)
        .ok()
        .map(|u| format!("{}://{}", u.scheme(), u.host_str().unwrap_or("")))
        .unwrap_or_else(|| link.clone());
    let response = client
        .get(&link)
        .header(reqwest::header::REFERER, &page_referer)
        .header(
            reqwest::header::ACCEPT_LANGUAGE,
            "zh-CN,zh;q=0.9,en;q=0.8",
        )
        .send()
        .await
        .map_err(|e| format!("获取原文失败：{e}"))?;
    if !response.status().is_success() {
        return Err(format!("获取原文失败：服务器返回 {}", response.status().as_u16()));
    }
    let charset = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("读取原文失败：{e}"))?;
    let page = fulltext::decode_html(&bytes, charset.as_deref());
    let extracted = fulltext::extract(&page);
    if extracted.trim().is_empty() {
        return Err("未能从原文中提取到正文（页面可能需要登录或由脚本渲染）".to_string());
    }
    // Download in-article images with the article URL as Referer, then inline
    // them so the reading pane can show them without hotlink checks.
    let body = images::embed_images(&client, &link, &extracted).await;
    let text = html::to_text(Some(&body), 400_000);

    // Persist the extracted body so subsequent reads don't refetch.
    {
        let mut data = state.lock()?;
        if let Some(article) = data.articles.iter_mut().find(|a| a.id == id) {
            article.content_html = Some(body.clone());
            article.content_text = Some(text);
        }
        state.persist(&data)?;
    }

    let mut page_html = article_html::render(
        Some(&body),
        Some(&link),
        settings_dark,
        font_size,
        Some(&reading_font),
    );
    if page_html.contains("</body>") {
        page_html = page_html.replacen("</body>", &format!("{EXTERNAL_LINK_SCRIPT}</body>"), 1);
    } else {
        page_html.push_str(EXTERNAL_LINK_SCRIPT);
    }
    Ok(page_html)
}

#[tauri::command]
pub async fn mark_article(
    id: String,
    read: Option<bool>,
    starred: Option<bool>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let (account, remote_id) = {
        let mut data = state.lock()?;
        // Copy scalar fields first so the mutable borrow of `article` ends
        // before we borrow `data.accounts` immutably below.
        let (account_id, remote_id) = {
            let article = data
                .articles
                .iter_mut()
                .find(|a| a.id == id)
                .ok_or_else(|| "找不到这篇文章".to_string())?;
            if let Some(value) = read {
                article.is_read = value;
            }
            if let Some(value) = starred {
                article.is_starred = value;
            }
            (article.account_id, article.remote_id.clone())
        };
        let account = data.accounts.iter().find(|a| a.id == account_id).cloned();
        state.persist(&data)?;
        (account, remote_id)
    };

    if let Some(account) = account {
        if account.kind != AccountKind::Local && !remote_id.is_empty() {
            // Push before returning so the next sync can safely trust the
            // server's read/starred flags instead of stale local copies.
            let mut client = GoogleReaderClient::new(account)?;
            if let Some(value) = read {
                client
                    .mark(
                        &remote_id,
                        value.then_some(greader::READ_TAG),
                        (!value).then_some(greader::READ_TAG),
                    )
                    .await
                    .map_err(|e| format!("同步已读状态失败：{e}"))?;
            }
            if let Some(value) = starred {
                client
                    .mark(
                        &remote_id,
                        value.then_some(greader::STARRED_TAG),
                        (!value).then_some(greader::STARRED_TAG),
                    )
                    .await
                    .map_err(|e| format!("同步星标状态失败：{e}"))?;
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Feeds / synchronization
// ---------------------------------------------------------------------------

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("dews/0.1 (+desktop RSS reader)")
        .timeout(std::time::Duration::from_secs(30))
        .gzip(true)
        .deflate(true)
        .build()
        .map_err(|e| e.to_string())
}

async fn fetch_rss(client: &reqwest::Client, url: &str) -> Result<ParsedFeed, String> {
    let body = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("请求订阅源失败：{e}"))?
        .error_for_status()
        .map_err(|e| format!("订阅源返回错误：{e}"))?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    rss::parse(&body, Some(url))
}

fn article_from_parsed(
    account_id: i32,
    feed: &Feed,
    item: &rss::ParsedArticle,
) -> Article {
    let text = html::to_text(item.summary_html.as_deref(), 200_000);
    let id = format!("local{}:{}", feed.id, item.id);
    Article {
        id,
        account_id,
        feed_id: feed.id.clone(),
        remote_id: item.id.clone(),
        title: item.title.clone(),
        author: item.author.clone(),
        link: item.link.clone(),
        published: item.published,
        summary_html: item.summary_html.clone(),
        content_html: item.summary_html.clone(),
        content_text: Some(text),
        is_read: false,
        is_starred: false,
        is_archived: false,
        snippet: Some(html::snippet(item.summary_html.as_deref(), 240)),
        thumbnail_url: item.thumbnail_url.clone(),
    }
}

fn article_from_greader(account_id: i32, feed_id: &str, item: &GReaderItem) -> Article {
    let id = format!("gr{}:{}", account_id, item.id);
    let text = html::to_text(item.summary_html.as_deref(), 200_000);
    Article {
        id,
        account_id,
        feed_id: feed_id.to_string(),
        remote_id: item.id.clone(),
        title: item.title.clone(),
        author: item.author.clone(),
        link: item.link.clone(),
        published: item.published,
        summary_html: item.summary_html.clone(),
        content_html: item.summary_html.clone(),
        content_text: Some(text),
        is_read: item.is_read,
        is_starred: item.is_starred,
        is_archived: false,
        snippet: Some(html::snippet(item.summary_html.as_deref(), 240)),
        thumbnail_url: item.thumbnail_url.clone(),
    }
}

/// Insert or update while preserving the user's local read/starred/archived state.
fn upsert_article(data: &mut AppData, article: Article) -> bool {
    if let Some(existing) = data.articles.iter_mut().find(|a| a.id == article.id) {
        let (read, starred, archived) =
            (existing.is_read, existing.is_starred, existing.is_archived);
        *existing = article;
        existing.is_read = read;
        existing.is_starred = starred;
        existing.is_archived = archived;
        false
    } else {
        data.articles.push(article);
        true
    }
}

const MAX_ARTICLES_PER_FEED: usize = 5000;

/// Insert or update from the server: the server's read/starred flags win.
/// Local-only archiving is preserved because Google Reader has no archive bit.
fn upsert_article_synced(data: &mut AppData, article: Article) -> bool {
    if let Some(existing) = data.articles.iter_mut().find(|a| a.id == article.id) {
        let archived = existing.is_archived;
        *existing = article;
        existing.is_archived = archived;
        false
    } else {
        data.articles.push(article);
        true
    }
}

/// Phase 1: refresh the subscription tree (groups + feeds) from every active
/// account. The UI refreshes the left pane immediately after this completes.
async fn sync_subscriptions_inner(state: &AppState) -> Result<SyncPlan, String> {
    let accounts: Vec<Account> = state
        .lock()?
        .accounts
        .iter()
        .filter(|a| a.is_active)
        .cloned()
        .collect();
    let mut plan = SyncPlan {
        feeds: Vec::new(),
        errors: Vec::new(),
        message: String::new(),
    };

    for account in accounts {
        if account.kind == AccountKind::Local {
            let local: Vec<SyncFeedInfo> = state
                .lock()?
                .feeds
                .iter()
                .filter(|f| f.account_id == account.id)
                .map(|f| SyncFeedInfo {
                    id: f.id.clone(),
                    title: f.title.clone(),
                    account_name: account.name.clone(),
                })
                .collect();
            plan.feeds.extend(local);
            continue;
        }

        let mut client = match GoogleReaderClient::new(account.clone()) {
            Ok(c) => c,
            Err(e) => {
                plan.errors.push(format!("{}：{e}", account.name));
                continue;
            }
        };
        let subscriptions = match client.get_subscriptions().await {
            Ok(s) => s,
            Err(e) => {
                plan.errors.push(format!("{}：{e}", account.name));
                continue;
            }
        };

        let now = chrono::Utc::now().to_rfc3339();
        {
            let mut data = state.lock()?;
            // Every category reported by the server becomes a group.
            for sub in &subscriptions {
                if let Some(category) =
                    sub.category_label.as_ref().filter(|c| !c.trim().is_empty())
                {
                    let group_id = format!("gr{}:{}", account.id, category);
                    if let Some(group) = data.groups.iter_mut().find(|g| g.id == group_id) {
                        group.name = category.clone();
                        group.account_id = account.id;
                    } else {
                        data.groups.push(crate::data::model::FeedGroup {
                            id: group_id,
                            account_id: account.id,
                            name: category.clone(),
                        });
                    }
                }
            }
            // Update existing feeds in place, including their group assignment.
            for sub in &subscriptions {
                let id = format!("gr{}:{}", account.id, sub.stream_id);
                let group_id = sub
                    .category_label
                    .as_ref()
                    .filter(|c| !c.trim().is_empty())
                    .map(|c| format!("gr{}:{}", account.id, c));
                let website = if sub.website.is_empty() {
                    None
                } else {
                    Some(sub.website.clone())
                };
                if let Some(feed) = data.feeds.iter_mut().find(|f| f.id == id) {
                    feed.title = sub.title.clone();
                    feed.url = sub.stream_id.clone();
                    feed.website = website;
                    feed.group_id = group_id;
                    feed.last_synced = Some(now.clone());
                    feed.failure_count = 0;
                } else {
                    data.feeds.push(Feed {
                        id,
                        account_id: account.id,
                        remote_id: sub.stream_id.clone(),
                        title: sub.title.clone(),
                        url: sub.stream_id.clone(),
                        website,
                        group_id,
                        last_synced: Some(now.clone()),
                        failure_count: 0,
                    });
                }
            }
            // Remove feeds unsubscribed on the server, together with their articles.
            let live_ids: Vec<String> = subscriptions
                .iter()
                .map(|s| format!("gr{}:{}", account.id, s.stream_id))
                .collect();
            let removed: Vec<String> = data
                .feeds
                .iter()
                .filter(|f| f.account_id == account.id && !live_ids.contains(&f.id))
                .map(|f| f.id.clone())
                .collect();
            data.feeds
                .retain(|f| f.account_id != account.id || live_ids.contains(&f.id));
            data.articles.retain(|a| !removed.contains(&a.feed_id));
            // Drop groups left empty by the removal.
            let active_groups: std::collections::HashSet<String> =
                data.feeds.iter().filter_map(|f| f.group_id.clone()).collect();
            data.groups
                .retain(|g| g.account_id != account.id || active_groups.contains(&g.id));
            state.persist(&data)?;
        }

        let infos: Vec<SyncFeedInfo> = {
            let data = state.lock()?;
            data.feeds
                .iter()
                .filter(|f| f.account_id == account.id)
                .map(|f| SyncFeedInfo {
                    id: f.id.clone(),
                    title: f.title.clone(),
                    account_name: account.name.clone(),
                })
                .collect()
        };
        plan.feeds.extend(infos);
    }
    plan.message = if plan.errors.is_empty() {
        format!("已获取 {} 个订阅源", plan.feeds.len())
    } else {
        format!(
            "已获取 {} 个订阅源（{} 项失败）",
            plan.feeds.len(),
            plan.errors.len()
        )
    };
    Ok(plan)
}

#[tauri::command]
pub async fn sync_subscriptions(state: State<'_, AppState>) -> Result<SyncPlan, String> {
    sync_subscriptions_inner(&state).await
}

/// Phase 2: synchronize one feed. The server's read/starred flags win.
async fn sync_feed_inner(state: &AppState, feed_id: &str) -> FeedSyncResult {
    let (feed, account) = {
        let data = match state.lock() {
            Ok(d) => d,
            Err(e) => {
                return FeedSyncResult {
                    feed_id: feed_id.to_string(),
                    title: String::new(),
                    articles: 0,
                    total: 0,
                    error: Some(e),
                }
            }
        };
        let Some(feed) = data.feeds.iter().find(|f| f.id == feed_id).cloned() else {
            return FeedSyncResult {
                feed_id: feed_id.to_string(),
                title: String::new(),
                articles: 0,
                total: 0,
                error: Some("找不到该订阅源".to_string()),
            };
        };
        let account = data.accounts.iter().find(|a| a.id == feed.account_id).cloned();
        (feed, account)
    };
    let mut result = FeedSyncResult {
        feed_id: feed.id.clone(),
        title: feed.title.clone(),
        articles: 0,
        total: 0,
        error: None,
    };
    let mut added = 0usize;

    let sync = async {
        let now = chrono::Utc::now().to_rfc3339();
        if let Some(account) = account {
            if account.kind == AccountKind::Local {
                let client = http_client()?;
                let parsed = fetch_rss(&client, &feed.url).await?;
                result.total = parsed.items.len();
                let mut data = state.lock()?;
                for item in &parsed.items {
                    let article = article_from_parsed(account.id, &feed, item);
                    if upsert_article(&mut data, article) {
                        added += 1;
                    }
                }
                if let Some(stored) = data.feeds.iter_mut().find(|f| f.id == feed.id) {
                    stored.title = parsed.title.clone();
                    stored.website = parsed.site_url.clone();
                    stored.last_synced = Some(now);
                    stored.failure_count = 0;
                }
                state.persist(&data)?;
            } else {
                let mut client = GoogleReaderClient::new(account.clone())?;
                let stream_id = if feed.remote_id.is_empty() {
                    feed.url.as_str()
                } else {
                    feed.remote_id.as_str()
                };
                // Fetch every page of THIS feed instead of the global
                // reading-list (which is capped and mixes feeds together).
                let items = client
                    .get_stream_all(stream_id, MAX_ARTICLES_PER_FEED)
                    .await?;
                result.total = items.len();
                // Only treat the list as authoritative when it was not cut
                // short by the safety cap; otherwise a cleanup would delete
                // older articles that simply were not fetched yet.
                let complete = items.len() < MAX_ARTICLES_PER_FEED;
                let remote_ids: std::collections::HashSet<String> =
                    items.iter().map(|item| item.id.clone()).collect();
                let mut data = state.lock()?;
                for item in &items {
                    let article = article_from_greader(account.id, &feed.id, item);
                    if upsert_article_synced(&mut data, article) {
                        added += 1;
                    }
                }
                if complete {
                    data.articles
                        .retain(|a| a.feed_id != feed.id || remote_ids.contains(&a.remote_id));
                }
                if let Some(stored) = data.feeds.iter_mut().find(|f| f.id == feed.id) {
                    stored.last_synced = Some(now);
                    stored.failure_count = 0;
                }
                state.persist(&data)?;
            }
        }
        Ok::<(), String>(())
    }
    .await;

    match sync {
        Ok(()) => result.articles = added,
        Err(e) => {
            if let Ok(mut data) = state.lock() {
                if let Some(stored) = data.feeds.iter_mut().find(|f| f.id == feed.id) {
                    stored.failure_count += 1;
                }
                let _ = state.persist(&data);
            }
            result.error = Some(e);
        }
    }
    result
}

#[tauri::command]
pub async fn sync_feed_articles(
    feed_id: String,
    state: State<'_, AppState>,
) -> Result<FeedSyncResult, String> {
    Ok(sync_feed_inner(&state, &feed_id).await)
}

#[tauri::command]
pub async fn sync_all(state: State<'_, AppState>) -> Result<SyncReport, String> {
    // Single-shot compatibility command; the UI uses the two phased commands
    // so it can redraw the feed tree before article synchronization starts.
    let SyncPlan { feeds, errors, .. } = sync_subscriptions_inner(&state).await?;
    let mut report = SyncReport {
        feeds: feeds.len(),
        articles: 0,
        errors,
        message: String::new(),
    };
    for info in &feeds {
        let result = sync_feed_inner(&state, &info.id).await;
        report.articles += result.articles;
        if let Some(error) = result.error {
            report.errors.push(format!("{}：{error}", info.title));
        }
    }
    report.message = if report.errors.is_empty() {
        format!("同步完成：{} 个订阅源，{} 篇新文章", report.feeds, report.articles)
    } else {
        format!(
            "同步完成（{} 项失败）：{} 个订阅源，{} 篇新文章",
            report.errors.len(),
            report.feeds,
            report.articles
        )
    };
    Ok(report)
}

async fn add_feed_inner(url: &str, state: &AppState) -> Result<String, String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("请输入订阅地址。".to_string());
    }
    let account = state
        .lock()?
        .accounts
        .iter()
        .find(|a| a.is_active)
        .cloned()
        .ok_or_else(|| "没有可用的活动账户".to_string())?;

    if account.kind == AccountKind::Local {
        let client = http_client()?;
        let parsed = fetch_rss(&client, url).await?;
        let title = parsed.title.clone();
        let website = parsed.site_url.clone();
        let default_group = state.lock()?.groups.first().map(|g| g.id.clone());
        let feed = Feed {
            id: new_id(),
            account_id: account.id,
            remote_id: String::new(),
            title: title.clone(),
            url: url.to_string(),
            website,
            group_id: default_group,
            last_synced: Some(chrono::Utc::now().to_rfc3339()),
            failure_count: 0,
        };
        let articles: Vec<Article> = parsed
            .items
            .iter()
            .map(|item| article_from_parsed(account.id, &feed, item))
            .collect();
        let mut data = state.lock()?;
        if data.feeds.iter().any(|f| f.url == url && f.account_id == account.id) {
            return Err("该订阅源已存在。".to_string());
        }
        data.feeds.push(feed);
        for article in articles {
            upsert_article(&mut data, article);
        }
        state.persist(&data)?;
        Ok(format!("已添加：{title}"))
    } else {
        let mut client = GoogleReaderClient::new(account.clone())?;
        let stream_id = client.subscribe(url).await?;
        // Refresh the whole subscription tree, then pull every page of the
        // newly subscribed feed so local state matches the server exactly.
        sync_subscriptions_inner(state).await?;
        let feed_id = {
            let data = state.lock()?;
            data.feeds
                .iter()
                .find(|f| {
                    f.account_id == account.id
                        && stream_id
                            .as_deref()
                            .map(|s| f.remote_id == s)
                            .unwrap_or(false)
                })
                .map(|f| f.id.clone())
        };
        let mut added = 0;
        if let Some(id) = feed_id {
            added = sync_feed_inner(state, &id).await.articles;
        }
        Ok(format!(
            "已订阅：{}{}",
            stream_id.unwrap_or_else(|| url.to_string()),
            if added > 0 {
                format!("（{added} 篇新文章）")
            } else {
                String::new()
            }
        ))
    }
}

#[tauri::command]
pub async fn add_feed(url: String, state: State<'_, AppState>) -> Result<String, String> {
    add_feed_inner(&url, &state).await
}

#[tauri::command]
pub async fn remove_feed(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let (account, remote_id) = {
        let mut data = state.lock()?;
        let feed = data
            .feeds
            .iter()
            .find(|f| f.id == id)
            .ok_or_else(|| "找不到该订阅源".to_string())?;
        let account = data
            .accounts
            .iter()
            .find(|a| a.id == feed.account_id)
            .cloned();
        let remote_id = feed.remote_id.clone();
        data.feeds.retain(|f| f.id != id);
        data.articles.retain(|a| a.feed_id != id);
        state.persist(&data)?;
        (account, remote_id)
    };
    if let Some(account) = account {
        if account.kind != AccountKind::Local && !remote_id.is_empty() {
            let mut client = GoogleReaderClient::new(account)?;
            client.unsubscribe(&remote_id).await?;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn import_opml(xml: String, state: State<'_, AppState>) -> Result<String, String> {
    let subscriptions = opml::import(&xml)?;
    let mut ok = 0;
    let mut errors = Vec::new();
    for sub in subscriptions {
        match add_feed_inner(&sub.xml_url, &state).await {
            Ok(_) => ok += 1,
            Err(e) => errors.push(format!("{}：{e}", sub.title)),
        }
    }
    if errors.is_empty() {
        Ok(format!("已导入 {ok} 个订阅源"))
    } else {
        Ok(format!(
            "已导入 {ok} 个订阅源，{} 个失败：{}",
            errors.len(),
            errors.join("；")
        ))
    }
}

#[tauri::command]
pub fn export_opml(state: State<'_, AppState>) -> Result<String, String> {
    let data = state.lock()?;
    Ok(opml::export(&data.feeds, &data.groups))
}

// ---------------------------------------------------------------------------
// Accounts
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn validate_account(
    kind: AccountKind,
    endpoint: String,
    username: String,
    password: String,
) -> Result<(), String> {
    let account = Account {
        id: 0,
        kind,
        name: String::new(),
        endpoint,
        username,
        password_protected: SecretProtector::protect(Some(&password)),
        is_active: true,
    };
    let mut client = GoogleReaderClient::new(account)?;
    client.validate().await
}

#[tauri::command]
pub fn save_account(input: AccountInput, state: State<'_, AppState>) -> Result<Vec<Account>, String> {
    let mut data = state.lock()?;
    let previous = if input.id > 0 {
        data.accounts.iter().find(|a| a.id == input.id).cloned()
    } else {
        None
    };
    let protected = if input.password.trim().is_empty() {
        previous
            .as_ref()
            .map(|a| a.password_protected.clone())
            .unwrap_or_default()
    } else {
        SecretProtector::protect(Some(&input.password))
    };
    let id = if input.id > 0 {
        input.id
    } else {
        data.accounts.iter().map(|a| a.id).max().unwrap_or(0) + 1
    };
    let account = Account {
        id,
        kind: input.kind,
        name: input.name.trim().to_string(),
        endpoint: input.endpoint.trim().to_string(),
        username: input.username.trim().to_string(),
        password_protected: protected,
        is_active: input.is_active,
    };
    if input.is_active {
        for item in data.accounts.iter_mut() {
            item.is_active = false;
        }
    }
    if let Some(existing) = data.accounts.iter_mut().find(|a| a.id == id) {
        *existing = account;
    } else {
        data.accounts.push(account);
    }
    if !data.accounts.iter().any(|a| a.is_active) {
        if let Some(first) = data.accounts.first_mut() {
            first.is_active = true;
        }
    }
    state.persist(&data)?;
    Ok(data.accounts.clone())
}

#[tauri::command]
pub fn delete_account(id: i32, state: State<'_, AppState>) -> Result<Vec<Account>, String> {
    let mut data = state.lock()?;
    data.accounts.retain(|a| a.id != id);
    let feed_ids: Vec<String> = data
        .feeds
        .iter()
        .filter(|f| f.account_id == id)
        .map(|f| f.id.clone())
        .collect();
    data.feeds.retain(|f| f.account_id != id);
    data.articles.retain(|a| a.account_id != id);
    data.groups.retain(|g| g.account_id != id);
    let _ = feed_ids;
    if data.accounts.is_empty() {
        data.accounts.push(Account {
            id: 1,
            kind: AccountKind::Local,
            name: "本地账户".to_string(),
            is_active: true,
            ..Account::default()
        });
    } else if !data.accounts.iter().any(|a| a.is_active) {
        data.accounts[0].is_active = true;
    }
    state.persist(&data)?;
    Ok(data.accounts.clone())
}

#[tauri::command]
pub fn open_external(url: String) -> Result<(), String> {
    tauri_plugin_opener::open_url(&url, None::<&str>).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_link_script_is_injected_once() {
        let page = article_html::render(
            Some("<article><p>hello</p></article>"),
            None,
            false,
            15,
            None,
        );
        let page = page.replacen("</body>", &format!("{EXTERNAL_LINK_SCRIPT}</body>"), 1);
        assert_eq!(page.matches("__readyou_open").count(), 1);
        assert!(page.contains("</body>"));
    }

    #[test]
    fn sync_uses_server_read_and_starred_flags() {
        let item = GReaderItem {
            id: "item-1".to_string(),
            title: "同一篇文章".to_string(),
            link: None,
            published: 1,
            summary_html: None,
            author: None,
            feed_stream_id: "feed/1".to_string(),
            feed_title: None,
            is_read: false,
            is_starred: false,
            thumbnail_url: None,
        };
        let mut data = AppData::default();
        let mut local = article_from_greader(1, "gr1:feed/1", &item);
        local.is_read = true;
        local.is_starred = true;
        local.is_archived = true;
        data.articles.push(local);

        let mut remote = item;
        remote.is_read = false;
        remote.is_starred = false;
        assert!(!upsert_article_synced(
            &mut data,
            article_from_greader(1, "gr1:feed/1", &remote)
        ));
        let stored = &data.articles[0];
        assert!(!stored.is_read, "已读状态以服务器为准");
        assert!(!stored.is_starred, "星标状态以服务器为准");
        assert!(stored.is_archived, "本地归档状态保留");
    }
}
