import { memo } from "react";
import type { Article } from "../types";

interface Props {
  article: Article | null;
  feedTitle: string;
  html: string;
  fetching: boolean;
  onFetchFullText: (article: Article) => void;
  onOpenOriginal: (url: string) => void;
  onToggleStar: (article: Article) => void;
}

function ReadingPane({
  article,
  feedTitle,
  html,
  fetching,
  onFetchFullText,
  onOpenOriginal,
  onToggleStar,
}: Props) {
  if (!article) {
    return (
      <section className="reading-pane">
        <div className="empty">
          <div className="big">📖</div>
          <div>选择一篇文章开始阅读</div>
          <div className="hint">支持 RSS / Atom 与 FreshRSS（Google Reader API）</div>
        </div>
      </section>
    );
  }

  const date = article.published
    ? new Date(article.published * 1000).toLocaleString("zh-CN", {
        year: "numeric",
        month: "long",
        day: "numeric",
        hour: "2-digit",
        minute: "2-digit",
      })
    : "";

  return (
    <section className="reading-pane">
      <div className="reading-head">
        <h2>{article.title || "(无标题)"}</h2>
        <div className="reading-meta">
          <span>{feedTitle}</span>
          {date && <span>· {date}</span>}
          {article.author && <span>· {article.author}</span>}
          <div className="reading-actions">
            <button
              className="tool-btn"
              disabled={fetching || !article.link}
              onClick={() => onFetchFullText(article)}
              title={article.link ? "抓取原文网页并提取正文" : "这篇文章没有原文链接"}
            >
              {fetching ? "获取中…" : "↧ 获取全文"}
            </button>
            <button
              className="tool-btn"
              onClick={() => onToggleStar(article)}
              title={article.is_starred ? "取消星标" : "加星标"}
            >
              {article.is_starred ? "★ 已星标" : "☆ 星标"}
            </button>
            <button
              className="tool-btn primary"
              disabled={!article.link}
              onClick={() => article.link && onOpenOriginal(article.link)}
              title="在浏览器中打开原文"
            >
              打开原文 ↗
            </button>
          </div>
        </div>
      </div>
      <iframe
        key={article?.id ?? "empty"}
        className="reading-frame"
        title="文章正文"
        srcDoc={html}
      />
    </section>
  );
}

export default memo(ReadingPane);
