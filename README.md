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
- 安装模式：`%LOCALAPPDATA%\dews\`

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

## 测试

```bash
CARGO_PROFILE_TEST_DEBUG=0 cargo test --lib --manifest-path src-tauri/Cargo.toml
```
