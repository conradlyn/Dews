import type { AppSettings } from "../types";

interface Props {
  settings: AppSettings;
  fonts: string[];
  onChange: (patch: Partial<AppSettings>) => void;
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

export default function SettingsDialog({ settings, fonts, onChange, onClose }: Props) {
  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <div className="modal" onMouseDown={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <span>设置</span>
          <button className="tool-btn icon" onClick={onClose} title="关闭">×</button>
        </div>
        <div className="modal-body">
          <div className="form-grid">
            <div className="group-title">外观</div>
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

            <div className="group-title">同步与筛选</div>
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
          </div>
        </div>
        <div className="modal-foot">
          <button className="tool-btn primary" onClick={onClose}>完成</button>
        </div>
      </div>
    </div>
  );
}
