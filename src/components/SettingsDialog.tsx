import { useRef, useState } from "react";
import { api } from "../api";
import type { Account, AccountKind, AppSettings, Feed, FeedGroup } from "../types";

type Category = "appearance" | "sync" | "data";

const CATEGORIES: { id: Category; label: string }[] = [
  { id: "appearance", label: "外观与字体" },
  { id: "sync", label: "账户与同步" },
  { id: "data", label: "导入与导出" },
];

const KINDS: { value: AccountKind; label: string }[] = [
  { value: "Local", label: "本地订阅（RSS/Atom）" },
  { value: "FreshRSS", label: "FreshRSS" },
  { value: "GoogleReader", label: "Google Reader 兼容服务" },
];

interface Props {
  settings: AppSettings;
  fonts: string[];
  accounts: Account[];
  activeAccount: Account | null;
  onAccounts: (accounts: Account[]) => void;
  onAccountSaved: (accounts: Account[], savedId: number) => void;
  groups: FeedGroup[];
  feeds: Feed[];
  onChange: (patch: Partial<AppSettings>) => void;
  onCreateGroup: (name: string) => Promise<void>;
  onRenameGroup: (id: string, name: string) => Promise<void>;
  onReorderGroups: (ids: string[]) => Promise<void>;
  onDeleteGroup: (id: string) => Promise<void>;
  onSetFeedGroup: (feedId: string, groupId: string | null) => Promise<void>;
  onImportOpml: (file: File) => void;
  onExportOpml: () => void;
  notify: (message: string, error?: boolean) => void;
  onClose: () => void;
}

function FontSelect({
  value,
  fonts,
  onSelect,
}: {
  value: string;
  fonts: string[];
  onSelect: (value: string) => void;
}) {
  const options = fonts.includes(value) ? fonts : [value, ...fonts];
  return (
    <select value={value} onChange={(e) => onSelect(e.target.value)}>
      {options.map((font) => (
        <option key={font} value={font}>
          {font}
        </option>
      ))}
    </select>
  );
}

export default function SettingsDialog({
  settings,
  fonts,
  accounts,
  activeAccount,
  onAccounts,
  onAccountSaved,
  groups,
  feeds,
  onChange,
  onCreateGroup,
  onRenameGroup,
  onReorderGroups,
  onDeleteGroup,
  onSetFeedGroup,
  onImportOpml,
  onExportOpml,
  notify,
  onClose,
}: Props) {
  const [category, setCategory] = useState<Category>("appearance");
  const fileInput = useRef<HTMLInputElement | null>(null);
  const [newGroupName, setNewGroupName] = useState("");
  const [draftNames, setDraftNames] = useState<Record<string, string>>({});

  const initialAccount = activeAccount ?? accounts[0] ?? null;
  const [selectedAccountId, setSelectedAccountId] = useState<number | null>(
    initialAccount?.id ?? null,
  );
  const [accountKind, setAccountKind] = useState<AccountKind>(
    initialAccount?.kind ?? "Local",
  );
  const [accountName, setAccountName] = useState(initialAccount?.name ?? "本地账户");
  const [accountEndpoint, setAccountEndpoint] = useState(initialAccount?.endpoint ?? "");
  const [accountUsername, setAccountUsername] = useState(initialAccount?.username ?? "");
  const [accountPassword, setAccountPassword] = useState("");
  const [accountActive, setAccountActive] = useState(initialAccount?.is_active ?? true);
  const [accountBusy, setAccountBusy] = useState(false);
  const [confirmAccountDelete, setConfirmAccountDelete] = useState(false);

  // Remote groups are server-owned and rebuilt on every sync, so group
  // management is exposed only while the local account is active.
  const isLocal = activeAccount?.kind === "Local";
  const localAccountId = isLocal && activeAccount ? activeAccount.id : null;
  const localGroups =
    localAccountId === null
      ? []
      : groups.filter((g) => g.account_id === localAccountId);
  const localFeeds =
    localAccountId === null
      ? []
      : feeds.filter((f) => f.account_id === localAccountId);

  const runAction = async (action: () => Promise<void>, success?: string) => {
    try {
      await action();
      if (success) notify(success);
    } catch (e) {
      notify(String(e), true);
    }
  };

  const selectAccount = (account: Account) => {
    setSelectedAccountId(account.id);
    setAccountKind(account.kind);
    setAccountName(account.name);
    setAccountEndpoint(account.endpoint);
    setAccountUsername(account.username);
    setAccountPassword("");
    setAccountActive(account.is_active);
    setConfirmAccountDelete(false);
  };

  const addNewAccount = () => {
    setSelectedAccountId(null);
    setAccountKind("FreshRSS");
    setAccountName("");
    setAccountEndpoint("");
    setAccountUsername("");
    setAccountPassword("");
    setAccountActive(true);
    setConfirmAccountDelete(false);
  };

  const runAccountAction = async (action: () => Promise<void>) => {
    setAccountBusy(true);
    try {
      await action();
    } catch (e) {
      notify(String(e), true);
    } finally {
      setAccountBusy(false);
    }
  };

  const saveAccount = () =>
    void runAccountAction(async () => {
      const beforeIds = new Set(accounts.map((a) => a.id));
      const next = await api.saveAccount({
        id: selectedAccountId ?? 0,
        kind: accountKind,
        name: accountName,
        endpoint: accountKind === "Local" ? "" : accountEndpoint,
        username: accountKind === "Local" ? "" : accountUsername,
        password: accountKind === "Local" ? "" : accountPassword,
        is_active: accountActive,
      });
      const saved =
        selectedAccountId === null
          ? next.find((a) => !beforeIds.has(a.id)) ?? next[next.length - 1]
          : next.find((a) => a.id === selectedAccountId) ?? next[0];
      onAccounts(next);
      notify("账户已保存");
      if (saved) {
        selectAccount(next.find((a) => a.id === saved.id) ?? saved);
        onAccountSaved(next, saved.id);
      }
    });

  const validateAccount = () =>
    void runAccountAction(async () => {
      await api.validateAccount(accountKind, accountEndpoint, accountUsername, accountPassword);
      notify("连接验证成功");
    });

  const removeAccount = () =>
    void runAccountAction(async () => {
      if (selectedAccountId === null) return;
      const next = await api.deleteAccount(selectedAccountId);
      onAccounts(next);
      if (next[0]) selectAccount(next[0]);
      setConfirmAccountDelete(false);
      notify("账户已删除");
    });

  const submitCreate = () => {
    const name = newGroupName.trim();
    if (!name) return;
    void runAction(async () => {
      await onCreateGroup(name);
      setNewGroupName("");
      notify("分组已创建");
    });
  };

  const commitRename = (group: FeedGroup) => {
    const draft = draftNames[group.id];
    const clearDraft = () =>
      setDraftNames((prev) => {
        const next = { ...prev };
        delete next[group.id];
        return next;
      });
    if (draft === undefined || !draft.trim() || draft.trim() === group.name) {
      clearDraft();
      return;
    }
    const name = draft.trim();
    void runAction(async () => {
      await onRenameGroup(group.id, name);
      clearDraft();
      notify("分组已重命名");
    });
  };

  const moveGroup = (index: number, delta: number) => {
    const ids = localGroups.map((g) => g.id);
    const target = index + delta;
    if (target < 0 || target >= ids.length) return;
    [ids[index], ids[target]] = [ids[target], ids[index]];
    void runAction(() => onReorderGroups(ids));
  };

  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <div className="modal" onMouseDown={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <span>设置</span>
          <button className="modal-close" onClick={onClose} title="关闭" aria-label="关闭设置">×</button>
        </div>
        <div className="modal-body settings-body">
          <nav className="settings-nav" role="tablist" aria-label="设置分类">
            {CATEGORIES.map((c) => (
              <button
                key={c.id}
                type="button"
                role="tab"
                aria-selected={category === c.id}
                className={category === c.id ? "on" : ""}
                onClick={() => setCategory(c.id)}
              >
                {c.label}
              </button>
            ))}
          </nav>
          <div className="settings-content" role="tabpanel">
            <div className="form-grid">
              {category === "appearance" && (
                <>
                  <div className="group-title">主题</div>
                  <label htmlFor="dark">深色主题</label>
                  <div className="field-row">
                    <input
                      id="dark"
                      type="checkbox"
                      checked={settings.dark_theme}
                      onChange={(e) => onChange({ dark_theme: e.target.checked })}
                    />
                    <span style={{ color: "var(--muted)", fontSize: 12 }}>正文与界面同步切换</span>
                  </div>

                  <label>顶部按钮风格</label>
                  <select
                    value={settings.button_style}
                    onChange={(e) =>
                      onChange({
                        button_style: e.target.value as AppSettings["button_style"],
                      })
                    }
                  >
                    <option value="icon-text">图标与文字</option>
                    <option value="icon">仅图标</option>
                  </select>

                  <div className="group-title">订阅栏</div>
                  <label>字体</label>
                  <FontSelect
                    value={settings.feed_font_family}
                    fonts={fonts}
                    onSelect={(v) => onChange({ feed_font_family: v })}
                  />
                  <label>字号（px）</label>
                  <input
                    type="number"
                    min={10}
                    max={22}
                    value={settings.feed_font_size}
                    onChange={(e) => onChange({ feed_font_size: Number(e.target.value) })}
                  />

                  <div className="group-title">文章列表</div>
                  <label>字体</label>
                  <FontSelect
                    value={settings.article_font_family}
                    fonts={fonts}
                    onSelect={(v) => onChange({ article_font_family: v })}
                  />
                  <label>字号（px）</label>
                  <input
                    type="number"
                    min={10}
                    max={22}
                    value={settings.article_font_size}
                    onChange={(e) => onChange({ article_font_size: Number(e.target.value) })}
                  />
                  <label>摘要行数</label>
                  <select
                    value={settings.summary_lines}
                    onChange={(e) => onChange({ summary_lines: Number(e.target.value) })}
                  >
                    {[0, 1, 2, 3, 4, 5].map((n) => (
                      <option key={n} value={n}>{n === 0 ? "不显示" : `${n} 行`}</option>
                    ))}
                  </select>
                  <label>缩略图</label>
                  <select
                    value={settings.thumbnail_size}
                    onChange={(e) => onChange({ thumbnail_size: Number(e.target.value) })}
                  >
                    <option value={0}>关闭</option>
                    <option value={1}>小（48px）</option>
                    <option value={2}>中（72px）</option>
                    <option value={3}>大（整行，文字在下）</option>
                  </select>

                  <div className="group-title">阅读视图</div>
                  <label>正文字体</label>
                  <FontSelect
                    value={settings.reading_font_family}
                    fonts={fonts}
                    onSelect={(v) => onChange({ reading_font_family: v })}
                  />
                  <label>正文字号（px）</label>
                  <input
                    type="number"
                    min={12}
                    max={26}
                    value={settings.font_size}
                    onChange={(e) => onChange({ font_size: Number(e.target.value) })}
                  />
                  <label>两边空白（px）</label>
                  <input
                    type="number"
                    min={0}
                    max={200}
                    value={settings.reading_margin}
                    onChange={(e) => onChange({ reading_margin: Number(e.target.value) })}
                  />
                  <label>行距（倍）</label>
                  <input
                    type="number"
                    min={1}
                    max={3}
                    step={0.05}
                    value={settings.reading_line_height}
                    onChange={(e) => onChange({ reading_line_height: Number(e.target.value) })}
                  />
                  <label>段首缩进（字符）</label>
                  <input
                    type="number"
                    min={0}
                    max={8}
                    value={settings.reading_indent}
                    onChange={(e) => onChange({ reading_indent: Number(e.target.value) })}
                  />
                  <label>打开文章即已读</label>
                  <div className="field-row">
                    <input
                      type="checkbox"
                      checked={settings.mark_as_read_on_open}
                      onChange={(e) => onChange({ mark_as_read_on_open: e.target.checked })}
                    />
                  </div>
                  <label>切换下一篇时标记已读</label>
                  <div className="field-row">
                    <input
                      type="checkbox"
                      checked={settings.mark_as_read_when_next}
                      onChange={(e) => onChange({ mark_as_read_when_next: e.target.checked })}
                    />
                  </div>
                </>
              )}

              {category === "sync" && (
                <>
                  <div className="group-title">账户</div>
                  <div className="account-section account-list">
                    {accounts.map((account) => (
                      <div
                        key={account.id}
                        className={`account-item${selectedAccountId === account.id ? " on" : ""}`}
                        onClick={() => selectAccount(account)}
                      >
                        <span className="name">{account.name || account.kind}</span>
                        <span className="kind">{account.kind}</span>
                        {account.is_active && <span className="kind">当前</span>}
                      </div>
                    ))}
                    <button
                      className="tool-btn"
                      disabled={accountBusy}
                      onClick={addNewAccount}
                      style={{ alignSelf: "flex-start" }}
                    >
                      + 新增账户
                    </button>
                  </div>

                  <label>账户类型</label>
                  <select
                    value={accountKind}
                    onChange={(e) => {
                      setAccountKind(e.target.value as AccountKind);
                      setAccountPassword("");
                    }}
                  >
                    {KINDS.map((k) => (
                      <option key={k.value} value={k.value}>{k.label}</option>
                    ))}
                  </select>
                  <label>名称</label>
                  <input
                    className="grow"
                    type="text"
                    value={accountName}
                    onChange={(e) => setAccountName(e.target.value)}
                  />
                  {accountKind !== "Local" && (
                    <>
                      <label>API 地址</label>
                      <input
                        className="grow"
                        type="text"
                        placeholder="https://rss.example.com/api/greader.php"
                        value={accountEndpoint}
                        onChange={(e) => setAccountEndpoint(e.target.value)}
                      />
                      <label>用户名</label>
                      <input
                        className="grow"
                        type="text"
                        value={accountUsername}
                        onChange={(e) => setAccountUsername(e.target.value)}
                      />
                      <label>密码 / API 密码</label>
                      <input
                        className="grow"
                        type="password"
                        placeholder={selectedAccountId !== null ? "留空表示不修改" : ""}
                        value={accountPassword}
                        onChange={(e) => setAccountPassword(e.target.value)}
                      />
                    </>
                  )}
                  <label>设为当前账户</label>
                  <div className="field-row">
                    <input
                      type="checkbox"
                      checked={accountActive}
                      onChange={(e) => setAccountActive(e.target.checked)}
                    />
                    <span style={{ color: "var(--muted)", fontSize: 12 }}>
                      本地账户读取本机订阅；远程账户通过 Google Reader API 同步
                    </span>
                  </div>
                  <div className="field-row account-actions">
                    {selectedAccountId !== null && (
                      <button
                        className="tool-btn"
                        disabled={accountBusy}
                        onClick={() => {
                          if (confirmAccountDelete) void removeAccount();
                          else setConfirmAccountDelete(true);
                        }}
                        style={{ marginRight: "auto", color: "var(--danger)" }}
                      >
                        {confirmAccountDelete ? "确认删除？" : "删除账户"}
                      </button>
                    )}
                    {accountKind !== "Local" && (
                      <button
                        className="tool-btn"
                        disabled={accountBusy}
                        onClick={() => void validateAccount()}
                      >
                        验证连接
                      </button>
                    )}
                    <button
                      className="tool-btn primary"
                      disabled={accountBusy}
                      onClick={saveAccount}
                    >
                      保存账户
                    </button>
                  </div>

                  <label>自动同步间隔（分钟）</label>
                  <input
                    type="number"
                    min={0}
                    max={1440}
                    value={settings.sync_interval_minutes}
                    onChange={(e) => onChange({ sync_interval_minutes: Number(e.target.value) })}
                  />
                  <label>默认筛选</label>
                  <select
                    value={settings.filter}
                    onChange={(e) => onChange({ filter: e.target.value })}
                  >
                    <option value="all">全部</option>
                    <option value="unread">仅未读</option>
                    <option value="read">仅已读</option>
                  </select>

                  {isLocal && (
                    <>
                      <div className="group-title">订阅源分组</div>
                      <label>新建分组</label>
                      <div className="field-row">
                        <input
                          type="text"
                          className="grow"
                          placeholder="输入分组名称"
                          value={newGroupName}
                          onChange={(e) => setNewGroupName(e.target.value)}
                          onKeyDown={(e) => {
                            if (e.key === "Enter") submitCreate();
                          }}
                        />
                        <button className="tool-btn" onClick={submitCreate}>新建</button>
                      </div>

                      <label>分组顺序 / 重命名</label>
                      <div className="field-row manage-list">
                        {localGroups.map((group, i) => (
                          <div className="manage-row" key={group.id}>
                            <input
                              type="text"
                              className="grow"
                              value={draftNames[group.id] ?? group.name}
                              onChange={(e) =>
                                setDraftNames((prev) => ({ ...prev, [group.id]: e.target.value }))
                              }
                              onBlur={() => commitRename(group)}
                              onKeyDown={(e) => {
                                if (e.key === "Enter") e.currentTarget.blur();
                              }}
                            />
                            <button
                              className="tool-btn icon"
                              disabled={i === 0}
                              title="上移"
                              onClick={() => moveGroup(i, -1)}
                            >
                              ↑
                            </button>
                            <button
                              className="tool-btn icon"
                              disabled={i === localGroups.length - 1}
                              title="下移"
                              onClick={() => moveGroup(i, 1)}
                            >
                              ↓
                            </button>
                            <button
                              className="tool-btn icon"
                              title="删除分组"
                              onClick={() =>
                                void runAction(() => onDeleteGroup(group.id), "分组已删除")
                              }
                            >
                              ×
                            </button>
                          </div>
                        ))}
                        {localGroups.length === 0 && (
                          <span style={{ color: "var(--muted)", fontSize: 12 }}>暂无分组</span>
                        )}
                      </div>

                      <label>订阅源归属</label>
                      <div className="field-row manage-list">
                        {localFeeds.map((feed) => (
                          <div className="manage-row" key={feed.id}>
                            <span className="grow manage-feed-title" title={feed.url}>
                              {feed.title || feed.url}
                            </span>
                            <select
                              value={feed.group_id ?? ""}
                              onChange={(e) =>
                                void runAction(() => onSetFeedGroup(feed.id, e.target.value || null))
                              }
                            >
                              <option value="">未分组</option>
                              {localGroups.map((group) => (
                                <option key={group.id} value={group.id}>{group.name}</option>
                              ))}
                            </select>
                          </div>
                        ))}
                        {localFeeds.length === 0 && (
                          <span style={{ color: "var(--muted)", fontSize: 12 }}>暂无本地订阅源</span>
                        )}
                      </div>
                    </>
                  )}
                </>
              )}

              {category === "data" && (
                <>
                  <label>OPML 订阅</label>
                  <div className="field-row">
                    <button className="tool-btn" onClick={() => fileInput.current?.click()}>
                      导入 OPML
                    </button>
                    <button className="tool-btn" onClick={onExportOpml}>
                      导出 OPML
                    </button>
                    <input
                      ref={fileInput}
                      type="file"
                      accept=".opml,.xml,text/xml,application/xml"
                      style={{ display: "none" }}
                      onChange={(e) => {
                        const file = e.target.files?.[0];
                        if (file) onImportOpml(file);
                        e.target.value = "";
                      }}
                    />
                  </div>
                </>
              )}
            </div>
          </div>
        </div>
        <div className="modal-foot">
          <button className="tool-btn primary" onClick={onClose}>完成</button>
        </div>
      </div>
    </div>
  );
}
