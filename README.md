# dews

一个面向 Windows 的桌面 RSS 阅读器，使用 Tauri 2 + React + TypeScript + Rust 构建，通过 Google Reader / FreshRSS 兼容协议同步。

## 功能

- 三栏布局：订阅源 / 文章列表 / 阅读视图，支持拖动分栏
- FreshRSS 账号登录，两阶段同步（先订阅与分类，再逐源同步文章），底部状态栏显示进度
- 已读、星标状态与服务器双向对齐，支持全量同步
- 阅读视图内置「获取全文」：抓取原文正文，并带 `Referer` 下载正文图片后内嵌
- 系统字体枚举、分栏独立字体设置、摘要行数、缩略图尺寸等阅读偏好
- OPML 导入 / 导出，深色主题
- 便携模式：exe 同目录放一个空的 `portable` 文件，数据即存放在 `./data`

## 数据存放

- 便携模式：`./data/data.json`（元数据）与 `./data/content.json`（正文）
- 安装模式（Windows）：`%LOCALAPPDATA%\dews\`
- 安装模式（macOS）：`~/Library/Application Support/dews/`
- 安装模式（Linux）：`$XDG_DATA_HOME/dews/`（未设置时为 `~/.local/share/dews/`）

## 开发

```bash
npm install
npm run tauri dev
```

## 构建

```bash
npm run build
cargo build --release --features custom-protocol --manifest-path src-tauri/Cargo.toml
```

发布版必须启用 `custom-protocol`，否则程序会去加载开发服务器地址。

macOS 本地一键构建（`.app` + `.dmg`）：

```bash
./scripts/build_mac.sh
```

### GitHub Actions 自动构建

`.github/workflows/build.yml` 在推送 `main`、推送 `v*` 标签、PR 或手动触发时构建三类产物，并在 Actions 页面以 artifact 形式提供下载：

- Windows 安装版：NSIS（`*-setup.exe`）与 MSI
- Windows 便携版：`dews_*_portable.zip`（含 `dews.exe` 与空的 `portable` 标记文件）
- macOS：`dews_0.1.0_aarch64.dmg`（arm64）

macOS 的 dmg 由 `hdiutil` 生成，避免 Tauri 自带 dmg 脚本在部分环境下的 `grep` 兼容问题。当前构建为 ad-hoc 签名，对外分发仍需 Developer ID 签名与公证。

## 测试

```bash
CARGO_PROFILE_TEST_DEBUG=0 cargo test --lib --manifest-path src-tauri/Cargo.toml
```
