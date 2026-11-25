import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties, PointerEvent as ReactPointerEvent } from "react";
import { api } from "./api";
import rssIcon from "./assets/rss.png";
import ArticleList from "./components/ArticleList";
import Icon from "./components/icons";
import FeedPane from "./components/FeedPane";
import ReadingPane from "./components/ReadingPane";
import SettingsDialog from "./components/SettingsDialog";
import type { AppData, AppSettings, Article, Scope, SyncPlan } from "./types";
import "./styles.css";

type Dialog = "settings" | "addFeed" | null;

type SyncStage = "subscriptions" | "articles" | "done";
interface SyncState {
  stage: SyncStage;
  plan: SyncPlan | null;
  index: number;
  total: number;
  current: string;
  articles: number;
  errors: string[];
}

export default function App() {
  const [data, setData] = useState<AppData | null>(null);
  const [fonts, setFonts] = useState<string[]>([]);
  const [scope, setScope] = useState<Scope>({ type: "all" });
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [html, setHtml] = useState("");
  const [query, setQuery] = useState("");
  const [searchResult, setSearchResult] = useState<{ query: string; ids: string[] } | null>(null);
  const [toast, setToast] = useState<{ text: string; error: boolean } | null>(null);
  const [busy, setBusy] = useState(false);
  const [fetchingId, setFetchingId] = useState<string | null>(null);
  const [syncState, setSyncState] = useState<SyncState | null>(null);
  const syncingRef = useRef(false);
  const [dialog, setDialog] = useState<Dialog>(null);
  const [feedUrl, setFeedUrl] = useState("");
  const [confirmFeedId, setConfirmFeedId] = useState<string | null>(null);
  // All splitter drag state lives in refs so no re-render can tear down the
  // active drag. `dragging` only drives the visual highlight.
  type DragState = {
    which: "feed" | "article";
    startX: number;
    startW: number;
    target: HTMLElement;
    pointerId: number;
  };
  const dragRef = useRef<DragState | null>(null);
  const dragWidthRef = useRef<number | null>(null);
  const workspaceRef = useRef<HTMLDivElement | null>(null);
  const rafRef = useRef<number | null>(null);
  const [dragging, setDragging] = useState<"feed" | "article" | null>(null);
  const toastTimer = useRef<number | null>(null);

  const notify = useCallback((text: string, error = false) => {
    setToast({ text, error });
    if (toastTimer.current !== null) window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setToast(null), 3600);
  }, []);

  // Initial load: bootstrap + installed fonts.
  useEffect(() => {
    let cancelled = false;
    void Promise.all([api.bootstrap(), api.getFonts()])
      .then(([loaded, installed]) => {
        if (cancelled) return;
        setData(loaded);
        setFonts(installed);
      })
      .catch((e) => {
        if (!cancelled) notify(String(e), true);
      });
    return () => {
      cancelled = true;
    };
  }, [notify]);

  // Full-text search runs in Rust; debounce keystrokes so a 100 MB library is
  // never scanned more than once per pause.
  useEffect(() => {
    const term = query.trim();
    if (!term) {
      setSearchResult(null);
      return;
    }
    let cancelled = false;
    const timer = window.setTimeout(() => {
      void api
        .searchArticles(term)
        .then((ids) => {
          if (!cancelled) setSearchResult({ query: term, ids });
        })
        .catch((e) => {
          if (!cancelled) notify(String(e), true);
        });
    }, 200);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [query, notify]);

  // Persist settings after the user stops dragging / typing.
  useEffect(() => {
    if (!data) return;
    const snapshot = data.settings;
    const timer = window.setTimeout(() => {
      void api.saveSettings(snapshot).catch((e) => notify(String(e), true));
    }, 350);
    return () => window.clearTimeout(timer);
  }, [data?.settings, notify]);

  const patchSettings = useCallback((patch: Partial<AppSettings>) => {
    setData((prev) =>
      prev ? { ...prev, settings: { ...prev.settings, ...patch } } : prev,
    );
  }, []);

  const refresh = useCallback(async () => {
    const fresh = await api.bootstrap();
    setData((prev) => (prev ? { ...fresh, settings: prev.settings } : fresh));
  }, []);

  const articles = useMemo(() => {
    if (!data) return [];
    let list = data.articles.slice();
    switch (scope.type) {
      case "unread":
        list = list.filter((a) => !a.is_read);
        break;
      case "starred":
        list = list.filter((a) => a.is_starred);
        break;
      case "feed":
        list = list.filter((a) => a.feed_id === scope.id);
        break;
      case "group": {
        const ids = new Set(
          data.feeds.filter((f) => f.group_id === scope.id).map((f) => f.id),
        );
        list = list.filter((a) => ids.has(a.feed_id));
        break;
      }
      default:
        break;
    }
    const filter = data.settings.filter;
    if (filter === "unread") list = list.filter((a) => !a.is_read);
    if (filter === "read") list = list.filter((a) => a.is_read);
    const term = query.trim().toLowerCase();
    if (term) {
      if (searchResult?.query === query.trim()) {
        const ids = new Set(searchResult.ids);
        list = list.filter((a) => ids.has(a.id));
      } else {
        // Instant title/snippet feedback while Rust scans full article text.
        list = list.filter(
          (a) =>
            a.title.toLowerCase().includes(term) ||
            (a.snippet ?? "").toLowerCase().includes(term),
        );
      }
    }
    return list.sort((a, b) => b.published - a.published);
  }, [data, scope, query, searchResult]);

  const articleById = useMemo(
    () => new Map((data?.articles ?? []).map((a) => [a.id, a] as const)),
    [data?.articles],
  );
  const selected = selectedId ? articleById.get(selectedId) ?? null : null;
  const feedTitle = selected
    ? data?.feeds.find((f) => f.id === selected.feed_id)?.title ?? ""
    : "";
  const scopeTitle =
    scope.type === "all"
      ? "全部文章"
      : scope.type === "unread"
        ? "未读"
        : scope.type === "starred"
          ? "星标"
          : "";

  // Render the article in an isolated iframe with Rust-injected reading CSS.
  useEffect(() => {
    if (!selectedId) {
      setHtml("");
      return;
    }
    let cancelled = false;
    void api
      .getArticleHtml(selectedId, data?.settings)
      .then((page) => {
        if (!cancelled) setHtml(page);
      })
      .catch((e) => {
        if (cancelled) return;
        setHtml("");
        notify(String(e), true);
      });
    return () => {
      cancelled = true;
    };
  }, [
    selectedId,
    data?.settings.dark_theme,
    data?.settings.font_size,
    data?.settings.reading_font_family,
    data?.settings.reading_margin,
    data?.settings.reading_line_height,
    data?.settings.reading_indent,
    notify,
  ]);

  // Links inside the article are forwarded to the OS browser by Rust.
  useEffect(() => {
    const onMessage = (event: MessageEvent) => {
      const payload = event.data as { __readyou_open?: unknown } | null;
      if (payload && typeof payload.__readyou_open === "string") {
        void api
          .openExternal(payload.__readyou_open)
          .catch((e) => notify(String(e), true));
      }
    };
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, [notify]);

  // Column splitters use Pointer Events with pointer capture: once captured,
  // every move/up is delivered to the splitter even if the cursor leaves the
  // window or an iframe, so the drag can never get "stuck". All listeners are
  // bound once; the active drag is read from a ref, never from state.
  const finishDrag = useCallback(() => {
    if (!dragRef.current) return;
    const current = dragRef.current;
    if (rafRef.current !== null) {
      window.cancelAnimationFrame(rafRef.current);
      rafRef.current = null;
    }
    const width = dragWidthRef.current;
    if (width !== null) {
      const prop = current.which === "feed" ? "--feed-width" : "--article-width";
      workspaceRef.current?.style.setProperty(prop, `${width}px`);
      if (current.which === "feed") patchSettings({ feed_column_width: width });
      else patchSettings({ article_column_width: width });
    }
    // Release the pointer capture explicitly and restore the cursor. Without
    // this the captured element can keep the col-resize cursor after release.
    try {
      if (current.target.hasPointerCapture(current.pointerId)) {
        current.target.releasePointerCapture(current.pointerId);
      }
    } catch {
      /* capture may already be gone */
    }
    dragRef.current = null;
    dragWidthRef.current = null;
    setDragging(null);
    document.body.style.cursor = "";
    document.body.style.userSelect = "";
    // Freeze the splitter cursor as a normal arrow until the mouse moves.
    document.body.classList.add("splitter-idle");
  }, [patchSettings]);

  useEffect(() => {
    const onMove = (event: PointerEvent) => {
      const d = dragRef.current;
      if (!d) {
        // A real cursor move after release restores the resize affordance.
        document.body.classList.remove("splitter-idle");
        return;
      }
      const max = d.which === "feed" ? 520 : 900;
      const next = Math.round(
        Math.max(180, Math.min(max, d.startW + (event.clientX - d.startX))),
      );
      dragWidthRef.current = next;
      if (rafRef.current !== null) return;
      rafRef.current = window.requestAnimationFrame(() => {
        rafRef.current = null;
        const w = dragWidthRef.current;
        const active = dragRef.current;
        if (w !== null && active) {
          const prop = active.which === "feed" ? "--feed-width" : "--article-width";
          workspaceRef.current?.style.setProperty(prop, `${w}px`);
        }
      });
    };
    const onUp = () => finishDrag();
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onUp);
    window.addEventListener("blur", onUp);
    document.addEventListener("visibilitychange", onUp);
    return () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onUp);
      window.removeEventListener("blur", onUp);
      document.removeEventListener("visibilitychange", onUp);
      if (rafRef.current !== null) {
        window.cancelAnimationFrame(rafRef.current);
        rafRef.current = null;
      }
    };
  }, [finishDrag]);

  const startDrag =
    (which: "feed" | "article") =>
    (event: ReactPointerEvent<HTMLDivElement>) => {
      if (event.button !== 0) return;
      event.preventDefault();
      const startW =
        which === "feed"
          ? settings.feed_column_width
          : settings.article_column_width;
      dragRef.current = {
        which,
        startX: event.clientX,
        startW,
        target: event.currentTarget,
        pointerId: event.pointerId,
      };
      dragWidthRef.current = startW;
      setDragging(which);
      // Capture the pointer on the splitter so up/move events are guaranteed.
      event.currentTarget.setPointerCapture(event.pointerId);
      document.body.style.cursor = "col-resize";
      document.body.style.userSelect = "none";
    };

  const handleSelect = useCallback((id: string) => {
    if (id === selectedId || !data) return;
    const previous = selectedId ? articleById.get(selectedId) : undefined;
    const next = articleById.get(id);
    const markPrevious =
      data.settings.mark_as_read_when_next && !!previous && !previous.is_read;
    const markNext =
      data.settings.mark_as_read_on_open && !!next && !next.is_read;
    setSelectedId(id);
    if (markPrevious || markNext) {
      setData((current) =>
        current
          ? {
              ...current,
              articles: current.articles.map((a) => {
                if (markPrevious && a.id === previous?.id) return { ...a, is_read: true };
                if (markNext && a.id === next?.id) return { ...a, is_read: true };
                return a;
              }),
            }
          : current,
      );
      if (markPrevious && previous) {
        void api.markArticle(previous.id, true).catch((e) => {
          notify(String(e), true);
          void refresh();
        });
      }
      if (markNext && next) {
        void api.markArticle(next.id, true).catch((e) => {
          notify(String(e), true);
          void refresh();
        });
      }
    }
  }, [selectedId, data, articleById, notify, refresh]);

  const toggleStar = useCallback((article: Article) => {
    const starred = !article.is_starred;
    setData((current) =>
      current
        ? {
            ...current,
            articles: current.articles.map((a) =>
              a.id === article.id ? { ...a, is_starred: starred } : a,
            ),
          }
        : current,
    );
    void api.markArticle(article.id, undefined, starred).catch((e) => {
      notify(String(e), true);
      void refresh();
    });
  }, [notify, refresh]);

  // Mark the current visible list as read in one backend call. The Rust side
  // queues remote marks instead of raising a toast per article.
  const markAllRead = useCallback(() => {
    const ids = articles
      .filter((article) => !article.is_read)
      .map((article) => article.id);
    if (ids.length === 0) {
      notify("当前列表没有未读文章");
      return;
    }
    const wanted = new Set(ids);
    setData((current) =>
      current
        ? {
            ...current,
            articles: current.articles.map((a) =>
              wanted.has(a.id) ? { ...a, is_read: true } : a,
            ),
          }
        : current,
    );
    void api
      .markArticlesRead(ids)
      .then((report) => {
        const pending = report.pending
          ? `（${report.pending} 篇等待远程同步）`
          : "";
        notify(`已将 ${report.marked} 篇标为已读${pending}`);
      })
      .catch((e) => {
        notify(String(e), true);
        void refresh();
      });
  }, [articles, notify, refresh]);

  // Download the original page and extract its full text. The Rust command
  // persists the result, so we update the iframe and reload article data.
  const fetchFullText = useCallback(async (article: Article) => {
    if (fetchingId) return;
    setFetchingId(article.id);
    try {
      const page = await api.fetchFullText(article.id, data?.settings);
      // Only apply if the user hasn't moved to another article meanwhile.
      if (selectedId === article.id) setHtml(page);
      await refresh();
      notify("已获取全文");
    } catch (e) {
      notify(String(e), true);
    } finally {
      setFetchingId(null);
    }
  }, [fetchingId, selectedId, notify, refresh, data?.settings]);

  const openOriginal = useCallback((url: string) => {
    void api.openExternal(url).catch((e) => notify(String(e), true));
  }, [notify]);

  const runSync = useCallback(
    async (showToast = true) => {
      if (syncingRef.current) return;
      syncingRef.current = true;
      setBusy(true);
      setSyncState({
        stage: "subscriptions",
        plan: null,
        index: 0,
        total: 0,
        current: "",
        articles: 0,
        errors: [],
      });
      const errors: string[] = [];
      let totalArticles = 0;
      let plan: SyncPlan | null = null;
      try {
        plan = await api.syncSubscriptions();
        errors.push(...plan.errors);
        setSyncState({
          stage: "articles",
          plan,
          index: 0,
          total: plan.feeds.length,
          current: "",
          articles: 0,
          errors: [...errors],
        });
        // 订阅源与分类已经入库：先重绘最左侧栏，再开始逐源拉文章。
        await refresh();
        for (let i = 0; i < plan.feeds.length; i++) {
          const feed = plan.feeds[i];
          setSyncState((prev) =>
            prev
              ? { ...prev, index: i + 1, current: feed.title, errors: [...errors] }
              : prev,
          );
          try {
            const result = await api.syncFeedArticles(feed.id);
            totalArticles += result.articles;
            if (result.error) errors.push(`${feed.title}：${result.error}`);
            await refresh();
          } catch (e) {
            errors.push(`${feed.title}：${String(e)}`);
          }
          setSyncState((prev) =>
            prev
              ? { ...prev, articles: totalArticles, errors: [...errors] }
              : prev,
          );
        }
        setSyncState({
          stage: "done",
          plan,
          index: plan.feeds.length,
          total: plan.feeds.length,
          current: "",
          articles: totalArticles,
          errors: [...errors],
        });
        if (showToast) {
          notify(
            errors.length
              ? `同步完成（${errors.length} 项失败）：${plan.feeds.length} 个订阅源，${totalArticles} 篇新文章`
              : `同步完成：${plan.feeds.length} 个订阅源，${totalArticles} 篇新文章`,
            errors.length > 0,
          );
        }
      } catch (e) {
        errors.push(String(e));
        setSyncState((prev) =>
          prev ? { ...prev, stage: "done", errors: [...errors] } : prev,
        );
        if (showToast) notify(String(e), true);
      } finally {
        syncingRef.current = false;
        setBusy(false);
        window.setTimeout(() => setSyncState(null), 2600);
      }
    },
    [notify, refresh],
  );

  useEffect(() => {
    const minutes = data?.settings.sync_interval_minutes ?? 0;
    if (!minutes) return;
    const timer = window.setInterval(() => {
      void runSync(false);
    }, minutes * 60_000);
    return () => window.clearInterval(timer);
  }, [data?.settings.sync_interval_minutes, runSync]);

  const submitFeed = async () => {
    if (!feedUrl.trim()) return;
    setBusy(true);
    try {
      const message = await api.addFeed(feedUrl);
      notify(message);
      setFeedUrl("");
      setDialog(null);
      await refresh();
    } catch (e) {
      notify(String(e), true);
    } finally {
      setBusy(false);
    }
  };

  const removeFeed = async (id: string) => {
    if (confirmFeedId !== id) {
      setConfirmFeedId(id);
      notify("再次点击删除按钮以确认删除该订阅源");
      return;
    }
    setConfirmFeedId(null);
    try {
      await api.removeFeed(id);
      if (selected && selected.feed_id === id) setSelectedId(null);
      await refresh();
      notify("订阅源已删除");
    } catch (e) {
      notify(String(e), true);
    }
  };

  const importOpml = async (file: File) => {
    setBusy(true);
    try {
      const message = await api.importOpml(await file.text());
      notify(message);
      await refresh();
    } catch (e) {
      notify(String(e), true);
    } finally {
      setBusy(false);
    }
  };

  const exportOpml = async () => {
    try {
      const xml = await api.exportOpml();
      await navigator.clipboard.writeText(xml);
      notify("OPML 已复制到剪贴板");
    } catch (e) {
      notify(String(e), true);
    }
  };

  if (!data) {
    return <div className="loading">正在加载 dews…</div>;
  }

  const settings = data.settings;
  const compactButtons = settings.button_style === "icon";
  const shellClass = `app-shell${settings.dark_theme ? " dark" : ""}`;
  const workspaceStyle = {
    "--feed-width": `${settings.feed_column_width}px`,
    "--article-width": `${settings.article_column_width}px`,
  } as CSSProperties;

  return (
    <div className={shellClass}>
      <header className="toolbar">
        <div className="brand">
          <img className="brand-mark" src={rssIcon} alt="" />
          <span>dews</span>
        </div>
        <button
          className={`tool-btn quiet${compactButtons ? " icon-only" : ""}`}
          onClick={() => setDialog("addFeed")}
          title="添加订阅"
        >
          <Icon name="add-feed" size={14} />
          {!compactButtons && <span>添加订阅</span>}
        </button>
        <button
          className={`tool-btn quiet${compactButtons ? " icon-only" : ""}`}
          disabled={busy}
          onClick={() => void runSync()}
          title={busy ? "同步中…" : "同步"}
        >
          <Icon
            name="sync"
            size={14}
            className={busy ? "spinning" : undefined}
          />
          {!compactButtons && <span>{busy ? "同步中…" : "同步"}</span>}
        </button>
        <input
          className="search-box"
          type="text"
          placeholder="搜索标题 / 摘要 / 正文"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <div className="toolbar-spacer" />
        <span className="account-chip">
          {data.accounts.find((a) => a.is_active)?.name || "本地账户"}
        </span>
        <button
          className={`tool-btn quiet${compactButtons ? " icon-only" : ""}`}
          onClick={() => setDialog("settings")}
          title="设置"
        >
          <Icon name="settings" size={14} />
          {!compactButtons && <span>设置</span>}
        </button>
      </header>

      <div className="workspace" ref={workspaceRef} style={workspaceStyle}>
        <FeedPane
          data={data}
          scope={scope}
          onScope={setScope}
          onRemoveFeed={(id) => void removeFeed(id)}
        />
        <div
          className={`splitter${dragging === "feed" ? " dragging" : ""}`}
          onPointerDown={startDrag("feed")}
        />
        <ArticleList
          data={data}
          scope={scope}
          scopeTitle={scopeTitle}
          articles={articles}
          selectedId={selectedId}
          onSelect={handleSelect}
          onToggleStar={toggleStar}
          onFilter={(filter) => patchSettings({ filter })}
          onMarkAllRead={markAllRead}
        />
        <div
          className={`splitter${dragging === "article" ? " dragging" : ""}`}
          onPointerDown={startDrag("article")}
        />
        <ReadingPane
          article={selected}
          settings={settings}
          feedTitle={feedTitle}
          html={html}
          fetching={!!selected && fetchingId === selected.id}
          onFetchFullText={(a) => void fetchFullText(a)}
          onOpenOriginal={openOriginal}
          onToggleStar={toggleStar}
        />
      </div>

      {syncState && (
        <div className="sync-statusbar">
          <span className={`sync-dot${syncState.stage === "done" ? " done" : ""}`} />
          <span className="sync-text">
            {syncState.stage === "subscriptions"
              ? "正在获取全部订阅源与分类…"
              : syncState.stage === "articles"
                ? `正在同步文章 ${syncState.index}/${syncState.total}${syncState.current ? `：${syncState.current}` : ""}`
                : syncState.errors.length
                  ? "同步完成（部分订阅源失败）"
                  : "同步完成"}
          </span>
          {syncState.errors.length > 0 && (
            <span
              className="sync-errors"
              title={syncState.errors.join("\n")}
            >
              ⚠ {syncState.errors.length} 项失败
            </span>
          )}
          <span className="sync-articles">新增 {syncState.articles} 篇</span>
          <div className="sync-track">
            <div
              className="sync-fill"
              style={{
                width:
                  syncState.stage === "subscriptions"
                    ? "6%"
                    : syncState.stage === "done"
                      ? "100%"
                      : `${6 + (syncState.total ? (syncState.index - 1) / syncState.total : 0) * 94}%`,
              }}
            />
          </div>
        </div>
      )}

      {dialog === "settings" && (
        <SettingsDialog
          settings={settings}
          fonts={fonts}
          accounts={data.accounts}
          activeAccount={data.accounts.find((a) => a.is_active) ?? null}
          onAccounts={(accounts) => setData((prev) => (prev ? { ...prev, accounts } : prev))}
          onAccountSaved={(accounts, savedId) => {
            const saved = accounts.find((a) => a.id === savedId);
            if (saved?.is_active && saved.kind !== "Local") void runSync();
          }}
          groups={data.groups}
          feeds={data.feeds}
          onChange={patchSettings}
          onCreateGroup={async (name) => {
            await api.createGroup(name);
            await refresh();
          }}
          onRenameGroup={async (id, name) => {
            await api.renameGroup(id, name);
            await refresh();
          }}
          onReorderGroups={async (ids) => {
            await api.reorderGroups(ids);
            await refresh();
          }}
          onDeleteGroup={async (id) => {
            await api.deleteGroup(id);
            await refresh();
          }}
          onSetFeedGroup={async (feedId, groupId) => {
            await api.setFeedGroup(feedId, groupId);
            await refresh();
          }}
          onImportOpml={(file) => void importOpml(file)}
          onExportOpml={() => void exportOpml()}
          notify={notify}
          onClose={() => setDialog(null)}
        />
      )}
      {dialog === "addFeed" && (
        <div className="modal-backdrop" onMouseDown={() => setDialog(null)}>
          <div className="modal narrow" onMouseDown={(e) => e.stopPropagation()}>
            <div className="modal-head">
              <span>添加订阅</span>
              <button className="tool-btn icon" onClick={() => setDialog(null)}>×</button>
            </div>
            <div className="modal-body">
              <div className="form-grid">
                <label htmlFor="feed-url">订阅地址</label>
                <input
                  id="feed-url"
                  className="grow"
                  type="text"
                  autoFocus
                  placeholder="https://example.com/feed.xml"
                  value={feedUrl}
                  onChange={(e) => setFeedUrl(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") void submitFeed();
                  }}
                />
              </div>
              <p style={{ color: "var(--muted)", fontSize: 12, lineHeight: 1.6 }}>
                本地账户直接抓取 RSS / Atom；FreshRSS 账户会通过 Google Reader API 订阅。
                也可以导入 OPML 批量添加。
              </p>
            </div>
            <div className="modal-foot">
              <button className="tool-btn" onClick={() => setDialog(null)}>取消</button>
              <button className="tool-btn primary" disabled={busy} onClick={() => void submitFeed()}>
                添加
              </button>
            </div>
          </div>
        </div>
      )}

      {toast && (
        <div className={`toast${toast.error ? " error" : ""}`}>{toast.text}</div>
      )}
    </div>
  );
}
