import { invoke } from "@tauri-apps/api/core";
import type {
  Account,
  AccountInput,
  AccountKind,
  AppData,
  AppSettings,
  SyncReport,
  SyncPlan,
  FeedSyncResult,
} from "./types";

export const api = {
  bootstrap: () => invoke<AppData>("bootstrap"),
  saveSettings: (settings: AppSettings) => invoke<void>("save_settings", { settings }),
  getFonts: () => invoke<string[]>("get_fonts"),
  isPortable: () => invoke<boolean>("is_portable"),

  searchArticles: (query: string) => invoke<string[]>("search_articles", { query }),
  getArticleHtml: (id: string, settings?: AppSettings) =>
    invoke<string>("get_article_html", { id, settingsOverride: settings ?? null }),
  fetchFullText: (id: string, settings?: AppSettings) =>
    invoke<string>("fetch_full_text", { id, settingsOverride: settings ?? null }),
  markArticle: (id: string, read?: boolean, starred?: boolean) =>
    invoke<void>("mark_article", {
      id,
      read: read === undefined ? null : read,
      starred: starred === undefined ? null : starred,
    }),

  syncAll: () => invoke<SyncReport>("sync_all"),
  syncSubscriptions: () => invoke<SyncPlan>("sync_subscriptions"),
  syncFeedArticles: (feedId: string) =>
    invoke<FeedSyncResult>("sync_feed_articles", { feedId }),
  addFeed: (url: string) => invoke<string>("add_feed", { url }),
  removeFeed: (id: string) => invoke<void>("remove_feed", { id }),
  createGroup: (name: string) => invoke<void>("create_group", { name }),
  renameGroup: (id: string, name: string) => invoke<void>("rename_group", { id, name }),
  reorderGroups: (ids: string[]) => invoke<void>("reorder_groups", { ids }),
  deleteGroup: (id: string) => invoke<void>("delete_group", { id }),
  setFeedGroup: (feedId: string, groupId: string | null) =>
    invoke<void>("set_feed_group", { feedId, groupId }),
  importOpml: (xml: string) => invoke<string>("import_opml", { xml }),
  exportOpml: () => invoke<string>("export_opml"),

  validateAccount: (
    kind: AccountKind,
    endpoint: string,
    username: string,
    password: string,
  ) =>
    invoke<void>("validate_account", { kind, endpoint, username, password }),
  saveAccount: (input: AccountInput) => invoke<Account[]>("save_account", { input }),
  deleteAccount: (id: number) => invoke<Account[]>("delete_account", { id }),
  openExternal: (url: string) => invoke<void>("open_external", { url }),
};
