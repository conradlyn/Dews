import { memo, useMemo, useState } from "react";
import type { AppSettings, Article, Feed, FeedGroup, Scope } from "../types";

interface Props {
  /** Feeds of the active account only. */
  feeds: Feed[];
  /** Groups of the active account only. */
  groups: FeedGroup[];
  /** Articles of the active account only, used for the unread counters. */
  articles: Article[];
  settings: AppSettings;
  scope: Scope;
  onScope: (scope: Scope) => void;
  onRemoveFeed: (id: string) => void;
}

function sameScope(a: Scope, b: Scope): boolean {
  if (a.type !== b.type) return false;
  if ("id" in a && "id" in b) return a.id === b.id;
  return true;
}

function FeedPane({ feeds, groups, articles, settings, scope, onScope, onRemoveFeed }: Props) {
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});
  const [failedIcons, setFailedIcons] = useState<Set<string>>(new Set());

  // FreshRSS subscription data often has no website field and feeds are
  // identified as "feed/2". Article links still carry the real site origin,
  // so derive one per feed as a fallback for favicon lookup.
  const siteOriginByFeed = useMemo(() => {
    const needed = new Set(feeds.map((f) => f.id));
    const map = new Map<string, string>();
    for (const article of articles) {
      if (needed.size === 0) break;
      if (!needed.has(article.feed_id) || !article.link) continue;
      try {
        const url = new URL(article.link);
        if (url.protocol !== "http:" && url.protocol !== "https:") continue;
        map.set(article.feed_id, url.origin);
        needed.delete(article.feed_id);
      } catch {
        // Ignore malformed article links.
      }
    }
    return map;
  }, [articles, feeds]);

  const faviconUrl = (feed: Feed): string | null => {
    for (const raw of [feed.website, feed.url, siteOriginByFeed.get(feed.id)]) {
      if (!raw) continue;
      try {
        const url = new URL(raw);
        if (url.protocol !== "http:" && url.protocol !== "https:") continue;
        return `${url.origin}/favicon.ico`;
      } catch {
        // Try the next candidate.
      }
    }
    return null;
  };

  const unreadByFeed = useMemo(() => {
    const map = new Map<string, number>();
    for (const article of articles) {
      if (article.is_read) continue;
      map.set(article.feed_id, (map.get(article.feed_id) ?? 0) + 1);
    }
    return map;
  }, [articles]);

  const unreadTotal = articles.filter((a) => !a.is_read).length;
  const starredTotal = articles.filter((a) => a.is_starred).length;

  const unreadInGroup = (groupId: string) =>
    feeds
      .filter((f) => f.group_id === groupId)
      .reduce((sum, f) => sum + (unreadByFeed.get(f.id) ?? 0), 0);

  const feedRow = (feed: Feed, nested = false) => {
    const count = unreadByFeed.get(feed.id) ?? 0;
    const icon = faviconUrl(feed);
    const showIcon = icon && !failedIcons.has(feed.id);
    return (
      <div
        key={feed.id}
        className={`feed-row${nested ? " nested" : ""}${sameScope(scope, { type: "feed", id: feed.id }) ? " selected" : ""}`}
        onClick={() => onScope({ type: "feed", id: feed.id })}
        title={feed.url}
      >
        {showIcon ? (
          <img
            className="favicon icon-img"
            src={icon}
            alt=""
            onError={() =>
              setFailedIcons((prev) => new Set(prev).add(feed.id))
            }
          />
        ) : (
          <span className="favicon">📰</span>
        )}
        <span className="label">{feed.title || "(无标题)"}</span>
        {count > 0 && <span className="count">{count}</span>}
        {feed.failure_count > 0 && <span className="count" title="同步失败">⚠</span>}
        <button
          className="remove"
          title="删除此订阅源"
          onClick={(e) => {
            e.stopPropagation();
            onRemoveFeed(feed.id);
          }}
        >
          ×
        </button>
      </div>
    );
  };

  const visibleGroups = groups.filter((g) => feeds.some((f) => f.group_id === g.id));
  const ungrouped = feeds.filter(
    (f) => !f.group_id || !groups.some((g) => g.id === f.group_id),
  );

  return (
    <aside
      className="feed-pane"
      style={{
        fontFamily: `"${settings.feed_font_family}", "Microsoft YaHei", sans-serif`,
        fontSize: settings.feed_font_size,
      }}
    >
      <div className="scroll">
        <div
          className={`feed-row${sameScope(scope, { type: "all" }) ? " selected" : ""}`}
          onClick={() => onScope({ type: "all" })}
        >
          <span className="favicon">🗂</span>
          <span className="label">全部文章</span>
          {unreadTotal > 0 && <span className="count">{unreadTotal}</span>}
        </div>
        <div
          className={`feed-row${sameScope(scope, { type: "unread" }) ? " selected" : ""}`}
          onClick={() => onScope({ type: "unread" })}
        >
          <span className="favicon">📥</span>
          <span className="label">未读</span>
          {unreadTotal > 0 && <span className="count">{unreadTotal}</span>}
        </div>
        <div
          className={`feed-row${sameScope(scope, { type: "starred" }) ? " selected" : ""}`}
          onClick={() => onScope({ type: "starred" })}
        >
          <span className="favicon">⭐</span>
          <span className="label">星标</span>
          {starredTotal > 0 && <span className="count">{starredTotal}</span>}
        </div>

        <div className="section-label">订阅源</div>
        {visibleGroups.map((group) => {
          const isCollapsed = collapsed[group.id] ?? false;
          const groupFeeds = feeds.filter((f) => f.group_id === group.id);
          const count = unreadInGroup(group.id);
          return (
            <div key={group.id}>
              <div
                className={`feed-row group${sameScope(scope, { type: "group", id: group.id }) ? " selected" : ""}`}
                onClick={() => onScope({ type: "group", id: group.id })}
              >
                <span
                  className="twisty"
                  onClick={(e) => {
                    e.stopPropagation();
                    setCollapsed((prev) => ({ ...prev, [group.id]: !isCollapsed }));
                  }}
                >
                  {isCollapsed ? "▶" : "▼"}
                </span>
                <span className="label">{group.name}</span>
                {count > 0 && <span className="count">{count}</span>}
              </div>
              {!isCollapsed && groupFeeds.map((feed) => feedRow(feed, true))}
            </div>
          );
        })}
        {ungrouped.map((feed) => feedRow(feed))}

        {feeds.length === 0 && (
          <div className="empty">
            <div className="big">📡</div>
            <div>还没有订阅源</div>
            <div className="hint">点击工具栏「添加订阅」开始，或先同步 FreshRSS 账户</div>
          </div>
        )}
      </div>
    </aside>
  );
}

export default memo(FeedPane);
