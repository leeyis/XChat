# 远程协助语音修复与验证 · 2026-10-10

## 结论与边界

已修正 macOS 客户端缺少麦克风权限声明，以及前端直接访问不存在的 `navigator.mediaDevices` 的路径。补充了异步授权、挂断重拨、麦克风切换和会话切换的资源清理。Windows 上的真实 Chromium WebRTC 回环通过，使用浏览器合成麦克风，两端 Opus 音频 RTP 均持续增长。

本次环境是 Windows，**没有 macOS / WKWebView 运行结果，也没有两台实体设备的双向通话证据**。本报告不能用作 macOS 实机验收或真实声音可听性验收。界面原型审批不属于此修复的非可视运行时范围。

## 原因与实现

- 仓库原先没有 `src-tauri/Info.plist`。Apple 要求使用麦克风的应用声明 `NSMicrophoneUsageDescription`；WebKit 说明，嵌入应用具备原生采集能力后才暴露 `getUserMedia`。这与反馈中 `navigator.mediaDevices` 为 `undefined` 一致，是依据源码与官方文档作出的诊断，尚非 macOS 运行追踪。[Apple 麦克风声明](https://developer.apple.com/documentation/bundleresources/information-property-list/nsmicrophoneusagedescription)，[WebKit WKWebView 媒体支持](https://webkit.org/blog/11353/mediarecorder-api/)
- 新增 `Info.plist` 麦克风用途说明和 `Entitlements.plist` 的 `com.apple.security.device.audio-input`，通过 `bundle.macOS.entitlements` 参与签名。没有添加相机权限，也没有开启 App Sandbox。Tauri 会合并自定义 `Info.plist`；已按当前锁定的 `tauri-codegen 2.5.4` 复核 `context.rs:303–346`，macOS 开发态的非测试构建会自动嵌入该文件，因此不需要私有 WKWebView API 或新依赖。`build.rs` 仅增加 macOS 权限文件变更的重编译触发。尚未生成并检查真实 macOS `.app` 的签名和合并结果。[Tauri macOS Bundle 文档](https://v2.tauri.app/distribute/macos-application-bundle/)，[锁定版本的开发态源码](https://github.com/tauri-apps/tauri/blob/tauri-codegen-v2.5.4/crates/tauri-codegen/src/context.rs)，[Apple 音频输入 entitlement](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.security.device.audio-input)
- Wry 0.54.2 使用 `WKURLSchemeHandler` 注册 custom protocol；WebKit 将 localhost 或 scheme handler 处理的协议视为 potentially trustworthy。由此推断默认 `tauri://localhost` 不会仅因协议名而失去安全上下文；用户实际 WKWebView 的 `isSecureContext` 和 API 暴露仍需实测，权限声明也不能保证所有系统版本、签名和 TCC 状态均正常。[Wry 源码](https://github.com/tauri-apps/wry/blob/wry-v0.54.2/src/wkwebview/mod.rs)，[WebKit 源码](https://github.com/WebKit/WebKit/blob/main/Source/WebCore/page/SecurityOrigin.cpp)
- `remote-voice.js` 将能力缺失、安全上下文、权限拒绝、没有设备与选定设备失效分开处理。`remote-client.js` 在发起/接听含语音操作前检查能力，避免先把不支持语音的环境置为通话中。
- 保留初始 SDP 的 `sendrecv` 音频 transceiver，麦克风授权结束后通过 `replaceTrack` 挂接。真实回环确认在先协商、后采集的情况下，两端均能收到音频。
- 同一个通话的重复更新复用待完成的授权请求。旧通话授权结果会释放音轨；麦克风替换串行化，旧挂断不会晚于重拨并卸下新麦克风。新设备挂接完成前保持静音，并使用最新静音状态。切换麦克风失败会保留正在工作的输入。
- 语音失败回调携带通话 ID；排队的 `voice_end` 在发送前再次检查 ID，防止旧通话的异步失败挂断刚重拨的通话。语音失败不会停止屏幕共享。

## 自动验证

执行的命令：

```text
rtk node --test frontend/src/remote-voice.test.js frontend/src/remote-media.test.js frontend/src/remote-client.test.js
rtk python docs/verification/2026-10-10-remote-voice-loopback.py
```

Node 用例 16 项通过，覆盖权限缺失、重复授权、关闭后的迟到授权、旧通话与新通话竞争、慢挂断、轮询遗漏中间空闲态、静音与挂接竞争、设备切换失败、协商前后挂接及通话 ID 检查。

真实回环 8 项通过，细节和实际收发字节保存在 [JSON 报告](2026-10-10-remote-voice-loopback.json)。验证代码在 [回环脚本](2026-10-10-remote-voice-loopback.py)。测试建立隔离的临时 Chrome profile，静态服务器只绑定 `127.0.0.1:18943` 并只提供空测试页和实际 `remote-voice.js`；不启动或访问已安装的 Xchat，不访问聊天数据库。

| 验证场景 | 证据 |
| --- | --- |
| 未调用麦克风之前完成音频协商 | 双端 ICE/DTLS 连接成功，`currentDirection=sendrecv` |
| 授权后挂接音频 | 双端真实 Opus 入站、出站字节和包数增长 |
| 挂断后重拨 | 旧输入音轨结束，原 PeerConnection 保持连接，新输入双向传输 |
| 切换合成麦克风 | 旧音轨结束，新输入双向传输 |
| 切换到不存在的设备 | 真实媒体 API 返回设备约束错误，当前音轨仍存活且继续传输 |
| 挂断后返回授权结果 | 返回的音轨立即结束，不重新挂接 |
| 协商与资源生命周期 | 全程仅一份 offer、一份 answer；结束后所有采集音轨和连接关闭；无运行时异常 |

已使用 Python `plistlib` / JSON 解析验证权限声明与 entitlement 路径；相关 diff 空白检查通过。Rust 编译和整应用冒烟由本次远程优化的主任务统一执行，结果见主任务报告，不在这里重复声称。

## macOS 待完成的实机验收

必须使用包含本次 `Info.plist` 的新构建，而不是先前安装的旧应用。测试目标：

1. 检查 `.app/Contents/Info.plist` 的 `NSMicrophoneUsageDescription` 和签名的 `com.apple.security.device.audio-input`。开发态也重新构建，确认能弹出系统麦克风权限提示。
2. macOS 发起语音，Windows 接听：两端允许麦克风后，各说话验证对方能听到，持续至少 60 秒，检查两端 audio RTP 收发字节增长。
3. Windows 发起语音，macOS 接听：重复双向可听性与计数验证，确认不会接听后立即挂断。
4. 在 macOS 拒绝权限，确认错误明确且屏幕共享继续；重新授予权限后再次呼叫。
5. 挂断/重拨、授权提示仍未回应时挂断、静音、切换输入与断开输入设备，确认没有麦克风遗留占用或旧回调中断新通话。

HTTP 局域网地址本身不具备安全上下文时，浏览器不会开放麦克风；此路径会给出桌面客户端、HTTPS 或本机 localhost 的明确指引。它与 macOS 原生应用权限缺失是两个不同问题。
