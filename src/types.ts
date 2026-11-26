// Mirrors src-tauri/src/data/model.rs + commands.rs (serde camelCase kept as-is).

export type AccountKind = "Local" | "GoogleReader" | "FreshRSS";

export interface Account {
  id: number;
  kind: AccountKind;
  name: string;
  endpoint: string;
  username: string;
  password_protected: string;
  is_active: boolean;
}

export interface AccountInput {
  id: number;
  kind: AccountKind;
  name: string;
  endpoint: string;
  username: string;
  password: string;
  is_active: boolean;
}

export interface FeedGroup {
  id: string;
  account_id: number;
  name: string;
}

export interface Feed {
  id: string;
  account_id: number;
  remote_id: string;
  title: string;
  url: string;
  website: string | null;
  group_id: string | null;
  last_synced: string | null;
  failure_count: number;
}

export interface Article {
  id: string;
  account_id: number;
  feed_id: string;
  remote_id: string;
  title: string;
  author: string | null;
  link: string | null;
  published: number;
  is_read: boolean;
  is_starred: boolean;
  is_archived: boolean;
  snippet: string | null;
  thumbnail_url: string | null;
}

export interface AppSettings {
  dark_theme: boolean;
  font_size: number;
  feed_font_family: string;
  feed_font_size: number;
  article_font_family: string;
  article_font_size: number;
  article_title_font_family: string;
  article_title_font_size: number;
  article_title_bold: boolean;
  reading_font_family: string;
  reading_title_font_family: string;
  reading_title_font_size: number;
  reading_title_bold: boolean;
  summary_lines: number;
  thumbnail_size: number;
  mark_as_read_on_open: boolean;
  mark_as_read_when_next: boolean;
  sync_interval_minutes: number;
  filter: string;
  /** "icon-text" shows icon + label, "icon" hides the label. */
  button_style: "icon-text" | "icon";
  feed_column_width: number;
  article_column_width: number;
  reading_margin: number;
  reading_line_height: number;
  reading_indent: number;
}

export interface AppData {
  accounts: Account[];
  groups: FeedGroup[];
  feeds: Feed[];
  articles: Article[];
  settings: AppSettings;
}

export interface SyncReport {
  feeds: number;
  articles: number;
  errors: string[];
  message: string;
}

export interface SyncFeedInfo {
  id: string;
  title: string;
  account_name: string;
}

export interface SyncPlan {
  feeds: SyncFeedInfo[];
  errors: string[];
  message: string;
}

export interface FeedSyncResult {
  feed_id: string;
  title: string;
  articles: number;
  total: number;
  error: string | null;
  /** Rows inserted or whose read/starred flags moved during this pass. */
  changed: Article[];
  /** Article ids the server no longer has. */
  removed: string[];
}

export interface MarkReadReport {
  marked: number;
  pending: number;
}

export interface FeedCheck {
  title: string;
  website: string | null;
  remote: boolean;
  account_name: string;
  message: string;
}

export type Scope =
  | { type: "all" }
  | { type: "unread" }
  | { type: "starred" }
  | { type: "feed"; id: string }
  | { type: "group"; id: string };

/** Lifecycle of the in-app update check, driven by the Tauri updater plugin. */
export type UpdateStatus =
  | "idle"
  | "checking"
  | "latest"
  | "available"
  | "downloading"
  | "installing"
  | "error";

export interface UpdateState {
  status: UpdateStatus;
  /** Version announced by the release manifest, when one is available. */
  version?: string;
  notes?: string;
  date?: string;
  /** 0..1 while downloading; undefined when the server sends no length. */
  progress?: number;
  message?: string;
}
