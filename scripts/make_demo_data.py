"""Write demo data.json for stage-2 visual verification.

Run once before launching the app to see the three-column UI with real content.
After verification, delete %LOCALAPPDATA%/ReadYouTauri/data.json to start fresh.
"""

import json
import os
import time

path = os.path.join(os.environ["LOCALAPPDATA"], "ReadYouTauri", "data.json")
os.makedirs(os.path.dirname(path), exist_ok=True)

now = int(time.time())
day = 86400


def article(aid, feed, title, published, body, read=False, starred=False, thumb=None):
    return {
        "id": aid,
        "account_id": 1,
        "feed_id": feed,
        "remote_id": aid,
        "title": title,
        "author": "ReadYou 编辑部",
        "link": "https://example.com/article",
        "published": published,
        "summary_html": body,
        "content_html": body,
        "content_text": "这是一篇用于验证阅读视图的示例文章，包含标题、段落、引用和代码。",
        "is_read": read,
        "is_starred": starred,
        "is_archived": False,
        "snippet": "示例摘要：这篇文章演示了三栏布局与带 CSS 的阅读视图渲染效果，包含段落、引用块与代码样式。",
        "thumbnail_url": thumb,
    }


article_html = """<article>
<h1>Rust 与 React：新一代桌面 RSS 阅读器</h1>
<p>这是<strong>阶段 2</strong>的示例正文，用于验证阅读窗格的 <em>CSS 排版</em> 是否正常。</p>
<h2>为什么选择 Tauri</h2>
<p>Tauri 使用系统 WebView 而非打包整个 Chromium，因此应用体积更小。数据层由 Rust 编写，界面由 React 渲染，两者通过 IPC 命令通信。</p>
<blockquote><p>好的阅读体验，来自干净的排版和克制的设计。</p></blockquote>
<h2>代码示例</h2>
<pre><code>#[tauri::command]
fn bootstrap() -&gt; AppData { /* ... */ }</code></pre>
<p>正文中的<a href="https://www.rust-lang.org/">链接</a>会在系统浏览器中打开，图片会自适应宽度。</p>
<ul><li>三栏布局与可拖动分隔条</li><li>系统字体枚举</li><li>摘要行数与缩略图尺寸</li></ul>
</article>"""

simple_html = "<p>这是一篇较简短的文章，用于验证文章列表的摘要行数设置与已读状态样式。</p>"

data = {
    "accounts": [
        {
            "id": 1,
            "kind": "Local",
            "name": "本地账户",
            "endpoint": "",
            "username": "",
            "password_protected": "",
            "is_active": True,
        }
    ],
    "groups": [
        {"id": "1$default", "account_id": 1, "name": "默认分组"},
        {"id": "grp-tech", "account_id": 1, "name": "技术"},
    ],
    "feeds": [
        {
            "id": "feed-tech",
            "account_id": 1,
            "remote_id": "",
            "title": "开发者日报",
            "url": "https://example.com/feed.xml",
            "website": "https://example.com",
            "group_id": "grp-tech",
            "last_synced": None,
            "failure_count": 0,
        },
        {
            "id": "feed-design",
            "account_id": 1,
            "remote_id": "",
            "title": "设计之窗",
            "url": "https://design.example.com/rss",
            "website": "https://design.example.com",
            "group_id": "1$default",
            "last_synced": None,
            "failure_count": 0,
        },
    ],
    "articles": [
        article("a1", "feed-tech", "Rust 与 React：新一代桌面 RSS 阅读器", now - 3600, article_html, starred=True),
        article("a2", "feed-tech", "FreshRSS 的 Google Reader API 兼容性实践", now - day, simple_html),
        article("a3", "feed-design", "极简排版：让阅读回归内容本身", now - 2 * day, simple_html, read=True, thumb="https://picsum.photos/seed/readyou/200/200"),
        article("a4", "feed-design", "字体、行距与对比度：阅读体验的三个细节", now - 3 * day, simple_html, read=True),
        article("a5", "feed-tech", "WebView2 与系统组件复用的取舍", now - 4 * day, simple_html, read=True),
    ],
    "settings": {
        "dark_theme": False,
        "font_size": 16,
        "feed_font_family": "Microsoft YaHei",
        "feed_font_size": 13,
        "article_font_family": "Microsoft YaHei",
        "article_font_size": 13,
        "reading_font_family": "Microsoft YaHei",
        "summary_lines": 2,
        "thumbnail_size": 1,
        "mark_as_read_on_open": True,
        "mark_as_read_when_next": True,
        "sync_interval_minutes": 15,
        "filter": "all",
        "feed_column_width": 266,
        "article_column_width": 404,
    },
}

with open(path, "w", encoding="utf-8") as f:
    json.dump(data, f, ensure_ascii=False, indent=2)
print("wrote", path)
