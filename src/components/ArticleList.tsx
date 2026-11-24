import { memo, useMemo } from "react";
import type { AppData, Article, Scope } from "../types";

interface Props {
  data: AppData;
  scope: Scope;
  scopeTitle: string;
  articles: Article[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  onToggleStar: (article: Article) => void;
  onFilter: (filter: string) => void;
}

const THUMB_PX: Record<number, number> = { 1: 48, 2: 72, 3: 104 };

// toLocaleString on every row is surprisingly costly with 5000 articles.
const dateCache = new Map<number, string>();
function shortDate(seconds: number): string {
  if (!seconds) return "";
  const cached = dateCache.get(seconds);
  if (cached !== undefined) return cached;
  const value = new Date(seconds * 1000).toLocaleString("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
  if (dateCache.size > 4096) dateCache.clear();
  dateCache.set(seconds, value);
  return value;
}

function ArticleList({
  data,
  scope,
  scopeTitle,
  articles,
  selectedId,
  onSelect,
  onToggleStar,
  onFilter,
}: Props) {
  const settings = data.settings;
  const thumbPx = THUMB_PX[settings.thumbnail_size] ?? 0;
  const showSnippet = settings.summary_lines > 0;
  const feedById = useMemo(
    () => new Map(data.feeds.map((f) => [f.id, f] as const)),
    [data.feeds],
  );

  const scopeLabel = scope.type === "feed"
    ? feedById.get(scope.id)?.title ?? scopeTitle
    : scope.type === "group"
      ? data.groups.find((g) => g.id === scope.id)?.name ?? scopeTitle
      : scopeTitle;

  return (
    <section
      className="article-pane"
      style={{
        fontFamily: `"${settings.article_font_family}", "Microsoft YaHei", sans-serif`,
        fontSize: settings.article_font_size,
      }}
    >
      <div className="pane-head">
        <div className="pane-title">
          <span title={scopeLabel}>{scopeLabel}</span>
          <small>{articles.length} 篇</small>
        </div>
        <div className="segmented">
          {[
            ["all", "全部"],
            ["unread", "未读"],
            ["read", "已读"],
          ].map(([value, label]) => (
            <button
              key={value}
              className={settings.filter === value ? "on" : ""}
              onClick={() => onFilter(value)}
            >
              {label}
            </button>
          ))}
        </div>
      </div>
      <div className="scroll">
        {articles.map((article) => {
          const feed = feedById.get(article.feed_id);
          const showThumb = thumbPx > 0 && !!article.thumbnail_url;
          // In large mode the image sits above the text instead of beside it.
          const largeThumb = settings.thumbnail_size >= 3 && showThumb;
          return (
            <article
              key={article.id}
              className={`article-row${article.is_read ? " read" : ""}${selectedId === article.id ? " selected" : ""}${largeThumb ? " thumb-large" : ""}`}
              onClick={() => onSelect(article.id)}
            >
              {showThumb && (
                <img
                  className="thumb"
                  src={article.thumbnail_url ?? ""}
                  alt=""
                  width={thumbPx}
                  height={thumbPx}
                  style={largeThumb ? undefined : { width: thumbPx, height: thumbPx }}
                  loading="lazy"
                />
              )}
              <div className="body">
                <div
                  className="title"
                  style={{ WebkitLineClamp: 2 }}
                  title={article.title}
                >
                  {article.title || "(无标题)"}
                </div>
                <div className="meta">
                  <span className="source">{feed?.title ?? ""}</span>
                  <span>{shortDate(article.published)}</span>
                  {article.author && <span>{article.author}</span>}
                </div>
                {showSnippet && (
                  <div
                    className="snippet"
                    style={{ WebkitLineClamp: settings.summary_lines }}
                  >
                    {article.snippet || "（无摘要）"}
                  </div>
                )}
              </div>
              <button
                className={`star${article.is_starred ? " on" : ""}`}
                title={article.is_starred ? "取消星标" : "加星标"}
                onClick={(e) => {
                  e.stopPropagation();
                  onToggleStar(article);
                }}
              >
                {article.is_starred ? "★" : "☆"}
              </button>
            </article>
          );
        })}
        {articles.length === 0 && (
          <div className="empty">
            <div className="big">📭</div>
            <div>这里没有文章</div>
            <div className="hint">换个分类，或点击工具栏「同步」拉取新文章</div>
          </div>
        )}
      </div>
    </section>
  );
}

export default memo(ArticleList);
