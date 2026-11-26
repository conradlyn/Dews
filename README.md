# dews

一个跨平台的桌面 RSS 阅读器，使用 Tauri 2 + React + TypeScript + Rust 构建，通过 Google Reader / FreshRSS 兼容协议同步。

## 功能

- 三栏布局：订阅源 / 文章列表 / 阅读视图，支持拖动分栏
- FreshRSS 账号登录，两阶段同步（先订阅与分类，再以 6 个并发任务拉取文章），底部状态栏显示进度
- 文章增量同步：远端按上次同步时间提前结束分页，本地 RSS 用 ETag / Last-Modified 条件请求；每天做一次完整校准以收敛旧文章的服务器已读状态
- 已读、星标状态与服务器双向对齐，支持全量同步；远程不可用时本地操作先入队，待下次同步重试
- 当前列表一键批量标为已读，远程批量推送失败同样进入待同步队列
- 添加订阅时先验证订阅地址与账户连接，可选择分类；FreshRSS / GReader 账户下新增的订阅会在下次同步时写入远端服务器（含分类）
- 阅读视图内置「获取全文」：抓取原文正文，并带 `Referer` 下载正文图片后内嵌
- 系统字体枚举、分栏独立字体设置、摘要行数、缩略图尺寸、正文边距/行距/段首缩进等阅读偏好
- 工具栏与筛选按钮支持「图标 + 文字」或「仅图标」两种风格
- 本地账户订阅源分组：新建、重命名、排序、删除与归属调整
- OPML 导入 / 导出，深色主题
- 同步过程中按订阅源即时刷新未读计数：每个源拉取完成就把新文章并入列表，不必等全部源结束
- 未读总数显示在应用图标上：macOS 为 Dock 数字徽标，Windows 为任务栏图标上的星形角标
- 关闭窗口只是隐藏而不退出：macOS 点击 Dock 图标恢复、Cmd+Q 或 Dock 右键退出；Windows 常驻系统托盘，右键菜单含显示窗口 / 最小化到托盘 / 最大化还原 / 设置 / 退出，双击托盘图标恢复
- 便携模式：exe 所在目录可写（或已有 `./data`）时自动启用，无需 `portable` 空文件；也可用 `DEWS_DATA_DIR` 显式指定数据目录

## 数据存放

- 自动便携模式：`./data/data.json`（元数据）与 `./data/content.json`（正文）；旧的 `portable` 空文件仍兼容，但不再需要
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

`.github/workflows/build.yml` 在推送 `main`、`local/**` 分支、推送 `v*` 标签、PR 或手动触发时构建三类产物，并在 Actions 页面以 artifact 形式提供下载。推送 `v*` 标签时，还会自动创建 GitHub Release 并附上全部产物：

- Windows 安装版：NSIS（`*-setup.exe`）与 MSI
- Windows 便携版：`dews_*_portable.zip`（含 `dews.exe`，首次运行自动生成 `./data`）
- macOS：`dews_1.1.0_aarch64.dmg`（arm64）

macOS 的 dmg 由 `hdiutil` 生成，避免 Tauri 自带 dmg 脚本在部分环境下的 `grep` 兼容问题。当前构建为 ad-hoc 签名，对外分发仍需 Developer ID 签名与公证。

## 测试

```bash
CARGO_PROFILE_TEST_DEBUG=0 cargo test --lib --manifest-path src-tauri/Cargo.toml
```
