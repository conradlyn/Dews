//! End-to-end protocol test against a local mock FreshRSS server.
//! Mirrors tests/GReaderProtocolTest/Program.cs from the WPF edition: strict
//! checks on route prefix, Authorization header, token and form parameters.

use crate::data::greader::{GoogleReaderClient, STARRED_TAG};
use crate::data::model::{Account, AccountKind};
use crate::data::store::SecretProtector;
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const EXPECTED_AUTH: &str = "GoogleLogin auth=tester/A1B2C3";
const EXPECTED_TOKEN: &str = "T012345678901234567890123456789012345678901234567890123456789";

type Seen = Arc<Mutex<VecDeque<String>>>;

fn mock_server(port: u16, seen: Seen) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let listener = TcpListener::bind(format!("127.0.0.1:{port}")).unwrap();
        listener.set_nonblocking(true).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while std::time::Instant::now() < deadline {
            let mut stream = match listener.accept() {
                Ok((s, _)) => s,
                Err(_) => {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
            };
            // On Windows the accepted socket inherits the listener's
            // non-blocking flag; without this, read() returns WouldBlock and
            // we drop the connection before the request arrives.
            stream.set_nonblocking(false).ok();
            let mut buf = vec![0u8; 16384];
            let n = stream.read(&mut buf).unwrap_or(0);
            if n == 0 {
                continue;
            }
            let request = String::from_utf8_lossy(&buf[..n]).to_string();
            let first_line = request.lines().next().unwrap_or("");
            let mut parts = first_line.split_whitespace();
            let method = parts.next().unwrap_or("");
            let target = parts.next().unwrap_or("");
            let (path, query) = target.split_once('?').unwrap_or((target, ""));

            let header = |name: &str| -> String {
                let lower = name.to_ascii_lowercase();
                request
                    .lines()
                    .find(|l| l.to_ascii_lowercase().starts_with(&format!("{lower}:")))
                    .and_then(|l| l.split_once(':'))
                    .map(|(_, v)| v.trim().to_string())
                    .unwrap_or_default()
            };
            let auth = header("Authorization");
            let body = request.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or_default();
            seen.lock().unwrap().push_back(path.to_string());

            let (status, content_type, response_body) = if path == "/api/greader.php/accounts/ClientLogin"
                && method == "POST"
            {
                assert!(body.contains("Email=tester"), "ClientLogin 提交用户名");
                assert!(body.contains("Passwd=api-pass"), "ClientLogin 提交 API 密码");
                assert!(body.contains("service=reader"), "ClientLogin service=reader");
                assert!(body.contains("accountType=GOOGLE"), "ClientLogin accountType=GOOGLE");
                (200, "text/plain", "SID=tester/A1B2C3\nLSID=null\nAuth=tester/A1B2C3\n".to_string())
            } else if auth != EXPECTED_AUTH {
                (401, "text/plain", "Unauthorized!".to_string())
            } else {
                match path {
                    "/api/greader.php/reader/api/0/token" => (200, "text/plain", format!("{EXPECTED_TOKEN}\n")),
                    "/api/greader.php/reader/api/0/subscription/list" => {
                        assert!(query.contains("output=json"), "订阅列表携带 output=json");
                        (
                            200,
                            "application/json",
                            r#"{"subscriptions":[{"id":"feed/42","title":"测试订阅","htmlUrl":"https://example.com","categories":[{"id":"user/-/label/科技","label":"科技"}]}]}"#.to_string(),
                        )
                    }
                    "/api/greader.php/reader/api/0/stream/contents/feed/42" => {
                        assert!(query.contains("n=20"), "流内容携带条目数参数");
                        (
                            200,
                            "application/json",
                            r#"{"items":[{"id":"123456789","title":"协议测试文章","published":1763846400,"alternate":[{"href":"https://example.com/a"}],"summary":{"content":"<p>摘要</p>"},"author":"Conrad","origin":{"streamId":"feed/42","title":"测试订阅"},"categories":["user/-/state/com.google/reading-list","user/-/state/com.google/read","user/-/state/com.google/starred"]}]}"#.to_string(),
                        )
                    }
                    "/api/greader.php/reader/api/0/edit-tag" => {
                        assert!(body.contains(&format!("T={EXPECTED_TOKEN}")), "edit-tag 携带 token");
                        assert!(body.contains("i=123456789"), "edit-tag 携带条目 ID");
                        assert!(body.contains("a=user%2F-%2Fstate%2Fcom.google%2Fstarred"), "edit-tag 携带标签");
                        (200, "text/plain", "OK".to_string())
                    }
                    "/api/greader.php/reader/api/0/subscription/quickadd" => {
                        assert!(query.contains("quickadd="), "订阅请求携带 quickadd");
                        (200, "application/json", r#"{"numResults":1,"streamId":"feed/43","streamName":"新订阅"}"#.to_string())
                    }
                    "/api/greader.php/reader/api/0/subscription/edit" => {
                        assert!(body.contains("s=feed%2F43"), "订阅编辑携带 streamId");
                        assert!(
                            body.contains(&format!("T={EXPECTED_TOKEN}")),
                            "订阅编辑携带 token"
                        );
                        if body.contains("ac=unsubscribe") {
                            (200, "text/plain", "OK".to_string())
                        } else {
                            assert!(body.contains("ac=edit"), "分类编辑使用 ac=edit");
                            assert!(
                                body.contains("a=user%2F-%2Flabel%2F%E7%A7%91%E6%8A%80"),
                                "分类写入为 user/-/label/科技：{body}"
                            );
                            (200, "text/plain", "OK".to_string())
                        }
                    }
                    _ => (400, "text/plain", "Bad Request!".to_string()),
                }
            };

            let response = format!(
                "HTTP/1.1 {status} OK\r\nContent-Type: {content_type}; charset=UTF-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream.write_all(response.as_bytes()).ok();
        }
    })
}

#[tokio::test]
async fn freshrss_protocol_end_to_end() {
    let port = 18766;
    let seen: Seen = Arc::new(Mutex::new(VecDeque::new()));
    let server = mock_server(port, seen.clone());

    let account = Account {
        id: 99,
        kind: AccountKind::FreshRSS,
        name: "协议测试".to_string(),
        endpoint: format!("http://127.0.0.1:{port}"),
        username: "tester".to_string(),
        password_protected: SecretProtector::protect(Some("api-pass")),
        is_active: true,
    };
    let mut client = GoogleReaderClient::new(account).unwrap();

    // 1. Validate
    client.validate().await.expect("ValidateAsync 成功");

    // 2. Subscriptions
    let subs = client.get_subscriptions().await.expect("订阅列表解析");
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0].stream_id, "feed/42");
    assert_eq!(subs[0].website, "https://example.com");
    assert_eq!(subs[0].category_label.as_deref(), Some("科技"));

    // 3. Stream contents + read state
    let items = client.get_stream("feed/42", 20).await.expect("流内容解析");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "123456789");
    assert!(items[0].is_read, "FreshRSS 字符串格式 categories 的已读标签必须被识别");
    assert!(items[0].is_starred, "FreshRSS 字符串格式 categories 的星标标签必须被识别");

    // 4. Mark with token
    client
        .mark("123456789", Some(STARRED_TAG), None)
        .await
        .expect("MarkAsync 成功");

    // 5. Subscribe
    let stream_id = client
        .subscribe("https://example.com/new.xml")
        .await
        .expect("SubscribeAsync 成功");
    assert_eq!(stream_id.as_deref(), Some("feed/43"));

    // 6. Assign the new subscription to a server-side category
    client
        .set_subscription_category("feed/43", "科技")
        .await
        .expect("SetSubscriptionCategoryAsync 成功");

    // 7. Unsubscribe
    client.unsubscribe("feed/43").await.expect("UnsubscribeAsync 成功");

    // 8. No legacy paths were requested
    let paths = seen.lock().unwrap().clone();
    assert!(!paths.iter().any(|p| p == "/api/greader.php/subscription/list"
        || p == "/api/greader.php/editTag"
        || p == "/api/greader.php/subscription/unsubscribe"));
    assert!(paths.iter().any(|p| p == "/api/greader.php/reader/api/0/token"));

    server.join().ok();
}

/// A two-page stream: first page declares a continuation cursor, second page
/// carries the remaining item. Verifies full synchronization, not a 300-item slice.
fn paged_mock_server(port: u16, seen: Seen) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let listener = TcpListener::bind(format!("127.0.0.1:{port}")).unwrap();
        listener.set_nonblocking(true).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
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
            let first_line = request.lines().next().unwrap_or("");
            let mut parts = first_line.split_whitespace();
            let _method = parts.next().unwrap_or("");
            let target = parts.next().unwrap_or("");
            let (path, query) = target.split_once('?').unwrap_or((target, ""));
            seen.lock().unwrap().push_back(query.to_string());

            let body = if path.ends_with("/accounts/ClientLogin") {
                "Auth=tester/A1B2C3\n".to_string()
            } else if path.ends_with("/stream/contents/feed/99") {
                if query.contains("c=page2") {
                    r#"{"items":[{"id":"third","title":"第三篇","published":3}]}"#.to_string()
                } else {
                    r#"{"items":[{"id":"first","title":"第一篇","published":1},{"id":"second","title":"第二篇","published":2}],"continuation":"page2"}"#.to_string()
                }
            } else {
                "{}".to_string()
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=UTF-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).ok();
        }
    })
}

#[tokio::test]
async fn stream_pagination_fetches_every_page() {
    let port = 18767;
    let seen: Seen = Arc::new(Mutex::new(VecDeque::new()));
    let server = paged_mock_server(port, seen.clone());
    let account = Account {
        id: 98,
        kind: AccountKind::FreshRSS,
        name: "分页测试".to_string(),
        endpoint: format!("http://127.0.0.1:{port}"),
        username: "tester".to_string(),
        password_protected: SecretProtector::protect(Some("api-pass")),
        is_active: true,
    };
    let mut client = GoogleReaderClient::new(account).unwrap();
    let items = client
        .get_stream_all("feed/99", 5000)
        .await
        .expect("分页拉取全部条目");
    let ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, vec!["first", "second", "third"]);
    let queries = seen.lock().unwrap().clone();
    assert!(
        queries.iter().any(|q| q.contains("c=page2")),
        "第二页必须携带 continuation 游标"
    );
    server.join().ok();
}
