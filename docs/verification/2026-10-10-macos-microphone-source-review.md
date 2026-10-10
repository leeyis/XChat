# macOS 麦克风修复源码复核

日期：2026-10-10。只读复核源码及官方资料，未运行 macOS 构建或实机通话。

结论：这次修改补齐了 WKWebView 暴露麦克风 API 所需的应用配置，不只是替换 `undefined` 报错文案；但当前 Windows 环境不能证明 macOS 已成功采集或完成双向通话。

## 实际配置链

- [Info.plist](../../src-tauri/Info.plist) 包含 `NSMicrophoneUsageDescription`；[Entitlements.plist](../../src-tauri/Entitlements.plist) 包含 `com.apple.security.device.audio-input`，并被 [tauri.conf.json](../../src-tauri/tauri.conf.json) 的 `bundle.macOS.entitlements` 引用。请求为 `audio: {...}, video: false`，无需为本次语音额外申请摄像头权限。
- Tauri 2.10.2 + tauri-codegen 2.5.4 会读取配置目录中的 `Info.plist`：macOS 开发模式由 `generate_context!` 嵌入可执行文件，正式 `.app` 打包则合并到应用 Info.plist。项目 [build.rs](../../src-tauri/build.rs) 对两个 plist 增加重建跟踪，防止权限修改后继续使用旧开发二进制。版本源码：[开发模式嵌入](https://github.com/tauri-apps/tauri/blob/tauri-codegen-v2.5.4/crates/tauri-codegen/src/context.rs#L303-L350)、[macOS 打包配置说明](https://v2.tauri.app/reference/config/#macconfig)。
- 当前主窗口和独立 viewer 均加载应用页面；开发页面来自 `http://localhost:1420`，正式包使用 `tauri://localhost`。Tauri 已通过 localhost 主机名处理 macOS 自定义协议的 secure-context 问题；没有证据要求本项目把正式页换成 HTTPS。`useHttpsScheme` 只影响 Windows/Android。[Tauri URL 选择源码](https://github.com/tauri-apps/tauri/blob/tauri-v2.10.2/crates/tauri/src/manager/mod.rs#L345-L358)、[上游修复 #1550/#1551](https://github.com/tauri-apps/tauri/issues/1550)、[WebKit 自定义协议修复](https://bugs.webkit.org/show_bug.cgi?id=220184)。
- 本地 Wry 0.54.2 已实现 `requestMediaCapturePermissionForOrigin` delegate，并给出 WebView 层的 Grant；它不能替代 macOS 系统麦克风授权。[Wry 对应版本源码](https://github.com/tauri-apps/wry/blob/wry-v0.54.2/src/wkwebview/class/wry_web_view_ui_delegate.rs#L129-L140)

WebKit 官方明确：嵌入应用能够原生采集音频或视频时，WKWebView 才自动暴露 `navigator.mediaDevices.getUserMedia`；随后仍需经过授权。因此缺少 usage description 确实可能表现为 API 本身缺失，补齐 plist 是根因层面的修复条件。[WebKit 说明](https://webkit.org/blog/11353/mediarecorder-api/#getusermedia-in-wkwebview)

## 尚未验证的具体边界

1. 必须在新构建的 macOS `.app` 或 Tauri 开发程序中验证。`tauri-codegen` 的 plist 嵌入分支限定 `dev && !running_tests`；直接运行脱离 `.app` 的 release 可执行文件不等于运行带有 bundle Info.plist 的应用，不能拿它验证正式包权限。
2. 旧安装包、已有系统拒绝记录以及实际签名 entitlements 仍影响采集结果。源码配置正确不能替代检查最终 `.app` 的 Info.plist、签名权限及系统授权后，实测 `isSecureContext`、`typeof navigator.mediaDevices?.getUserMedia`、音轨 `readyState` 和双向声音。
3. [remote-voice.js](../../frontend/src/remote-voice.js) 的 capability guard 本身仅提供可理解的错误并避免直接读取 undefined；它不会创建缺失的 API。通话时序修复负责去重采集、取消旧请求及串行替换音轨；不能把这些单元验证表述为 macOS 真实麦克风验收通过。

本次未发现需要额外开启不安全协议或 WebKit 私有媒体开关的源码依据。
