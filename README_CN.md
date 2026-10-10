# Xchat

Xchat `0.1.14` 是一款基于 Tauri 2、React 和 Rust 的局域网聊天客户端。每台设备只需安装并运行客户端；客户端自身负责局域网发现、消息、文件传输和本地 SQLite 存储，不需要单独部署服务端。

## 功能

- 局域网自动发现与手动添加主机
- 单聊、群聊、离线补发、送达与已读状态
- 分块文件传输、取消、重试和文件中心
- 图片粘贴、拖放、输入区预览与消息内联显示
- 对话内音视频播放与 GIF、动态 WebP、APNG 展示，支持暂停和原文件操作
- Web/桌面共用截图编辑器：选区调整、多行文字编辑与边框拖动、形状与画笔、马赛克、模糊、橡皮、撤销重做，以及鼠标滚轮调节参数
- 直接框选、原位贴图与继续标注：支持缩放、旋转、透明度、窗口阴影和隐藏恢复；Web贴图保留在当前页面内
- 中英文、主题、通知、下载目录和网络参数设置
- 可选的 headless Web 运行模式
- 独立远程桌面窗口与全屏、紧凑共享工具条、会话内授权控制，以及和聊天布局分离的语音通话
- Windows DXGI 采集与硬件 HEVC 传输，能力不足时回退兼容 RTP 链路
- 桌面开机自启动设置，以及建群时的在线状态与在线优先排序

截图按钮和已配置的快捷键直接进入选区。截图文字编辑时，Enter直接换行，点击编辑框外部完成编辑；拖动边框可移动文字，Delete删除当前激活的标注。鼠标滚轮调整当前工具的大小。贴图右键打开操作菜单，Space进入标注并保持贴图位置与缩放；Ctrl+Shift+P开启鼠标穿透，F3恢复隐藏或穿透贴图，Esc隐藏，Shift+Esc销毁。关闭窗口阴影时也会移除贴图边框。

## 开发

远控性能仍在验证：本机 Windows RDP 测试中，1080p 前台与最小化均约 31.4 FPS，源图像更新时间平均 164.68 ms。这不能代表双机局域网性能；macOS 麦克风与硬件解码也待实机确认，详见[验证记录](docs/verification/2026-10-10-remote-followup.md)。

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

Windows 使用 `tauri.windows.conf.json` 将前端构建到 `dist/frontend`，并通过相对路径嵌入程序。`frontendDist` 请保持相对路径：Windows 盘符路径可能被解析成 URL，导致安装包缺少前端资源。

打包前检查首页及其 JavaScript、CSS 和图标是否已嵌入：

```bash
npm run build -- --outDir ../dist/frontend --emptyOutDir
cargo test --manifest-path src-tauri/Cargo.toml \
  --no-default-features --features desktop,custom-protocol --test bundled_frontend
```

macOS 产物通常位于：

```text
src-tauri/target/release/bundle/macos/Xchat.app
src-tauri/target/release/bundle/dmg/Xchat_0.1.14_*.dmg
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

内部 Rust 包和兼容二进制仍使用 `lanchat` / `lanchat-web` 名称；应用界面、安装包、版本和 bundle identifier 分别为 `Xchat`、`0.1.14` 和 `com.xchat.app`。

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
