# 飞秋远程协助与双向语音实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 XChat 内交付局域网远程协助：双方明确同意后共享桌面和双向麦克风语音，并以单独二次授权提供受限远程控制。

**Architecture:** 远程会话控制面复用现有 WebSocket/TCP 连接，使用短生命周期内存 registry 管理邀请、同意、静音、停止和控制授权；媒体面使用 WebRTC 局域网连接传输视频与双向音频。React 通过 `XChatModule` 订阅状态，`RemoteAssistPanel` 只渲染真实能力与媒体状态；不保存音频/视频帧。

**Tech Stack:** React 19.2.8、WebRTC `RTCPeerConnection`/`getDisplayMedia`/`getUserMedia`、Tauri 2、Rust/Tokio、Axum WebSocket、平台条件编译的输入注入 API。

## Global Constraints

- 远程控制默认关闭，必须单独二次同意；远程方始终看到明显的共享/被控制状态和停止按钮。
- 会话结束、断线、权限撤销和窗口关闭必须清理 peer connection、track、控制 token 和 registry。
- 局域网直连，不引入公网 STUN/TURN，不记录媒体内容。
- 不支持屏幕或麦克风采集的平台只禁用对应能力并解释原因；不显示无效按钮。
- 远程控制只允许 pointer/keyboard/wheel 事件，拒绝 shell、文件系统、进程执行和任意脚本。
- 远程协助入口只能出现在聊天顶栏/设备详情动作，不新增主导航栏。

### Task 1: 媒体能力 spike 与平台矩阵

**Files:**
- Create: `frontend/src/remote-assist-capabilities.js`
- Modify: `frontend/src/xchat.js`
- Modify: `src-tauri/src/workspace.rs`
- Test: `frontend/src/remote-assist-capabilities.test.js`

**Interfaces:**
- `detectRemoteAssistCapabilities(runtime, navigatorLike, platformLike) -> { screen, microphone, speaker, remoteControl }`.
- `runtimeCapabilities` adds `remoteAssist`, `remoteAudio`, `remoteControl`.

- [ ] 写纯逻辑测试：Web 无 `getDisplayMedia`、无麦克风、权限拒绝和 Tauri 平台 capability false 时都返回明确原因。
- [ ] 在真实 macOS/Web 环境手工验证 `getDisplayMedia`、`getUserMedia({audio:true})` 和 track stop；记录权限提示、失败 error name 和可重复启动行为。
- [ ] 将能力结果写入 workspace snapshot，避免前端根据 user-agent 猜测“已可用”。
- [ ] 对 Windows/Linux/Android 先以 capability 矩阵报告结果，不在 spike 阶段伪造 native backend。
- [ ] 运行 `rtk node --test frontend/src/remote-assist-capabilities.test.js`、`rtk npm test` 和双 feature 编译。
- [ ] 提交 `feat: detect remote assist media capabilities`。

### Task 2: 远程会话控制面状态机

**Files:**
- Create: `src-tauri/src/remote_assist.rs`
- Modify: `src-tauri/src/network/protocol.rs`, `src-tauri/src/network/messaging.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/remote_assist.rs` tests, protocol serde tests

**Interfaces:**
- `RemoteSessionState = Invited | Accepted | Negotiating | Active | Stopping | Ended | Rejected | Expired`.
- `RemoteSessionRegistry::invite`, `accept`, `reject`, `set_muted`, `request_control`, `grant_control`, `revoke_control`, `stop`, `expire`。
- Wire events: `remote_session.invite.v1`, `accept.v1`, `reject.v1`, `signal.v1`, `mute.v1`, `control_request.v1`, `control_grant.v1`, `control_revoke.v1`, `stop.v1`。

- [ ] 写状态机红灯：非法状态迁移、重复 stop、重复 grant、过期后迟到 accept 都必须返回固定错误。
- [ ] 实现 registry，按 session ID、双方稳定设备 ID和 10 分钟 TTL 管理会话；不把媒体数据写 SQLite。
- [ ] 在协议中加入版本和能力摘要，保留旧 WebSocket/TCP 帧兼容。
- [ ] 在消息接收循环中校验发送者是否为会话双方，拒绝伪造 peer ID 或 HTTP body 地址。
- [ ] 运行 `rtk cargo test --manifest-path src-tauri/Cargo.toml --lib remote_assist` 与 network tests。
- [ ] 提交 `feat: add remote assist control state machine`。

### Task 3: Tauri/Web 信令 adapter

**Files:**
- Modify: `src-tauri/src/commands.rs`, `src-tauri/src/web_server.rs`
- Modify: `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`
- Modify: `src-tauri/permissions/commands.toml` and relevant capability JSON
- Modify: `frontend/src/xchat.js`
- Test: `frontend/src/xchat.test.js`, Rust handler tests

**Interfaces:**
- Adapter actions: `remote.invite`, `remote.accept`, `remote.reject`, `remote.signal`, `remote.mute`, `remote.requestControl`, `remote.grantControl`, `remote.revokeControl`, `remote.stop`。
- Snapshot `remoteSessions: RemoteSessionView[]`。
- HTTP: `POST /api/remote-sessions`, `POST /api/remote-sessions/:id/accept`, `POST /api/remote-sessions/:id/stop`, `POST /api/remote-sessions/:id/signal`。
- Tauri commands with equivalent names。

- [ ] 写 adapter 测试：HTTP 404/旧 Tauri command 返回 unsupported，不吞掉真实拒绝；远程事件统一为 `remote.session.changed`。
- [ ] 接入 Tauri/Web 薄入口，所有输入只接受稳定 `sessionId`/`peerId`/能力字段。
- [ ] 将邀请/同意/拒绝/停止/静音/授权状态聚合到 snapshot，前端不直接监听 WebSocket。
- [ ] 为错误映射增加 `remote_peer_rejected`、`remote_capability_unavailable`、`remote_permission_denied`、`remote_session_expired`。
- [ ] 运行 `rtk node --test frontend/src/xchat.test.js`、Rust tests 和 `rtk npm run build`。
- [ ] 提交 `feat: expose remote assist signaling through adapters`。

### Task 4: WebRTC 屏幕与双向音频

**Files:**
- Create: `frontend/src/remote-assist.js`
- Create: `frontend/src/RemoteAssistPanel.jsx`
- Modify: `frontend/src/App.jsx`, `frontend/src/styles.css`
- Test: `frontend/src/remote-assist.test.js`

**Interfaces:**
- `createRemotePeerConnection({ sendSignal, onState, onRemoteStream, onError })`。
- `startLocalTracks({ shareScreen, shareMicrophone }) -> { screenTrack, microphoneTrack, stream, stop }`。
- `toggleLocalMute(session, muted)` and `closeRemoteMediaSession(session)`。

- [ ] 写纯逻辑测试：邀请未接受不能创建 media session；重复 mute/stop 幂等；远端 track 到达后状态变为 active。
- [ ] 实现 WebRTC offer/answer/ICE signal 处理；媒体连接只通过局域网候选建立，不配置公网中继。
- [ ] 发起方按能力获取屏幕轨和麦克风轨，接受方至少获取麦克风轨；两端都显示视频预览和远端音频状态。
- [ ] 加入静音、关闭摄像/共享桌面、结束会话、权限失败和断线重连；所有 track 在组件卸载时 stop。
- [ ] 在聊天顶栏和设备详情添加“远程协助”入口；面板内显示对端名称/MAC、连接阶段、视频、双向语音、静音、结束。
- [ ] 运行 `rtk node --test frontend/src/remote-assist.test.js`、`rtk npm test`，并在双浏览器实例做手工 screen/audio smoke。
- [ ] 提交 `feat: add screen sharing and two-way audio`。

### Task 5: 远程控制二次授权

**Files:**
- Create: `src-tauri/src/remote_input.rs`
- Modify: `src-tauri/src/remote_assist.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/web_server.rs`
- Modify: `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`
- Modify: `src-tauri/permissions/commands.toml` and relevant capability JSON
- Modify: `frontend/src/remote-assist.js`, `frontend/src/RemoteAssistPanel.jsx`, `frontend/src/styles.css`
- Test: `src-tauri/src/remote_input.rs` tests, `frontend/src/remote-assist.test.js`

**Interfaces:**
- `validate_control_event(event) -> Result<ValidatedControlEvent, ControlError>`。
- `requestControl(sessionId)`, `grantControl(sessionId)`, `revokeControl(sessionId)`。
- `inject_control_event(sessionId, event)` only succeeds while session is `Active` and control grant is current。

- [ ] 写事件验证红灯：拒绝 shell/path/process 字段；限制坐标、按键名、滚轮范围和事件大小。
- [ ] 实现 macOS CoreGraphics input injection；Windows 使用 `SendInput`；Linux/Android 没有已验证 backend 时 capability false。
- [ ] 控制授权必须是独立事件；被控端显示持久横幅和“立即停止控制”，失焦/窗口关闭自动 revoke。
- [ ] 前端默认不采集/发送控制事件；用户点击“允许远程控制”后才绑定 pointer/keyboard listener。
- [ ] 运行 desktop/web 编译、远程状态机测试和手工双实例控制 smoke；无权限时验证降级文案。
- [ ] 提交 `feat: add consented remote input control`。

### Task 6: 远程协助视觉回归与交付门槛

**Files:**
- Inspect: `ui-ref/DESIGN.md`, `ui-ref/ui_kits/app/chat-workspace.html`, `frontend/src/styles.css`
- Test: `frontend/src/remote-assist.test.js`

- [ ] 对 360×800、820×1180、1024×768、1366×768、1440×900、1920×1080 截图检查远程面板没有横向溢出。
- [ ] 验证邀请、拒绝、权限失败、共享中、静音、远控授权、断线、停止、过期 9 个状态都有明确文案和恢复动作。
- [ ] 运行 `rtk npm test && rtk npm run build`、Rust library tests、desktop library/bin 与 web bin checks。
- [ ] 两个隔离 Web 实例做信令/拒绝/结束联调；桌面端验证屏幕录制和麦克风权限清理。
- [ ] 执行 `rtk git diff --check` 并提交 `test: verify remote assist milestone`。
