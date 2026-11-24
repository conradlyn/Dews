import { memo, useMemo, useState } from "react";
import type { AppData, Scope } from "../types";

interface Props {
  data: AppData;
  scope: Scope;
  onScope: (scope: Scope) => void;
  onRemoveFeed: (id: string) => void;
}

function sameScope(a: Scope, b: Scope): boolean {
  if (a.type !== b.type) return false;
  if ("id" in a && "id" in b) return a.id === b.id;
  return true;
}

function FeedPane({ data, scope, onScope, onRemoveFeed }: Props) {
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});

  const unreadByFeed = useMemo(() => {
    const map = new Map<string, number>();
    for (const article of data.articles) {
      if (article.is_read) continue;
      map.set(article.feed_id, (map.get(article.feed_id) ?? 0) + 1);
    }
    return map;
  }, [data.articles]);

  const unreadTotal = data.articles.filter((a) => !a.is_read).length;
  const starredTotal = data.articles.filter((a) => a.is_starred).length;

  const unreadInGroup = (groupId: string) =>
    data.feeds
      .filter((f) => f.group_id === groupId)
      .reduce((sum, f) => sum + (unreadByFeed.get(f.id) ?? 0), 0);

  const feedRow = (feed: (typeof data.feeds)[number], nested = false) => {
    const count = unreadByFeed.get(feed.id) ?? 0;
    return (
      <div
        key={feed.id}
        className={`feed-row${nested ? " nested" : ""}${sameScope(scope, { type: "feed", id: feed.id }) ? " selected" : ""}`}
        onClick={() => onScope({ type: "feed", id: feed.id })}
        title={feed.url}
      >
        <span className="favicon">📰</span>
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

  const groups = data.groups.filter((g) => data.feeds.some((f) => f.group_id === g.id));
  const ungrouped = data.feeds.filter(
    (f) => !f.group_id || !data.groups.some((g) => g.id === f.group_id),
  );

  return (
    <aside
      className="feed-pane"
      style={{
        fontFamily: `"${data.settings.feed_font_family}", "Microsoft YaHei", sans-serif`,
        fontSize: data.settings.feed_font_size,
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
        {groups.map((group) => {
          const isCollapsed = collapsed[group.id] ?? false;
          const feeds = data.feeds.filter((f) => f.group_id === group.id);
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
              {!isCollapsed && feeds.map((feed) => feedRow(feed, true))}
            </div>
          );
        })}
        {ungrouped.map((feed) => feedRow(feed))}

        {data.feeds.length === 0 && (
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
