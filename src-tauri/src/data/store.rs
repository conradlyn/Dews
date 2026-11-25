//! Local JSON document store + platform-native password protection
//! (Windows DPAPI / macOS Keychain).
//!
//! Article bodies can be huge (a few hundred MB after "获取全文"). They live
//! in a sidecar `content.json` and are only rewritten when the body actually
//! changes, so marking one article read or dragging a splitter no longer
//! rewrites the whole library. Writes stay atomic (temp file + rename).

use crate::data::model::{
    Account, AccountKind, AppData, AppSettings, Feed, FeedGroup, PendingMark,
};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Metadata written to data.json. Deliberately omits article bodies.
#[derive(Serialize)]
struct ArticleMeta<'a> {
    id: &'a str,
    account_id: i32,
    feed_id: &'a str,
    remote_id: &'a str,
    title: &'a str,
    author: Option<&'a str>,
    link: Option<&'a str>,
    published: i64,
    is_read: bool,
    is_starred: bool,
    is_archived: bool,
    snippet: Option<&'a str>,
    thumbnail_url: Option<&'a str>,
}

#[derive(Serialize)]
struct DataMeta<'a> {
    accounts: &'a [Account],
    groups: &'a [FeedGroup],
    feeds: &'a [Feed],
    articles: Vec<ArticleMeta<'a>>,
    settings: &'a AppSettings,
    pending_marks: &'a [PendingMark],
}

#[derive(Clone, Serialize, Deserialize)]
struct ArticleContent {
    #[serde(default)]
    summary_html: Option<String>,
    content_html: Option<String>,
    content_text: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct ContentFile {
    articles: HashMap<String, ArticleContent>,
}

#[derive(Serialize)]
struct ContentRef<'a> {
    summary_html: Option<&'a str>,
    content_html: Option<&'a str>,
    content_text: Option<&'a str>,
}

#[derive(Serialize)]
struct ContentFileRef<'a> {
    articles: HashMap<&'a str, ContentRef<'a>>,
}

pub struct JsonAppStore {
    pub path: PathBuf,
    content_path: PathBuf,
    write_lock: Mutex<()>,
    last_content_hash: Mutex<Option<u64>>,
}

impl JsonAppStore {
    /// Portable mode: a "portable" marker next to the exe keeps data in ./data.
    pub fn new(data_path: Option<PathBuf>) -> Self {
        let base = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."));
        let portable = base.join("portable").exists();
        let dir = if portable {
            base.join("data")
        } else {
            #[cfg(target_os = "macos")]
            {
                // macOS apps launched from Finder have a read-only CWD (usually
                // "/"), so never fall back to "." here. Use the standard
                // per-user Application Support directory.
                let home = std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("."));
                home.join("Library/Application Support/dews")
            }
            #[cfg(target_os = "windows")]
            {
                let local = std::env::var_os("LOCALAPPDATA")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("."));
                // Renamed from ReadYouTauri: keep reading the old folder so an
                // existing install does not lose its library.
                let legacy = local.join("ReadYouTauri");
                let current = local.join("dews");
                if !current.exists() && legacy.exists() {
                    legacy
                } else {
                    current
                }
            }
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            {
                let data_home = std::env::var_os("XDG_DATA_HOME")
                    .map(PathBuf::from)
                    .or_else(|| {
                        std::env::var_os("HOME")
                            .map(|h| PathBuf::from(h).join(".local/share"))
                    })
                    .unwrap_or_else(|| PathBuf::from("."));
                data_home.join("dews")
            }
        };
        if let Err(e) = fs::create_dir_all(&dir) {
            eprintln!("无法创建数据目录 {dir:?}：{e}");
        }
        let path = data_path.unwrap_or_else(|| dir.join("data.json"));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok();
        }
        let content_path = path
            .parent()
            .unwrap_or(Path::new("."))
            .join("content.json");
        Self {
            path,
            content_path,
            write_lock: Mutex::new(()),
            last_content_hash: Mutex::new(None),
        }
    }

    pub fn is_portable() -> bool {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("portable").exists()))
            .unwrap_or(false)
    }

    pub fn load(&self) -> AppData {
        if !self.path.exists() {
            let data = fresh_data();
            let _ = self.save(&data);
            return data;
        }
        let json = match fs::read_to_string(&self.path) {
            Ok(json) => json,
            Err(e) => {
                eprintln!("数据文件读取失败：{e}");
                return fresh_data();
            }
        };
        let mut data: AppData = match serde_json::from_str(&json) {
            Ok(d) => d,
            Err(e) => {
                let backup = self.path.with_extension(format!(
                    "bad.{}",
                    chrono::Utc::now().format("%Y%m%d%H%M%S")
                ));
                fs::copy(&self.path, &backup).ok();
                eprintln!("数据文件读取失败，已备份至 {backup:?}：{e}");
                return fresh_data();
            }
        };

        let legacy = !self.content_path.exists();
        let content: ContentFile = if legacy {
            // Older builds kept bodies inline in data.json. Move them to the
            // sidecar once, then keep both structures in memory.
            let mut articles = HashMap::new();
            for article in &mut data.articles {
                let summary = article.summary_html.clone();
                let html = article.content_html.clone();
                let text = article.content_text.clone();
                if summary.is_some() || html.is_some() || text.is_some() {
                    articles.insert(
                        article.id.clone(),
                        ArticleContent {
                            summary_html: summary,
                            content_html: html,
                            content_text: text,
                        },
                    );
                }
            }
            ContentFile { articles }
        } else {
            match fs::read_to_string(&self.content_path)
                .ok()
                .and_then(|s| serde_json::from_str::<ContentFile>(&s).ok())
            {
                Some(c) => c,
                None => {
                    eprintln!("正文数据读取失败，文章将只显示订阅摘要");
                    ContentFile::default()
                }
            }
        };

        for article in &mut data.articles {
            if let Some(body) = content.articles.get(&article.id) {
                article.summary_html = body.summary_html.clone();
                article.content_html = body.content_html.clone();
                article.content_text = body.content_text.clone();
            }
        }
        if legacy {
            // One-time migration; subsequent saves touch only changed bodies.
            let _ = self.save(&data);
        }
        if data.accounts.is_empty() {
            data.accounts.push(local_account());
        }
        // Seed the hash from what we just loaded. Without this, the first save
        // (e.g. a settings change on launch) sees last=None and needlessly
        // rewrites the entire multi-hundred-MB content sidecar.
        if !legacy {
            if let Ok(mut last) = self.last_content_hash.lock() {
                *last = Some(content_hash(&data));
            }
        }
        data
    }

    /// Save metadata every time. Article bodies are written only when their
    /// fingerprint changes, which keeps click/sync saves small and fast.
    pub fn save(&self, data: &AppData) -> Result<(), String> {
        let _guard = self.write_lock.lock().map_err(|e| e.to_string())?;
        let hash = content_hash(data);
        let need_content = {
            let last = self.last_content_hash.lock().map_err(|e| e.to_string())?;
            *last != Some(hash) || !self.content_path.exists()
        };
        // Bodies first: if the process dies after this, the previous data.json
        // still has its inline copy (legacy) or the content file is complete.
        if need_content {
            self.write_content(data)?;
        }
        self.write_meta(data)?;
        if need_content {
            let mut last = self.last_content_hash.lock().map_err(|e| e.to_string())?;
            *last = Some(hash);
        }
        Ok(())
    }

    fn write_meta(&self, data: &AppData) -> Result<(), String> {
        let view = DataMeta {
            accounts: &data.accounts,
            groups: &data.groups,
            feeds: &data.feeds,
            articles: data
                .articles
                .iter()
                .map(|a| ArticleMeta {
                    id: &a.id,
                    account_id: a.account_id,
                    feed_id: &a.feed_id,
                    remote_id: &a.remote_id,
                    title: &a.title,
                    author: a.author.as_deref(),
                    link: a.link.as_deref(),
                    published: a.published,
                    is_read: a.is_read,
                    is_starred: a.is_starred,
                    is_archived: a.is_archived,
                    snippet: a.snippet.as_deref(),
                    thumbnail_url: a.thumbnail_url.as_deref(),
                })
                .collect(),
            settings: &data.settings,
            pending_marks: &data.pending_marks,
        };
        atomic_write(&self.path, |w| serde_json::to_writer(w, &view).map_err(Into::into))
    }

    fn write_content(&self, data: &AppData) -> Result<(), String> {
        let mut articles = HashMap::new();
        for a in &data.articles {
            if a.summary_html.is_some() || a.content_html.is_some() || a.content_text.is_some() {
                articles.insert(
                    a.id.as_str(),
                    ContentRef {
                        summary_html: a.summary_html.as_deref(),
                        content_html: a.content_html.as_deref(),
                        content_text: a.content_text.as_deref(),
                    },
                );
            }
        }
        let view = ContentFileRef { articles };
        atomic_write(&self.content_path, |w| {
            serde_json::to_writer(w, &view).map_err(Into::into)
        })
    }
}

/// Cheap fingerprint: content length plus the first/last 64 bytes catches
/// replaced or extended bodies without hashing hundreds of MB on every save.
fn content_hash(data: &AppData) -> u64 {
    let mut hasher = DefaultHasher::new();
    let mut with_body = 0usize;
    for article in &data.articles {
        for body in [
            &article.summary_html,
            &article.content_html,
            &article.content_text,
        ] {
            let Some(s) = body else { continue };
            with_body += 1;
            article.id.hash(&mut hasher);
            s.len().hash(&mut hasher);
            let head = &s.as_bytes()[..s.len().min(64)];
            let tail = &s.as_bytes()[s.len().saturating_sub(64)..];
            head.hash(&mut hasher);
            tail.hash(&mut hasher);
        }
    }
    with_body.hash(&mut hasher);
    hasher.finish()
}

fn atomic_write<F>(path: &Path, write: F) -> Result<(), String>
where
    F: FnOnce(&mut BufWriter<File>) -> Result<(), std::io::Error>,
{
    let tmp = path.with_extension("tmp");
    {
        let file = File::create(&tmp).map_err(|e| e.to_string())?;
        let mut writer = BufWriter::new(file);
        write(&mut writer).map_err(|e| e.to_string())?;
        writer.flush().map_err(|e| e.to_string())?;
    }
    if path.exists() {
        fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    fs::rename(&tmp, path).map_err(|e| e.to_string())
}

fn local_account() -> Account {
    Account {
        id: 1,
        kind: AccountKind::Local,
        name: "本地账户".to_string(),
        is_active: true,
        ..Account::default()
    }
}

fn fresh_data() -> AppData {
    AppData {
        accounts: vec![local_account()],
        groups: vec![FeedGroup::default()],
        settings: AppSettings::default(),
        ..AppData::default()
    }
}

#[cfg(windows)]
const ENTROPY: &[u8] = b"ReadYouDesktop.AccountPassword.v1";

/// Reference stored in `data.json` on macOS. The secret itself lives in the
/// login Keychain, keyed by a stable hash of the plaintext plus a fixed
/// account label; the JSON only ever holds this non-reversible marker.
#[cfg(target_os = "macos")]
const KEYCHAIN_SERVICE: &str = "com.lfk.dews.account";
#[cfg(target_os = "macos")]
const KEYCHAIN_MARKER: &str = "keychain:v1:";

/// Decrypted secrets are cached per process. Without this, every star/read
/// click and every sync step creates a GoogleReader client and re-reads the
/// Keychain, which can show the macOS authorization dialog repeatedly.
#[cfg(target_os = "macos")]
fn keychain_cache() -> &'static Mutex<HashMap<String, String>> {
    static CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Platform-native secret protection: DPAPI on Windows, Keychain on macOS,
/// pass-through on other Unix (Linux dev only; no secure store available).
pub struct SecretProtector;

impl SecretProtector {
    pub fn protect(plain: Option<&str>) -> String {
        let plain = match plain {
            Some(p) if !p.is_empty() => p,
            _ => return String::new(),
        };
        #[cfg(target_os = "macos")]
        {
            return match keychain_protect(plain) {
                Ok(marker) => marker,
                Err(_) => String::new(),
            };
        }
        #[cfg(not(target_os = "macos"))]
        {
            match dpapi_protect(plain.as_bytes()) {
                Ok(bytes) => base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes),
                Err(_) => String::new(),
            }
        }
    }

    pub fn unprotect(cipher: Option<&str>) -> String {
        let cipher = match cipher {
            Some(c) if !c.is_empty() => c,
            _ => return String::new(),
        };
        #[cfg(target_os = "macos")]
        {
            if let Some(account) = cipher.strip_prefix(KEYCHAIN_MARKER) {
                return keychain_unprotect(account).unwrap_or_default();
            }
            // Backward compatibility: plaintext/base64 written by older builds.
        }
        let bytes = match base64::Engine::decode(&base64::engine::general_purpose::STANDARD, cipher) {
            Ok(b) => b,
            Err(_) => return String::new(),
        };
        match dpapi_unprotect(&bytes) {
            Ok(bytes) => String::from_utf8(bytes).unwrap_or_default(),
            Err(_) => String::new(),
        }
    }
}

#[cfg(windows)]
fn dpapi_protect(plain: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CRYPT_INTEGER_BLOB,
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: plain.len() as u32,
        pbData: plain.as_ptr() as *mut u8,
    };
    let entropy = CRYPT_INTEGER_BLOB {
        cbData: ENTROPY.len() as u32,
        pbData: ENTROPY.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    let ok = unsafe {
        CryptProtectData(
            &input,
            std::ptr::null(),
            &entropy,
            std::ptr::null(),
            std::ptr::null(),
            0, // CRYPT_CURRENT_USER
            &mut output,
        )
    };
    if ok == 0 {
        return Err("CryptProtectData 失败".to_string());
    }
    let bytes = unsafe {
        std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec()
    };
    Ok(bytes)
}

#[cfg(windows)]
fn dpapi_unprotect(cipher: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPT_INTEGER_BLOB,
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: cipher.len() as u32,
        pbData: cipher.as_ptr() as *mut u8,
    };
    let entropy = CRYPT_INTEGER_BLOB {
        cbData: ENTROPY.len() as u32,
        pbData: ENTROPY.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    let ok = unsafe {
        CryptUnprotectData(
            &input,
            std::ptr::null_mut(),
            &entropy,
            std::ptr::null(),
            std::ptr::null(),
            0,
            &mut output,
        )
    };
    if ok == 0 {
        return Err("CryptUnprotectData 失败".to_string());
    }
    let bytes = unsafe {
        std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec()
    };
    Ok(bytes)
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn dpapi_protect(plain: &[u8]) -> Result<Vec<u8>, String> {
    Ok(plain.to_vec())
}

#[cfg(not(windows))]
fn dpapi_unprotect(cipher: &[u8]) -> Result<Vec<u8>, String> {
    Ok(cipher.to_vec())
}

// --- macOS Keychain -------------------------------------------------------

#[cfg(target_os = "macos")]
fn keychain_fingerprint(plain: &str) -> String {
    let mut hasher = DefaultHasher::new();
    plain.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(target_os = "macos")]
fn keychain_protect(plain: &str) -> Result<String, String> {
    use security_framework::passwords::set_generic_password;

    let account = keychain_fingerprint(plain);
    set_generic_password(KEYCHAIN_SERVICE, &account, plain.as_bytes())
        .map_err(|e| format!("Keychain 写入失败: {}", e))?;
    if let Ok(mut cache) = keychain_cache().lock() {
        cache.remove(&account);
    }
    Ok(format!("{}{}", KEYCHAIN_MARKER, account))
}

#[cfg(target_os = "macos")]
fn keychain_unprotect(account: &str) -> Result<String, String> {
    use security_framework::passwords::get_generic_password;

    if account.len() != 16 || !account.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("无效的 Keychain 引用".to_string());
    }
    // Hold the cache lock across the Keychain read so concurrent sync/mark
    // calls can only trigger one authorization dialog for the same secret.
    let mut cache = keychain_cache()
        .lock()
        .map_err(|e| format!("Keychain 缓存访问失败: {e}"))?;
    if let Some(cached) = cache.get(account) {
        return Ok(cached.clone());
    }
    let bytes = get_generic_password(KEYCHAIN_SERVICE, account)
        .map_err(|e| format!("Keychain 读取失败: {}", e))?;
    let value = String::from_utf8(bytes).map_err(|e| e.to_string())?;
    cache.insert(account.to_string(), value.clone());
    Ok(value)
}

#[cfg(all(test, target_os = "macos"))]
fn keychain_delete(account: &str) -> Result<(), String> {
    use security_framework::passwords::delete_generic_password;
    delete_generic_password(KEYCHAIN_SERVICE, account)
        .map_err(|e| format!("Keychain 删除失败: {}", e))?;
    if let Ok(mut cache) = keychain_cache().lock() {
        cache.remove(account);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::model::Article;

    #[test]
    fn secret_roundtrip() {
        let cipher = SecretProtector::protect(Some("api-pass"));
        assert!(!cipher.is_empty());
        assert_eq!(SecretProtector::unprotect(Some(&cipher)), "api-pass");
        assert!(SecretProtector::unprotect(Some("garbage!!!")).is_empty());
        #[cfg(target_os = "macos")]
        if let Some(account) = cipher.strip_prefix(KEYCHAIN_MARKER) {
            let _ = keychain_delete(account);
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn default_store_dir_is_application_support() {
        let store = JsonAppStore::new(None);
        let path_str = store.path.to_string_lossy();
        assert!(
            path_str.contains("Library/Application Support/dews"),
            "unexpected store path: {path_str}"
        );
        assert!(
            store.path.parent().unwrap().exists(),
            "store directory was not created: {:?}",
            store.path.parent()
        );
    }

    #[test]
    fn store_creates_fresh_data_and_roundtrips() {
        let dir = std::env::temp_dir().join(format!("ry_store_test_{}", std::process::id()));
        let path = dir.join("data.json");
        let _ = fs::remove_dir_all(&dir);
        let store = JsonAppStore::new(Some(path.clone()));
        let data = store.load();
        assert_eq!(data.accounts.len(), 1);
        assert_eq!(data.accounts[0].kind, AccountKind::Local);
        assert_eq!(data.settings.summary_lines, 2);
        assert!(path.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn data_without_pending_marks_still_loads() {
        let dir = std::env::temp_dir().join(format!("ry_store_pending_{}", std::process::id()));
        let path = dir.join("data.json");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let mut value = serde_json::to_value(AppData::default()).unwrap();
        let settings = value
            .as_object_mut()
            .unwrap()
            .remove("pending_marks")
            .is_some();
        assert!(settings);
        value
            .as_object_mut()
            .unwrap()
            .get_mut("settings")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove("button_style");
        fs::write(&path, serde_json::to_string(&value).unwrap()).unwrap();

        let store = JsonAppStore::new(Some(path.clone()));
        let data = store.load();
        assert!(data.pending_marks.is_empty());
        assert_eq!(data.settings.button_style, "icon-text");
        let persisted: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(persisted.get("pending_marks").is_some());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_inline_bodies_are_migrated_on_load() {
        let dir = std::env::temp_dir().join(format!("ry_store_migrate_{}", std::process::id()));
        let path = dir.join("data.json");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let mut article = Article::default();
        article.id = "legacy1".to_string();
        article.title = "旧数据".to_string();
        article.summary_html = Some("<p>旧摘要</p>".to_string());
        article.content_html = Some("<p>旧正文</p>".to_string());
        article.content_text = Some("旧正文".to_string());
        let legacy = AppData {
            articles: vec![article],
            ..AppData::default()
        };
        fs::write(&path, serde_json::to_string(&legacy).unwrap()).unwrap();

        let store = JsonAppStore::new(Some(path.clone()));
        let loaded = store.load();
        assert_eq!(loaded.articles[0].content_html.as_deref(), Some("<p>旧正文</p>"));
        assert!(store.content_path.exists(), "content sidecar must be created");
        let meta = fs::read_to_string(&path).unwrap();
        assert!(!meta.contains("旧正文"), "legacy body must leave data.json");
        let bodies = fs::read_to_string(store.content_path.as_path()).unwrap();
        assert!(bodies.contains("旧正文"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn bodies_live_in_sidecar_and_survive_reload() {
        let dir = std::env::temp_dir().join(format!("ry_store_body_test_{}", std::process::id()));
        let path = dir.join("data.json");
        let _ = fs::remove_dir_all(&dir);
        let store = JsonAppStore::new(Some(path.clone()));
        let mut data = store.load();
        let mut article = Article::default();
        article.id = "a1".to_string();
        article.title = "测试".to_string();
        article.content_html = Some("<p>正文内容</p>".repeat(100));
        article.content_text = Some("正文内容".repeat(100));
        data.articles.push(article);
        store.save(&data).unwrap();

        let meta = fs::read_to_string(&path).unwrap();
        let bodies = fs::read_to_string(store.content_path.as_path()).unwrap();
        assert!(!meta.contains("正文内容"), "data.json must not hold bodies");
        assert!(bodies.contains("正文内容"));

        let reloaded = JsonAppStore::new(Some(path.clone())).load();
        assert_eq!(reloaded.articles.len(), 1);
        assert_eq!(
            reloaded.articles[0].content_html,
            data.articles[0].content_html
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
