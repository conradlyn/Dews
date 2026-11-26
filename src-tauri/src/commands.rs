//! Tauri commands bridging the React UI to the ported data layer.

use crate::data::article_html;
use crate::data::fulltext;
use crate::data::fonts;
use crate::data::greader::{self, GReaderItem, GoogleReaderClient};
use crate::data::html;
use crate::data::images;
use crate::data::model::{
    Account, AccountKind, AppData, AppSettings, Article, Feed, FeedGroup, PendingMark,
    PendingSubscription, new_id,
};
use crate::data::opml;
use crate::data::rss::{self, ParsedFeed};
use crate::data::store::{JsonAppStore, SecretProtector};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tauri::State;

pub struct AppState {
    pub store: JsonAppStore,
    pub data: Mutex<AppData>,
    /// Accounts that recently failed a remote call are skipped briefly so a
    /// single unavailable server cannot stall every click with a 12s timeout.
    remote_backoff: Mutex<HashMap<i32, Instant>>,
}

impl AppState {
    pub fn new() -> Self {
        let store = JsonAppStore::new(None);
        let data = store.load();
        Self {
            store,
            data: Mutex::new(data),
            remote_backoff: Mutex::new(HashMap::new()),
        }
    }

    fn remote_ready(&self, account_id: i32) -> bool {
        let Ok(backoff) = self.remote_backoff.lock() else {
            return true;
        };
        match backoff.get(&account_id) {
            Some(until) => Instant::now() >= *until,
            None => true,
        }
    }

    fn note_remote_failure(&self, account_id: i32) {
        if let Ok(mut backoff) = self.remote_backoff.lock() {
            backoff.insert(account_id, Instant::now() + Duration::from_secs(60));
        }
    }

    fn note_remote_success(&self, account_id: i32) {
        if let Ok(mut backoff) = self.remote_backoff.lock() {
            backoff.remove(&account_id);
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
///
/// `changed` and `removed` let the UI fold one feed's outcome into its local
/// copy immediately. Without them the unread counters would only move after
/// every feed finished, which made a 50-feed sync look frozen.
#[derive(Debug, Clone, Serialize)]
pub struct FeedSyncResult {
    pub feed_id: String,
    pub title: String,
    pub articles: usize,
    pub total: usize,
    pub error: Option<String>,
    /// Rows that were inserted or whose read/starred flags moved.
    pub changed: Vec<ArticleView>,
    /// Article ids deleted because the server no longer has them.
    pub removed: Vec<String>,
}

/// Result of the preflight check in the "add feed" dialog.
#[derive(Debug, Clone, Serialize)]
pub struct FeedCheck {
    pub title: String,
    pub website: Option<String>,
    pub remote: bool,
    pub account_name: String,
    pub message: String,
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

/// Build the lightweight row the UI receives. Snippets fall back to the stored
/// plain text and finally to a stripped summary, so a row is never blank.
fn article_view(a: &Article) -> ArticleView {
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
}

#[tauri::command]
pub fn bootstrap(state: State<'_, AppState>) -> Result<AppDataView, String> {
    let data = state.lock()?;
    let articles = data.articles.iter().map(article_view).collect();
    Ok(AppDataView {
        accounts: data.accounts.clone(),
        groups: data.groups.clone(),
        feeds: data.feeds.clone(),
        articles,
        settings: data.settings.clone(),
    })
}

/// Push the unread total to the native shell: a Dock badge on macOS, a star
/// overlay on the Windows taskbar button. The UI owns the number because it
/// also reflects optimistic read/star updates that never touch the server.
#[tauri::command]
pub fn set_unread_badge(count: usize, app: tauri::AppHandle) {
    crate::desktop::set_badge(&app, count);
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
pub fn get_article_html(
    id: String,
    settings_override: Option<AppSettings>,
    state: State<'_, AppState>,
) -> Result<String, String> {
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
    // Prefer the caller's live settings so typography changes take effect
    // immediately, even before the debounced save reaches the backend.
    let settings = settings_override.as_ref().unwrap_or(&data.settings);
    let mut page = article_html::render(
        source,
        base,
        settings.dark_theme,
        settings.font_size,
        Some(&settings.reading_font_family),
        article_html::ReadingStyle {
            margin_px: settings.reading_margin,
            line_height: settings.reading_line_height,
            indent_chars: settings.reading_indent,
        },
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
pub async fn fetch_full_text(
    id: String,
    settings_override: Option<AppSettings>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    // Snapshot everything needed under the lock, then release before awaiting.
    let (link, settings_dark, font_size, reading_font, reading_style) = {
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
        let s = settings_override.as_ref().unwrap_or(&data.settings);
        (
            link,
            s.dark_theme,
            s.font_size,
            s.reading_font_family.clone(),
            article_html::ReadingStyle {
                margin_px: s.reading_margin,
                line_height: s.reading_line_height,
                indent_chars: s.reading_indent,
            },
        )
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
        reading_style,
    );
    if page_html.contains("</body>") {
        page_html = page_html.replacen("</body>", &format!("{EXTERNAL_LINK_SCRIPT}</body>"), 1);
    } else {
        page_html.push_str(EXTERNAL_LINK_SCRIPT);
    }
    Ok(page_html)
}

/// Merge a click into the pending queue. Last write wins for each field.
fn queue_pending_mark(data: &mut AppData, article_id: &str, read: Option<bool>, starred: Option<bool>) {
    let now = chrono::Utc::now().timestamp_millis();
    if let Some(pending) = data
        .pending_marks
        .iter_mut()
        .find(|p| p.article_id == article_id)
    {
        if read.is_some() {
            pending.read = read;
        }
        if starred.is_some() {
            pending.starred = starred;
        }
        pending.updated_at = now;
    } else {
        data.pending_marks.push(PendingMark {
            article_id: article_id.to_string(),
            read,
            starred,
            updated_at: now,
        });
    }
}

fn remove_processed_pending(data: &mut AppData, processed: &[PendingMark]) {
    data.pending_marks.retain(|pending| {
        !processed.iter().any(|done| {
            done.article_id == pending.article_id
                && done.read == pending.read
                && done.starred == pending.starred
                && done.updated_at == pending.updated_at
        })
    });
}

/// Push queued local marks for one account. Failures leave the queue intact
/// and are reported through the normal sync summary instead of a toast per
/// article click.
async fn flush_pending_for_account(state: &AppState, account: &Account) -> Result<(), String> {
    if account.kind == AccountKind::Local || !state.remote_ready(account.id) {
        return Ok(());
    }
    let pending: Vec<PendingMark> = {
        let data = state.lock()?;
        data.pending_marks
            .iter()
            .filter(|p| {
                data.articles.iter().any(|a| {
                    a.id == p.article_id
                        && a.account_id == account.id
                        && !a.remote_id.is_empty()
                })
            })
            .cloned()
            .collect()
    };
    if pending.is_empty() {
        return Ok(());
    }

    let outcome: Result<(), String> = async {
        let mut client =
            GoogleReaderClient::with_timeout(account.clone(), Duration::from_secs(12))?;
        let mut groups: HashMap<(Option<bool>, Option<bool>), Vec<String>> = HashMap::new();
        let mut processed = Vec::new();
        for item in pending {
            let remote_id = {
                let data = state.lock()?;
                data.articles
                    .iter()
                    .find(|a| a.id == item.article_id)
                    .map(|a| a.remote_id.clone())
                    .filter(|id| !id.is_empty())
            };
            let Some(remote_id) = remote_id else { continue };
            groups
                .entry((item.read, item.starred))
                .or_default()
                .push(remote_id);
            processed.push(item);
        }
        for ((read, starred), ids) in groups {
            if let Some(value) = read {
                client
                    .mark_many(
                        &ids,
                        value.then_some(greader::READ_TAG),
                        (!value).then_some(greader::READ_TAG),
                    )
                    .await
                    .map_err(|e| format!("同步已读状态失败：{e}"))?;
            }
            if let Some(value) = starred {
                client
                    .mark_many(
                        &ids,
                        value.then_some(greader::STARRED_TAG),
                        (!value).then_some(greader::STARRED_TAG),
                    )
                    .await
                    .map_err(|e| format!("同步星标状态失败：{e}"))?;
            }
        }
        if !processed.is_empty() {
            let mut data = state.lock()?;
            remove_processed_pending(&mut data, &processed);
            state.persist(&data)?;
        }
        Ok(())
    }
    .await;

    match outcome {
        Ok(()) => {
            state.note_remote_success(account.id);
            Ok(())
        }
        Err(e) => {
            // Do not retry a dead server on every star click. The queued mark
            // is retried by the next sync once the cooldown expires.
            state.note_remote_failure(account.id);
            Err(e)
        }
    }
}

fn pending_subscription_ids(data: &AppData, account_id: i32) -> HashSet<String> {
    data.pending_subscriptions
        .iter()
        .filter(|p| p.account_id == account_id)
        .map(|p| p.feed_id.clone())
        .collect()
}

/// Category label to write to a FreshRSS / Google Reader server. Local groups
/// are private to the local account and must never be pushed remotely.
fn category_label_for(
    data: &AppData,
    account: &Account,
    group_id: Option<&str>,
) -> Option<String> {
    if account.kind == AccountKind::Local {
        return None;
    }
    let group_id = group_id?;
    data.groups
        .iter()
        .find(|g| g.id == group_id && g.account_id == account.id)
        .map(|g| g.name.trim().to_string())
        .filter(|name| !name.is_empty())
}

/// Push locally added subscriptions to a remote account. One queue entry that
/// fails does not block the others; the failed entry stays queued for the next
/// sync, and its local feed is protected from server-side cleanup.
async fn flush_pending_subscriptions_for_account(
    state: &AppState,
    account: &Account,
) -> Result<(), String> {
    if account.kind == AccountKind::Local || !state.remote_ready(account.id) {
        return Ok(());
    }
    let pending: Vec<PendingSubscription> = {
        let data = state.lock()?;
        data.pending_subscriptions
            .iter()
            .filter(|p| p.account_id == account.id)
            .cloned()
            .collect()
    };
    if pending.is_empty() {
        return Ok(());
    }

    let outcome: Result<(), String> = async {
        let mut client =
            GoogleReaderClient::with_timeout(account.clone(), Duration::from_secs(12))?;
        let mut processed: Vec<PendingSubscription> = Vec::new();
        let mut errors: Vec<String> = Vec::new();
        for item in pending {
            let one: Result<(), String> = async {
                let still_local = {
                    let data = state.lock()?;
                    data.feeds.iter().any(|f| f.id == item.feed_id)
                };
                if !still_local {
                    // The feed was deleted locally before the push happened.
                    return Ok(());
                }
                let stream_id = client
                    .subscribe(&item.url)
                    .await?
                    .filter(|id| !id.is_empty())
                    .ok_or_else(|| "服务器未返回订阅 ID".to_string())?;
                if let Some(label) = item.category_label.as_deref() {
                    client.set_subscription_category(&stream_id, label).await?;
                }
                let mut data = state.lock()?;
                if let Some(feed) = data.feeds.iter_mut().find(|f| f.id == item.feed_id) {
                    feed.remote_id = stream_id;
                }
                Ok(())
            }
            .await;
            match one {
                Ok(()) => processed.push(item),
                Err(e) => errors.push(format!("{}：{e}", item.url)),
            }
        }
        if !processed.is_empty() {
            let mut data = state.lock()?;
            data.pending_subscriptions.retain(|p| {
                !processed.iter().any(|done| {
                    done.feed_id == p.feed_id
                        && done.url == p.url
                        && done.category_label == p.category_label
                })
            });
            state.persist(&data)?;
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("；"))
        }
    }
    .await;

    match outcome {
        Ok(()) => {
            state.note_remote_success(account.id);
            Ok(())
        }
        Err(e) => {
            state.note_remote_failure(account.id);
            Err(e)
        }
    }
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
        let is_remote = account
            .as_ref()
            .is_some_and(|a| a.kind != AccountKind::Local && !remote_id.is_empty());
        if is_remote {
            queue_pending_mark(&mut data, &id, read, starred);
        }
        state.persist(&data)?;
        (account, remote_id)
    };

    if let Some(account) = account {
        if account.kind != AccountKind::Local && !remote_id.is_empty() {
            // Best effort. A transport/auth failure must not undo the local
            // click or raise a red toast on every star. The mark stays queued
            // and is retried by the next sync (with a short cooldown).
            if let Err(e) = flush_pending_for_account(&state, &account).await {
                eprintln!("远程阅读状态暂时无法同步，已加入待同步队列：{e}");
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct MarkReadReport {
    pub marked: usize,
    /// Marks that could not be pushed yet and will be retried on next sync.
    pub pending: usize,
}

/// Mark a whole visible list as read with one disk write and one queued batch
/// per remote account. Remote failures stay in `pending_marks`; the UI does not
/// receive an error toast for them.
#[tauri::command]
pub async fn mark_articles_read(
    ids: Vec<String>,
    state: State<'_, AppState>,
) -> Result<MarkReadReport, String> {
    if ids.is_empty() {
        return Ok(MarkReadReport { marked: 0, pending: 0 });
    }
    let wanted: HashSet<&str> = ids.iter().map(String::as_str).collect();
    let (marked, account_ids, accounts) = {
        let mut data = state.lock()?;
        let remote_accounts: HashMap<i32, AccountKind> = data
            .accounts
            .iter()
            .map(|a| (a.id, a.kind))
            .collect();
        let mut marked = 0usize;
        let mut queued_ids = Vec::new();
        let mut account_ids = HashSet::new();
        for article in data.articles.iter_mut() {
            if article.is_read || !wanted.contains(article.id.as_str()) {
                continue;
            }
            article.is_read = true;
            marked += 1;
            let is_remote = remote_accounts
                .get(&article.account_id)
                .is_some_and(|kind| *kind != AccountKind::Local)
                && !article.remote_id.is_empty();
            if is_remote {
                queued_ids.push(article.id.clone());
                account_ids.insert(article.account_id);
            }
        }
        for id in &queued_ids {
            queue_pending_mark(&mut data, id, Some(true), None);
        }
        if marked > 0 {
            state.persist(&data)?;
        }
        (marked, account_ids, data.accounts.clone())
    };

    // Best effort, with the same cooldown as a single star/read click.
    for account_id in account_ids {
        if let Some(account) = accounts.iter().find(|a| a.id == account_id) {
            if let Err(e) = flush_pending_for_account(&state, account).await {
                eprintln!("批量阅读状态暂时无法同步，已加入待同步队列：{e}");
            }
        }
    }

    let pending = {
        let data = state.lock()?;
        data.pending_marks
            .iter()
            .filter(|p| wanted.contains(p.article_id.as_str()))
            .count()
    };
    Ok(MarkReadReport { marked, pending })
}

// ---------------------------------------------------------------------------
// Feeds / synchronization
// ---------------------------------------------------------------------------

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("dews/1.0 (+desktop RSS reader)")
        .timeout(std::time::Duration::from_secs(30))
        .gzip(true)
        .deflate(true)
        .build()
        .map_err(|e| e.to_string())
}

enum FeedFetch {
    NotModified,
    Updated {
        parsed: ParsedFeed,
        etag: Option<String>,
        last_modified: Option<String>,
    },
}

/// Conditional GET for RSS/Atom. A 304 response means the feed has not
/// changed since the last sync, so we skip parsing and article upserts.
async fn fetch_rss_conditional(
    client: &reqwest::Client,
    url: &str,
    etag: Option<&str>,
    last_modified: Option<&str>,
) -> Result<FeedFetch, String> {
    let mut request = client.get(url);
    if let Some(tag) = etag.filter(|s| !s.trim().is_empty()) {
        request = request.header(reqwest::header::IF_NONE_MATCH, tag);
    }
    if let Some(date) = last_modified.filter(|s| !s.trim().is_empty()) {
        request = request.header(reqwest::header::IF_MODIFIED_SINCE, date);
    }
    let response = request
        .send()
        .await
        .map_err(|e| format!("请求订阅源失败：{e}"))?
        .error_for_status()
        .map_err(|e| format!("订阅源返回错误：{e}"))?;
    if response.status().as_u16() == 304 {
        return Ok(FeedFetch::NotModified);
    }
    let etag = response
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let last_modified = response
        .headers()
        .get(reqwest::header::LAST_MODIFIED)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let body = response.text().await.map_err(|e| e.to_string())?;
    Ok(FeedFetch::Updated {
        parsed: rss::parse(&body, Some(url))?,
        etag,
        last_modified,
    })
}

async fn fetch_rss(client: &reqwest::Client, url: &str) -> Result<ParsedFeed, String> {
    match fetch_rss_conditional(client, url, None, None).await? {
        FeedFetch::Updated { parsed, .. } => Ok(parsed),
        FeedFetch::NotModified => Err("订阅源未发生变化，但没有可解析的内容。".to_string()),
    }
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

/// Hard ceiling for one full server pass. 1000 keeps the first sync of a
/// large FreshRSS feed bounded; reaching the cap leaves `complete = false`,
/// so no local article is deleted — the conservative behaviour is intentional.
const MAX_ARTICLES_PER_FEED: usize = 1000;
/// Re-fetch 24 hours of extra history on incremental syncs so backfilled /
/// late-published articles are not missed.
const INCREMENTAL_OVERLAP_SECS: i64 = 86400;
/// Incremental syncs never delete articles. Do one full server reconciliation
/// per day so read/star changes made on older items still converge.
const FULL_RECONCILE_HOURS: i64 = 24;
const MAX_INCREMENTAL_PER_FEED: usize = 1000;

/// Decide whether a feed needs a full reconciliation pass. Two conditions
/// force it: the feed has never had an article pass (`last_synced == None`),
/// or it has never completed a full pass / the last one is older than the
/// reconcile interval (`last_full_sync` missing or stale). Keeping this as a
/// pure function makes the first-install behaviour directly testable.
fn needs_full_sync(
    last_synced: Option<&str>,
    last_full_sync: Option<&str>,
    now: chrono::DateTime<chrono::Utc>,
) -> bool {
    let parse = |value: Option<&str>| {
        value
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.with_timezone(&chrono::Utc))
    };
    let never_synced = parse(last_synced).is_none();
    let full_due = parse(last_full_sync)
        .map(|last| now.signed_duration_since(last) >= chrono::Duration::hours(FULL_RECONCILE_HOURS))
        .unwrap_or(true);
    never_synced || full_due
}

/// Insert or update from the server: the server's read/starred flags normally
/// win. Articles with an unsent local mark keep that flag, and local-only
/// archiving is always preserved because Google Reader has no archive bit.
fn upsert_article_synced(
    data: &mut AppData,
    article: Article,
    pending_ids: Option<&HashSet<String>>,
) -> bool {
    if let Some(existing) = data.articles.iter_mut().find(|a| a.id == article.id) {
        let archived = existing.is_archived;
        let keep_local = pending_ids.is_some_and(|ids| ids.contains(&article.id));
        let read = existing.is_read;
        let starred = existing.is_starred;
        *existing = article;
        existing.is_archived = archived;
        if keep_local {
            existing.is_read = read;
            existing.is_starred = starred;
        }
        false
    } else {
        data.articles.push(article);
        true
    }
}

/// Read/starred flags per article id for one feed. Diffing a snapshot taken
/// before a sync pass against the state afterwards yields exactly the rows the
/// UI must replace, without shipping the whole feed over IPC.
fn flag_snapshot(data: &AppData, feed_id: &str) -> HashMap<String, (bool, bool)> {
    data.articles
        .iter()
        .filter(|a| a.feed_id == feed_id)
        .map(|a| (a.id.clone(), (a.is_read, a.is_starred)))
        .collect()
}

/// Append the rows that were inserted or whose flags moved since `before`.
fn collect_changes(
    data: &AppData,
    feed_id: &str,
    before: &HashMap<String, (bool, bool)>,
    out: &mut Vec<ArticleView>,
) {
    for a in data.articles.iter().filter(|a| a.feed_id == feed_id) {
        let changed = match before.get(&a.id) {
            Some(flags) => *flags != (a.is_read, a.is_starred),
            None => true,
        };
        if changed {
            out.push(article_view(a));
        }
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

        // Push locally added subscriptions first so the server list below is
        // authoritative and does not delete feeds the user just added offline.
        if let Err(e) = flush_pending_subscriptions_for_account(state, &account).await {
            plan.errors.push(format!("{}：待推送订阅失败：{e}", account.name));
        }
        // Push any locally queued read/star changes. If the remote is
        // unavailable, report it once here instead of on every article click.
        if let Err(e) = flush_pending_for_account(state, &account).await {
            plan.errors.push(format!("{}：待同步阅读状态推送失败：{e}", account.name));
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
                // A feed added locally keeps its id and articles until the
                // subscription is pushed; after the push it is recognized by
                // its freshly assigned remote id and updated in place.
                let existing = data.feeds.iter().position(|f| {
                    f.id == id
                        || (f.account_id == account.id
                            && !f.remote_id.is_empty()
                            && f.remote_id == sub.stream_id)
                });
                if let Some(pos) = existing {
                    let feed = &mut data.feeds[pos];
                    feed.title = sub.title.clone();
                    feed.url = sub.stream_id.clone();
                    feed.remote_id = sub.stream_id.clone();
                    feed.website = website;
                    feed.group_id = group_id;
                    // Do not touch last_synced here: this phase only refreshes
                    // the subscription tree, and stamping "now" would shrink
                    // the article phase's incremental window to the overlap.
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
                        // First discovery: no article pass has run yet. A full
                        // reconciliation is required on the first sync.
                        last_synced: None,
                        last_full_sync: None,
                        etag: None,
                        last_modified: None,
                        failure_count: 0,
                    });
                }
            }
            // Remove feeds unsubscribed on the server, together with their
            // articles. Locally added feeds whose remote push failed stay put,
            // otherwise a transient outage would delete the user's new feed.
            let live_ids: HashSet<String> = subscriptions
                .iter()
                .map(|s| format!("gr{}:{}", account.id, s.stream_id))
                .collect();
            let pending_feeds = pending_subscription_ids(&data, account.id);
            let removed: Vec<String> = data
                .feeds
                .iter()
                .filter(|f| {
                    f.account_id == account.id
                        && !live_ids.contains(&f.id)
                        && !live_ids.contains(&format!("gr{}:{}", account.id, f.remote_id))
                        && !pending_feeds.contains(&f.id)
                })
                .map(|f| f.id.clone())
                .collect();
            data.feeds.retain(|f| !removed.contains(&f.id));
            data.articles.retain(|a| !removed.contains(&a.feed_id));
            let live_articles: HashSet<String> =
                data.articles.iter().map(|a| a.id.clone()).collect();
            data.pending_marks
                .retain(|p| live_articles.contains(&p.article_id));
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
                    changed: Vec::new(),
                    removed: Vec::new(),
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
                changed: Vec::new(),
                removed: Vec::new(),
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
        changed: Vec::new(),
        removed: Vec::new(),
    };
    let mut added = 0usize;

    // Queued marks are flushed once per account in phase 1. Doing it here as
    // well would fire duplicate edit-tag requests when feeds sync in parallel.
    // `pending_ids` still keeps local flags authoritative if that push failed.
    let pending_ids = match state.lock() {
        Ok(data) => data
            .pending_marks
            .iter()
            .map(|p| p.article_id.clone())
            .collect::<HashSet<String>>(),
        Err(e) => {
            result.error = Some(e);
            return result;
        }
    };

    let sync = async {
        let now = chrono::Utc::now().to_rfc3339();
        if let Some(account) = account {
            if account.kind == AccountKind::Local {
                let client = http_client()?;
                let fetched = fetch_rss_conditional(
                    &client,
                    &feed.url,
                    feed.etag.as_deref(),
                    feed.last_modified.as_deref(),
                )
                .await?;
                let mut data = state.lock()?;
                match fetched {
                    FeedFetch::NotModified => {
                        // ETag/Last-Modified say nothing changed: skip parsing
                        // and article upserts entirely.
                    }
                    FeedFetch::Updated {
                        parsed,
                        etag,
                        last_modified,
                    } => {
                        result.total = parsed.items.len();
                        let before = flag_snapshot(&data, &feed.id);
                        for item in &parsed.items {
                            let article = article_from_parsed(account.id, &feed, item);
                            if upsert_article(&mut data, article) {
                                added += 1;
                            }
                        }
                        collect_changes(&data, &feed.id, &before, &mut result.changed);
                        if let Some(stored) = data.feeds.iter_mut().find(|f| f.id == feed.id) {
                            stored.title = parsed.title.clone();
                            stored.website = parsed.site_url.clone();
                            stored.etag = etag;
                            stored.last_modified = last_modified;
                        }
                    }
                }
                if let Some(stored) = data.feeds.iter_mut().find(|f| f.id == feed.id) {
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
                // Fetch only the pages newer than the last sync. A full pass
                // is kept for the first sync of a feed and once per day so
                // server-side changes to older articles still converge.
                let parse_ts = |value: Option<&str>| {
                    value
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .map(|d| d.with_timezone(&chrono::Utc))
                };
                let last_sync = parse_ts(feed.last_synced.as_deref());
                let full_sync =
                    needs_full_sync(feed.last_synced.as_deref(), feed.last_full_sync.as_deref(), chrono::Utc::now());
                let (items, complete) = if full_sync {
                    let all = client
                        .get_stream_all(stream_id, MAX_ARTICLES_PER_FEED)
                        .await?;
                    let complete = all.len() < MAX_ARTICLES_PER_FEED;
                    (all, complete)
                } else {
                    let cutoff = last_sync.unwrap().timestamp() - INCREMENTAL_OVERLAP_SECS;
                    let recent = client
                        .get_stream_since(stream_id, cutoff, MAX_INCREMENTAL_PER_FEED)
                        .await?;
                    (recent, false)
                };
                result.total = items.len();
                // Only a complete full pass is authoritative. Incremental
                // results deliberately leave older articles untouched.
                let remote_ids: std::collections::HashSet<String> =
                    items.iter().map(|item| item.id.clone()).collect();
                let mut data = state.lock()?;
                let before = flag_snapshot(&data, &feed.id);
                for item in &items {
                    let article = article_from_greader(account.id, &feed.id, item);
                    if upsert_article_synced(&mut data, article, Some(&pending_ids)) {
                        added += 1;
                    }
                }
                if complete {
                    let mut removed_now = Vec::new();
                    data.articles.retain(|a| {
                        let keep = a.feed_id != feed.id || remote_ids.contains(&a.remote_id);
                        if !keep {
                            removed_now.push(a.id.clone());
                        }
                        keep
                    });
                    result.removed = removed_now;
                }
                collect_changes(&data, &feed.id, &before, &mut result.changed);
                if let Some(stored) = data.feeds.iter_mut().find(|f| f.id == feed.id) {
                    stored.last_synced = Some(now.clone());
                    if complete || stored.last_full_sync.is_none() {
                        stored.last_full_sync = Some(now);
                    }
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

async fn add_feed_inner(
    url: &str,
    group_id: Option<String>,
    state: &AppState,
) -> Result<String, String> {
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
    let remote = account.kind != AccountKind::Local;

    let (group_id, category_label) = {
        let data = state.lock()?;
        if data
            .feeds
            .iter()
            .any(|f| f.account_id == account.id && f.url == url)
        {
            return Err("该订阅源已存在。".to_string());
        }
        let chosen = group_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty());
        if let Some(id) = chosen {
            if !data
                .groups
                .iter()
                .any(|g| g.id == id && g.account_id == account.id)
            {
                return Err("所选分类不属于当前账户。".to_string());
            }
        }
        // Only groups owned by this account may hold its feeds; taking the
        // global first group could assign a local feed to a remote group.
        let group_id = chosen
            .map(str::to_string)
            .or_else(|| {
                data.groups
                    .iter()
                    .find(|g| g.account_id == account.id)
                    .map(|g| g.id.clone())
            });
        let category_label = category_label_for(&data, &account, group_id.as_deref());
        (group_id, category_label)
    };

    // Preflight: a bad URL must fail here, before anything is stored. Remote
    // accounts additionally verify that the FreshRSS / GReader API is usable.
    let client = http_client()?;
    let parsed = fetch_rss(&client, url).await?;
    if remote {
        let mut remote_client = GoogleReaderClient::new(account.clone())?;
        remote_client.validate().await?;
        state.note_remote_success(account.id);
    }

    let title = parsed.title.clone();
    let website = parsed.site_url.clone();
    let feed = Feed {
        id: new_id(),
        account_id: account.id,
        remote_id: String::new(),
        title: title.clone(),
        url: url.to_string(),
        website,
        group_id,
        last_synced: Some(chrono::Utc::now().to_rfc3339()),
        last_full_sync: None,
        etag: None,
        last_modified: None,
        failure_count: 0,
    };
    let articles: Vec<Article> = parsed
        .items
        .iter()
        .map(|item| article_from_parsed(account.id, &feed, item))
        .collect();

    let mut data = state.lock()?;
    data.feeds.push(feed.clone());
    for article in articles {
        upsert_article(&mut data, article);
    }
    if remote {
        data.pending_subscriptions.push(PendingSubscription {
            feed_id: feed.id.clone(),
            account_id: account.id,
            url: url.to_string(),
            category_label,
        });
    }
    state.persist(&data)?;
    if remote {
        Ok(format!(
            "已添加：{title}（将在下次同步时写入 {}）",
            account.display_name()
        ))
    } else {
        Ok(format!("已添加：{title}"))
    }
}

/// Preflight used by the add-feed dialog. Rejects unreachable feeds, invalid
/// RSS/Atom payloads, duplicates and unusable remote accounts before the user
/// commits the subscription.
#[tauri::command]
pub async fn check_feed(url: String, state: State<'_, AppState>) -> Result<FeedCheck, String> {
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
    if state
        .lock()?
        .feeds
        .iter()
        .any(|f| f.account_id == account.id && f.url == url)
    {
        return Err("该订阅源已存在。".to_string());
    }
    let client = http_client()?;
    let parsed = fetch_rss(&client, url).await?;
    let remote = account.kind != AccountKind::Local;
    if remote {
        let mut remote_client = GoogleReaderClient::new(account.clone())?;
        remote_client.validate().await?;
        state.note_remote_success(account.id);
    }
    let message = if remote {
        format!("可连接；添加后将在下次同步时写入 {}", account.display_name())
    } else {
        "可连接；已读取到有效的 RSS / Atom 订阅".to_string()
    };
    Ok(FeedCheck {
        title: parsed.title,
        website: parsed.site_url,
        remote,
        account_name: account.name.clone(),
        message,
    })
}

/// Group management is only valid for the local account. Remote (FreshRSS /
/// GoogleReader) groups are owned by the server and rebuilt on every sync.
fn active_local_account_id(data: &AppData) -> Result<i32, String> {
    let active = data
        .accounts
        .iter()
        .find(|a| a.is_active)
        .ok_or_else(|| "没有可用的活动账户".to_string())?;
    if active.kind != AccountKind::Local {
        return Err("分组管理仅在本地账户下可用。".to_string());
    }
    Ok(active.id)
}

/// Reorder one account's groups in place while leaving other accounts' groups
/// at their existing positions in the vector.
fn reorder_groups_for(
    groups: &mut Vec<FeedGroup>,
    account_id: i32,
    ids: &[String],
) -> Result<(), String> {
    let owned: Vec<String> = groups
        .iter()
        .filter(|g| g.account_id == account_id)
        .map(|g| g.id.clone())
        .collect();
    if ids.len() != owned.len() || !owned.iter().all(|id| ids.contains(id)) {
        return Err("分组顺序参数无效。".to_string());
    }
    let slots: Vec<usize> = groups
        .iter()
        .enumerate()
        .filter(|(_, g)| g.account_id == account_id)
        .map(|(i, _)| i)
        .collect();
    let ordered: Vec<FeedGroup> = ids
        .iter()
        .map(|id| groups.iter().find(|g| &g.id == id).cloned().unwrap())
        .collect();
    for (slot, group) in slots.into_iter().zip(ordered) {
        groups[slot] = group;
    }
    Ok(())
}

#[tauri::command]
pub fn create_group(name: String, state: State<'_, AppState>) -> Result<(), String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("分组名称不能为空。".to_string());
    }
    let mut data = state.lock()?;
    let account_id = active_local_account_id(&data)?;
    if data
        .groups
        .iter()
        .any(|g| g.account_id == account_id && g.name == name)
    {
        return Err("分组名称已存在。".to_string());
    }
    data.groups.push(FeedGroup {
        id: format!("local:{}", new_id()),
        account_id,
        name,
    });
    state.persist(&data)
}

#[tauri::command]
pub fn rename_group(id: String, name: String, state: State<'_, AppState>) -> Result<(), String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("分组名称不能为空。".to_string());
    }
    let mut data = state.lock()?;
    let account_id = active_local_account_id(&data)?;
    if data.groups.iter().any(|g| {
        g.id != id && g.account_id == account_id && g.name == name
    }) {
        return Err("分组名称已存在。".to_string());
    }
    let group = data
        .groups
        .iter_mut()
        .find(|g| g.id == id && g.account_id == account_id)
        .ok_or_else(|| "找不到该分组。".to_string())?;
    group.name = name;
    state.persist(&data)
}

#[tauri::command]
pub fn reorder_groups(ids: Vec<String>, state: State<'_, AppState>) -> Result<(), String> {
    let mut data = state.lock()?;
    let account_id = active_local_account_id(&data)?;
    reorder_groups_for(&mut data.groups, account_id, &ids)?;
    state.persist(&data)
}

#[tauri::command]
pub fn delete_group(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let mut data = state.lock()?;
    let account_id = active_local_account_id(&data)?;
    if !data.groups.iter().any(|g| g.id == id && g.account_id == account_id) {
        return Err("找不到该分组。".to_string());
    }
    // Move the deleted group's feeds to the first remaining local group, or
    // leave them ungrouped when no group is left.
    let fallback = data
        .groups
        .iter()
        .find(|g| g.account_id == account_id && g.id != id)
        .map(|g| g.id.clone());
    for feed in data.feeds.iter_mut() {
        if feed.account_id == account_id && feed.group_id.as_deref() == Some(id.as_str()) {
            feed.group_id = fallback.clone();
        }
    }
    data.groups.retain(|g| g.id != id || g.account_id != account_id);
    state.persist(&data)
}

#[tauri::command]
pub fn set_feed_group(
    feed_id: String,
    group_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut data = state.lock()?;
    let account_id = active_local_account_id(&data)?;
    if let Some(ref gid) = group_id {
        if !data.groups.iter().any(|g| g.id == *gid && g.account_id == account_id) {
            return Err("分组不存在。".to_string());
        }
    }
    let feed = data
        .feeds
        .iter_mut()
        .find(|f| f.id == feed_id && f.account_id == account_id)
        .ok_or_else(|| "找不到该订阅源。".to_string())?;
    feed.group_id = group_id;
    state.persist(&data)
}

#[tauri::command]
pub async fn add_feed(
    url: String,
    group_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    add_feed_inner(&url, group_id, &state).await
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
        data.pending_subscriptions.retain(|p| p.feed_id != id);
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
        match add_feed_inner(&sub.xml_url, None, &state).await {
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
    data.pending_subscriptions
        .retain(|p| p.account_id != id && !feed_ids.contains(&p.feed_id));
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
    fn reorder_groups_only_touches_owned_account() {
        let mk = |id: &str, account_id: i32| FeedGroup {
            id: id.to_string(),
            account_id,
            name: id.to_string(),
        };
        let mut groups = vec![
            mk("local:1", 1),
            mk("gr2:A", 2),
            mk("local:2", 1),
            mk("gr2:B", 2),
        ];
        reorder_groups_for(&mut groups, 1, &["local:2".to_string(), "local:1".to_string()])
            .unwrap();
        let ids: Vec<String> = groups.iter().map(|g| g.id.clone()).collect();
        assert_eq!(ids, vec!["local:2", "gr2:A", "local:1", "gr2:B"]);

        // An incomplete id list must be rejected without touching the data.
        assert!(reorder_groups_for(&mut groups, 1, &["local:1".to_string()]).is_err());
        let after: Vec<String> = groups.iter().map(|g| g.id.clone()).collect();
        assert_eq!(after, ids);
    }

    #[test]
    fn group_management_rejects_remote_account() {
        let remote_active = AppData {
            accounts: vec![
                Account {
                    id: 1,
                    kind: AccountKind::Local,
                    is_active: false,
                    ..Account::default()
                },
                Account {
                    id: 2,
                    kind: AccountKind::FreshRSS,
                    is_active: true,
                    ..Account::default()
                },
            ],
            ..AppData::default()
        };
        assert!(active_local_account_id(&remote_active).is_err());

        let local_active = AppData {
            accounts: vec![Account {
                id: 1,
                kind: AccountKind::Local,
                is_active: true,
                ..Account::default()
            }],
            ..AppData::default()
        };
        assert_eq!(active_local_account_id(&local_active).unwrap(), 1);
    }

    #[test]
    fn external_link_script_is_injected_once() {
        let page = article_html::render(
            Some("<article><p>hello</p></article>"),
            None,
            false,
            15,
            None,
            article_html::ReadingStyle::default(),
        );
        let page = page.replacen("</body>", &format!("{EXTERNAL_LINK_SCRIPT}</body>"), 1);
        assert_eq!(page.matches("__readyou_open").count(), 1);
        assert!(page.contains("</body>"));
    }

    #[test]
    fn first_sync_is_always_full_and_later_syncs_are_incremental() {
        let now = chrono::Utc::now();
        let recent = now.to_rfc3339();
        let stale = (now - chrono::Duration::hours(FULL_RECONCILE_HOURS + 1)).to_rfc3339();

        // Fresh install: subscription discovery left both timestamps empty.
        assert!(needs_full_sync(None, None, now), "首次安装必须全量");
        // Upgrade from an older build: article pass happened but no full pass.
        assert!(needs_full_sync(Some(&recent), None, now), "旧版本升级必须补一次全量");
        // Full pass older than the reconcile interval.
        assert!(needs_full_sync(Some(&recent), Some(&stale), now), "过期全量需重做");
        // Normal case: recent full pass and last sync -> incremental is safe.
        assert!(
            !needs_full_sync(Some(&recent), Some(&recent), now),
            "近期已全量则走增量"
        );
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
            article_from_greader(1, "gr1:feed/1", &remote),
            None
        ));
        let stored = &data.articles[0];
        assert!(!stored.is_read, "已读状态以服务器为准");
        assert!(!stored.is_starred, "星标状态以服务器为准");
        assert!(stored.is_archived, "本地归档状态保留");
    }

    #[test]
    fn pending_mark_queue_merges_last_write_wins() {
        let mut data = AppData::default();
        queue_pending_mark(&mut data, "a1", Some(true), None);
        queue_pending_mark(&mut data, "a1", None, Some(true));
        assert_eq!(data.pending_marks.len(), 1);
        let pending = data.pending_marks[0].clone();
        assert_eq!(pending.read, Some(true));
        assert_eq!(pending.starred, Some(true));

        remove_processed_pending(&mut data, &[pending]);
        assert!(data.pending_marks.is_empty());
    }

    #[test]
    fn pending_local_flags_survive_server_refresh() {
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
        data.articles.push(local);

        let pending_ids = HashSet::from(["gr1:item-1".to_string()]);
        assert!(!upsert_article_synced(
            &mut data,
            article_from_greader(1, "gr1:feed/1", &item),
            Some(&pending_ids)
        ));
        let stored = &data.articles[0];
        assert!(stored.is_read, "待推送的已读状态不被服务器刷新覆盖");
        assert!(stored.is_starred, "待推送的星标状态不被服务器刷新覆盖");
    }

    #[test]
    fn category_label_uses_only_remote_group_names() {
        let data = AppData {
            groups: vec![
                FeedGroup {
                    id: "gr2:科技".to_string(),
                    account_id: 2,
                    name: "科技".to_string(),
                },
                FeedGroup {
                    id: "local:1".to_string(),
                    account_id: 1,
                    name: "本地分类".to_string(),
                },
            ],
            ..AppData::default()
        };
        let remote = Account {
            id: 2,
            kind: AccountKind::FreshRSS,
            ..Account::default()
        };
        let local = Account {
            id: 1,
            kind: AccountKind::Local,
            ..Account::default()
        };
        assert_eq!(
            category_label_for(&data, &remote, Some("gr2:科技")).as_deref(),
            Some("科技")
        );
        assert_eq!(category_label_for(&data, &remote, Some("local:1")), None);
        assert_eq!(category_label_for(&data, &local, Some("local:1")), None);
    }

    /// Minimal FreshRSS-compatible server: login + token + quickadd + folder edit.
    #[tokio::test]
    async fn pending_subscription_flush_writes_remote_and_category() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::thread;

        let port = 18803u16;
        let bodies: std::sync::Arc<Mutex<Vec<String>>> = std::sync::Arc::new(Mutex::new(Vec::new()));
        let seen = bodies.clone();
        let server = thread::spawn(move || {
            let listener = TcpListener::bind(format!("127.0.0.1:{port}")).unwrap();
            listener.set_nonblocking(true).unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while std::time::Instant::now() < deadline {
                let mut stream = match listener.accept() {
                    Ok((s, _)) => s,
                    Err(_) => {
                        thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                };
                stream.set_nonblocking(false).ok();
                let mut buf = vec![0u8; 8192];
                let n = stream.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    continue;
                }
                let request = String::from_utf8_lossy(&buf[..n]).to_string();
                let first = request.lines().next().unwrap_or("");
                let mut parts = first.split_whitespace();
                let method = parts.next().unwrap_or("");
                let target = parts.next().unwrap_or("");
                let path = target.split('?').next().unwrap_or(target);
                let body = request
                    .split_once("\r\n\r\n")
                    .map(|(_, b)| b.to_string())
                    .unwrap_or_default();
                seen.lock().unwrap().push(body.clone());
                let response = if path.ends_with("/accounts/ClientLogin") && method == "POST" {
                    "Auth=tester/A1B2C3\n".to_string()
                } else if path.ends_with("/token") {
                    "TOKEN123\n".to_string()
                } else if path.ends_with("/subscription/quickadd") {
                    r#"{"streamId":"feed/77","numResults":1}"#.to_string()
                } else if path.ends_with("/subscription/edit") {
                    "OK".to_string()
                } else {
                    "{}".to_string()
                };
                let http = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    response.len(),
                    response
                );
                stream.write_all(http.as_bytes()).ok();
            }
        });

        let dir = std::env::temp_dir().join(format!("dews_queue_sub_{}", std::process::id()));
        let store = JsonAppStore::new(Some(dir.join("data.json")));
        let account = Account {
            id: 3,
            kind: AccountKind::FreshRSS,
            name: "测试远端".to_string(),
            endpoint: format!("http://127.0.0.1:{port}"),
            username: "tester".to_string(),
            password_protected: SecretProtector::protect(Some("api-pass")),
            is_active: true,
        };
        let data = AppData {
            accounts: vec![account.clone()],
            feeds: vec![Feed {
                id: "local-feed".to_string(),
                account_id: 3,
                title: "本地新增订阅".to_string(),
                url: "https://example.com/feed.xml".to_string(),
                ..Feed::default()
            }],
            pending_subscriptions: vec![PendingSubscription {
                feed_id: "local-feed".to_string(),
                account_id: 3,
                url: "https://example.com/feed.xml".to_string(),
                category_label: Some("科技".to_string()),
            }],
            ..AppData::default()
        };
        store.save(&data).unwrap();
        let state = AppState {
            store,
            data: Mutex::new(data),
            remote_backoff: Mutex::new(HashMap::new()),
        };

        flush_pending_subscriptions_for_account(&state, &account)
            .await
            .expect("待推送订阅应成功写入远端");

        {
            let stored = state.lock().unwrap();
            assert!(stored.pending_subscriptions.is_empty());
            assert_eq!(stored.feeds[0].remote_id, "feed/77");
        }
        let bodies = bodies.lock().unwrap().clone();
        assert!(
            bodies
                .iter()
                .any(|b| b.contains("a=user%2F-%2Flabel%2F%E7%A7%91%E6%8A%80")),
            "分类必须以 user/-/label/科技 写入远端：{bodies:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
        server.join().ok();
    }

    #[tokio::test]
    async fn conditional_rss_fetch_skips_unchanged_feed() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::thread;

        let port = 18804u16;
        let headers: std::sync::Arc<Mutex<Vec<String>>> =
            std::sync::Arc::new(Mutex::new(Vec::new()));
        let seen = headers.clone();
        let server = thread::spawn(move || {
            let listener = TcpListener::bind(format!("127.0.0.1:{port}")).unwrap();
            listener.set_nonblocking(true).unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while std::time::Instant::now() < deadline {
                let mut stream = match listener.accept() {
                    Ok((s, _)) => s,
                    Err(_) => {
                        thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                };
                stream.set_nonblocking(false).ok();
                let mut buf = vec![0u8; 16384];
                let n = stream.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    continue;
                }
                let request = String::from_utf8_lossy(&buf[..n]).to_string();
                let conditional = request
                    .lines()
                    .find(|l| l.to_ascii_lowercase().starts_with("if-none-match:"))
                    .unwrap_or("")
                    .to_string();
                seen.lock().unwrap().push(conditional.clone());
                let response = if !conditional.is_empty() {
                    "HTTP/1.1 304 Not Modified\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        .to_string()
                } else {
                    let body = r#"<?xml version="1.0"?><rss version="2.0"><channel><title>测试订阅</title><link>https://example.com</link><description>测试</description><item><title>第一篇</title><link>https://example.com/1</link><guid>1</guid></item></channel></rss>"#;
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/xml\r\nETag: \"dews-test\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                };
                stream.write_all(response.as_bytes()).ok();
            }
        });

        let client = http_client().unwrap();
        let url = format!("http://127.0.0.1:{port}/feed.xml");
        let first = fetch_rss_conditional(&client, &url, None, None)
            .await
            .expect("首次抓取成功");
        let FeedFetch::Updated { parsed, etag, .. } = first else {
            panic!("首次抓取应返回完整订阅");
        };
        assert_eq!(parsed.title, "测试订阅");
        assert_eq!(etag.as_deref(), Some("\"dews-test\""));

        let second = fetch_rss_conditional(&client, &url, etag.as_deref(), None)
            .await
            .expect("条件请求成功");
        assert!(matches!(second, FeedFetch::NotModified));
        let seen = headers.lock().unwrap().clone();
        assert!(
            seen.iter().any(|h| h.contains("dews-test")),
            "第二次请求必须带上 If-None-Match：{seen:?}"
        );
        server.join().ok();
    }

    #[test]
    fn sync_reports_only_new_and_flag_changed_rows() {
        let mk = |id: &str, feed: &str, read: bool| Article {
            id: id.to_string(),
            feed_id: feed.to_string(),
            title: format!("t-{id}"),
            is_read: read,
            ..Article::default()
        };
        let mut data = AppData {
            articles: vec![mk("a1", "f1", true), mk("a2", "f1", false), mk("b1", "f2", false)],
            ..AppData::default()
        };

        let before = flag_snapshot(&data, "f1");
        assert_eq!(before.len(), 2, "快照只覆盖本订阅源");

        // One brand-new unread row, plus a read-flag flip on an existing row.
        data.articles.push(mk("a3", "f1", false));
        data.articles[1].is_read = true;

        let mut changed = Vec::new();
        collect_changes(&data, "f1", &before, &mut changed);

        let mut ids: Vec<&str> = changed.iter().map(|a| a.id.as_str()).collect();
        ids.sort();
        assert_eq!(
            ids,
            vec!["a2", "a3"],
            "只有新增行和标记变化的行需要回传给界面"
        );
        // The untouched row and the other feed's row must stay out of the diff.
        assert!(!ids.contains(&"a1"));
        assert!(!ids.contains(&"b1"));
        assert!(changed.iter().find(|a| a.id == "a2").unwrap().is_read);
        assert!(!changed.iter().find(|a| a.id == "a3").unwrap().is_read);
    }
}
