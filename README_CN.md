# Xchat

Xchat `0.1.5` 是一款基于 Tauri 2、React 和 Rust 的局域网聊天客户端。每台设备只需安装并运行客户端；客户端自身负责局域网发现、消息、文件传输和本地 SQLite 存储，不需要单独部署服务端。

## 功能

- 局域网自动发现与手动添加主机
- 单聊、群聊、离线补发、送达与已读状态
- 分块文件传输、取消、重试和文件中心
- 图片粘贴、拖放、输入区预览与消息内联显示
- 桌面端截图编辑（macOS、Windows、Linux）：矩形、椭圆、箭头、画笔、马赛克、文本、回退和钉图
- 中英文、主题、通知、下载目录和网络参数设置
- 可选的 headless Web 运行模式

## 开发

前置要求：Node.js、Rust、Tauri 2 的平台依赖，以及 `cargo-tauri`。

Linux 上截图功能还需要以下系统包（Debian/Ubuntu 包名）：

```bash
sudo apt install pkg-config libclang-dev libxcb1-dev libxrandr-dev \
  libdbus-1-dev libpipewire-0.3-dev libwayland-dev libegl-dev
```

```bash
npm install
cargo tauri dev -- -- --port 18888 --db-path /tmp/xchat-dev
```

若 `1420` 端口已被旧 Vite 进程占用，请先关闭对应开发进程，再重新运行。

只预览 React 界面：

```bash
npm run dev
```

## 构建安装包

当前平台的正式安装包：

```bash
cargo tauri build
```

macOS 产物通常位于：

```text
src-tauri/target/release/bundle/macos/Xchat.app
src-tauri/target/release/bundle/dmg/Xchat_0.1.5_*.dmg
```

指定架构：

```bash
rustup target add x86_64-apple-darwin
cargo tauri build --target x86_64-apple-darwin

rustup target add aarch64-apple-darwin
cargo tauri build --target aarch64-apple-darwin
```

其他 Makefile 目标：

```bash
make help
make deb
make rpm
make apk
make windows-desktop
make web
make web-windows
```

## Web 模式（可选）

普通桌面使用不需要此模式。需要浏览器访问或无界面主机时才启动：

```bash
npm run build
cargo run --manifest-path src-tauri/Cargo.toml \
  --no-default-features --features web --bin lanchat-web \
  -- --port 8888 --db-path /tmp/xchat-web
```

内部 Rust 包和兼容二进制仍使用 `lanchat` / `lanchat-web` 名称；应用界面、安装包、版本和 bundle identifier 分别为 `Xchat`、`0.1.5` 和 `com.xchat.app`。

## 验证

```bash
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml --lib
cargo check --manifest-path src-tauri/Cargo.toml \
  --no-default-features --features desktop --lib
cargo check --manifest-path src-tauri/Cargo.toml \
  --no-default-features --features web --bin lanchat-web
```

## 数据位置

- macOS：`~/Library/Application Support/com.xchat.app/xchat.db`
- Linux：`~/.local/share/com.xchat.app/xchat.db`
- Windows：`%APPDATA%\com.xchat.app\xchat.db`
- 配置目录：`xchat`
- 默认下载目录：`~/Downloads/Xchat`
