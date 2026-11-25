import { memo, useEffect, useMemo, useRef, useState } from "react";
import type { AppData, Article, Scope } from "../types";
import Icon from "./icons";

interface Props {
  data: AppData;
  scope: Scope;
  scopeTitle: string;
  articles: Article[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  onToggleStar: (article: Article) => void;
  onFilter: (filter: string) => void;
  onMarkAllRead: () => void;
}

const THUMB_PX: Record<number, number> = { 1: 48, 2: 72, 3: 104 };
const PAGE_SIZE = 150;

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
  onMarkAllRead,
}: Props) {
  const settings = data.settings;
  const thumbPx = THUMB_PX[settings.thumbnail_size] ?? 0;
  const showSnippet = settings.summary_lines > 0;
  const hasUnread = useMemo(() => articles.some((a) => !a.is_read), [articles]);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const [limit, setLimit] = useState(PAGE_SIZE);

  // Reset the rendered window whenever the visible article set changes, so a
  // new scope/filter starts fast instead of inheriting a huge previous limit.
  const scopeKey = `${scope.type}:${"id" in scope ? scope.id : ""}|${settings.filter}|${articles.length}`;
  useEffect(() => {
    setLimit(PAGE_SIZE);
    if (scrollRef.current) scrollRef.current.scrollTop = 0;
  }, [scopeKey]);

  const feedById = useMemo(
    () => new Map(data.feeds.map((f) => [f.id, f] as const)),
    [data.feeds],
  );

  const scopeLabel = scope.type === "feed"
    ? feedById.get(scope.id)?.title ?? scopeTitle
    : scope.type === "group"
      ? data.groups.find((g) => g.id === scope.id)?.name ?? scopeTitle
      : scopeTitle;

  // Incrementally reveal rows as the user nears the bottom. Rendering all
  // 7000+ rows at once creates tens of thousands of DOM nodes and freezes the
  // WebView on launch; this keeps the initial mount bounded.
  const onScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    if (el.scrollTop + el.clientHeight >= el.scrollHeight - 600) {
      setLimit((prev) => Math.min(prev + PAGE_SIZE, articles.length));
    }
  };

  const visible = useMemo(() => articles.slice(0, limit), [articles, limit]);

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
        <div className="pane-actions">
          <div className="segmented">
            {([
              ["all", "全部", "all"],
              ["unread", "未读", "unread"],
              ["read", "已读", "read"],
            ] as const).map(([value, label, icon]) => (
              <button
                key={value}
                className={`filter-chip${settings.filter === value ? " on" : ""}`}
                onClick={() => onFilter(value)}
                title={label}
              >
                <Icon name={icon} />
                <span>{label}</span>
              </button>
            ))}
          </div>
          <button
            className="filter-chip mark-all-read"
            onClick={onMarkAllRead}
            disabled={!hasUnread}
            title="将当前列表中的文章全部标为已读"
          >
            <Icon name="check-all" />
            <span>全部已读</span>
          </button>
        </div>
      </div>
      <div className="scroll" ref={scrollRef} onScroll={onScroll}>
        {visible.map((article) => {
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
        {limit < articles.length && (
          <div className="load-more">已显示 {limit} / {articles.length} 篇，向下滚动加载更多</div>
        )}
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
