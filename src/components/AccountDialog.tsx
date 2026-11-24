import { useState } from "react";
import { api } from "../api";
import type { Account, AccountKind } from "../types";

interface Props {
  accounts: Account[];
  onAccounts: (accounts: Account[]) => void;
  onClose: () => void;
  notify: (message: string, error?: boolean) => void;
}

const KINDS: { value: AccountKind; label: string }[] = [
  { value: "Local", label: "本地订阅（RSS/Atom）" },
  { value: "FreshRSS", label: "FreshRSS" },
  { value: "GoogleReader", label: "Google Reader 兼容服务" },
];

export default function AccountDialog({ accounts, onAccounts, onClose, notify }: Props) {
  const active = accounts.find((a) => a.is_active) ?? accounts[0];
  const [selectedId, setSelectedId] = useState<number | null>(active?.id ?? null);
  const [kind, setKind] = useState<AccountKind>(active?.kind ?? "Local");
  const [name, setName] = useState(active?.name ?? "本地账户");
  const [endpoint, setEndpoint] = useState(active?.endpoint ?? "");
  const [username, setUsername] = useState(active?.username ?? "");
  const [password, setPassword] = useState("");
  const [isActive, setIsActive] = useState(active?.is_active ?? true);
  const [busy, setBusy] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);

  const select = (account: Account) => {
    setSelectedId(account.id);
    setKind(account.kind);
    setName(account.name);
    setEndpoint(account.endpoint);
    setUsername(account.username);
    setPassword("");
    setIsActive(account.is_active);
    setConfirmDelete(false);
  };

  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    try {
      await action();
    } catch (e) {
      notify(String(e), true);
    } finally {
      setBusy(false);
    }
  };

  const save = () =>
    run(async () => {
      const next = await api.saveAccount({
        id: selectedId ?? 0,
        kind,
        name,
        endpoint: kind === "Local" ? "" : endpoint,
        username: kind === "Local" ? "" : username,
        password: kind === "Local" ? "" : password,
        is_active: isActive,
      });
      onAccounts(next);
      notify("账户已保存");
    });

  const validate = () =>
    run(async () => {
      await api.validateAccount(kind, endpoint, username, password);
      notify("连接验证成功");
    });

  const remove = () =>
    run(async () => {
      if (selectedId === null) return;
      const next = await api.deleteAccount(selectedId);
      onAccounts(next);
      setSelectedId(next[0]?.id ?? null);
      if (next[0]) select(next[0]);
      setConfirmDelete(false);
      notify("账户已删除");
    });

  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <div className="modal" onMouseDown={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <span>账户管理</span>
          <button className="tool-btn icon" onClick={onClose} title="关闭">×</button>
        </div>
        <div className="modal-body">
          <div className="account-list">
            {accounts.map((account) => (
              <div
                key={account.id}
                className={`account-item${selectedId === account.id ? " on" : ""}`}
                onClick={() => select(account)}
              >
                <span className="name">{account.name || account.kind}</span>
                <span className="kind">{account.kind}</span>
                {account.is_active && <span className="kind">当前</span>}
              </div>
            ))}
          </div>
          <div className="form-grid">
            <label>账户类型</label>
            <select
              value={kind}
              onChange={(e) => {
                setKind(e.target.value as AccountKind);
                setPassword("");
              }}
            >
              {KINDS.map((k) => (
                <option key={k.value} value={k.value}>{k.label}</option>
              ))}
            </select>
            <label>名称</label>
            <input className="grow" type="text" value={name} onChange={(e) => setName(e.target.value)} />
            {kind !== "Local" && (
              <>
                <label>API 地址</label>
                <input
                  className="grow"
                  type="text"
                  placeholder="https://rss.example.com/api/greader.php"
                  value={endpoint}
                  onChange={(e) => setEndpoint(e.target.value)}
                />
                <label>用户名</label>
                <input
                  className="grow"
                  type="text"
                  value={username}
                  onChange={(e) => setUsername(e.target.value)}
                />
                <label>密码 / API 密码</label>
                <input
                  className="grow"
                  type="password"
                  placeholder={selectedId ? "留空表示不修改" : ""}
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                />
              </>
            )}
            <label>设为当前账户</label>
            <div className="field-row">
              <input
                type="checkbox"
                checked={isActive}
                onChange={(e) => setIsActive(e.target.checked)}
              />
              <span style={{ color: "var(--muted)", fontSize: 12 }}>
                本地账户读取本机订阅；FreshRSS 通过 Google Reader API 同步
              </span>
            </div>
          </div>
        </div>
        <div className="modal-foot">
          {selectedId !== null && (
            <button
              className="tool-btn"
              disabled={busy}
              onClick={() => {
                if (confirmDelete) void remove();
                else setConfirmDelete(true);
              }}
              style={{ marginRight: "auto", color: "var(--danger)" }}
            >
              {confirmDelete ? "确认删除？" : "删除账户"}
            </button>
          )}
          {kind !== "Local" && (
            <button className="tool-btn" disabled={busy} onClick={() => void validate()}>
              验证连接
            </button>
          )}
          <button className="tool-btn primary" disabled={busy} onClick={() => void save()}>
            保存
          </button>
        </div>
      </div>
    </div>
  );
}
