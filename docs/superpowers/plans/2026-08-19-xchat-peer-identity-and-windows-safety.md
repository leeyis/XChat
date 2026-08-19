# XChat Peer Identity and Windows Network Safety Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task on the current branch. Do not create a worktree or commit unless the user asks.

**Goal:** 消除动态 IP 被复用时把消息交给错误设备的窗口；离线时完全停止网络发送并显示“等待对方上线”；手工固定地址必须先验证设备身份；同时让 Windows 按真实入站网卡回复发现包并删除不再需要的旧版兼容心跳。

**Architecture:** 继续以数据库中持久化的 UUID 作为设备主身份，IP 只作为可变连接地址，MAC/hostname 仅作辅助展示。所有 Rust 对等 WebSocket 发送在写入消息正文前，以期望 UUID 建立带目标身份的握手，并校验服务端返回的本机 UUID；不匹配、缺失或离线一律不写正文。手工固定地址通过只返回身份元数据的探测接口验证，后端在保存时再次核验并把 endpoint 与 UUID 绑定。Windows discovery listener 使用 `IP_PKTINFO`/`WSARecvMsg` 取得入站 interface index，复用现有按接口选择回复源地址的逻辑。

**Tech Stack:** Rust 2021、Tokio、Axum、tokio-tungstenite、reqwest、windows-sys、SQLite/sqlx、Tauri 2、React 19、Vite 8、Node test runner。

**Spec:** `docs/plans/2026-08-18-xchat-network-presence-message-reliability-design.md`；已批准 UI：`ui-ref/xchat-desktop-prototype.html`。

## Global Constraints

- 所有 shell 命令从仓库根目录运行并以 `rtk` 开头。
- 严格执行红灯 → 最小实现 → 绿灯；测试断言可观察行为，不断言源代码文本。
- 用户明确要求在当前分支实现；保留工作区已有修改，不 reset、不 clean、不自动提交。
- Android 本轮不优化、不改生成工程；共享核心若自然可用则保留，平台专属实现只新增 Windows 窄 `cfg`。
- 不兼容缺少身份握手的旧客户端；身份响应头缺失时拒绝发送消息正文。
- 不用 MAC 做主键。随机化、缺失、跨网卡和权限差异使 MAC 不适合可靠身份；UUID 是现有会话、数据库和协议共同使用的稳定身份。
- 不运行仓库级 `cargo fmt`；只格式化触及的 Rust 文件。
- UI 必须忠实实现已批准原型；离线消息状态的最终文案为“等待对方上线”。

---

## Task 1: 固化安全语义与原型

**Files:**
- Modify: `ui-ref/xchat-desktop-prototype.html`
- Modify: `task_plan.md`

- [x] **Step 1: 将原型离线消息和离线文件统一为“等待对方上线”**

- [x] **Step 2: 记录用户已经批准的行为**

离线时不尝试已知旧 IP；重新上线后必须先确认 UUID 才发出正文。手工地址先“测试连接”，展示设备名、设备 ID、hostname、辅助网卡地址和响应耗时；身份不一致时禁止保存。

---

## Task 2: 删除旧协议兼容心跳并补齐 Windows ingress metadata

**Files:**
- Modify: `src-tauri/src/network/discovery.rs`
- Modify: `src-tauri/src/peers.rs`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/Cargo.lock`

- [x] **Step 1: 写兼容桥删除红灯测试**

把 runtime 唤醒时间测试改为只接受 cadence deadline 与 policy-check deadline；断言不再存在 legacy-heartbeat deadline 对调度结果的影响。保留 75 秒 stale timeout 的独立测试，并把名称/注释改为 Presence 离线门禁而非兼容 watchdog。

- [x] **Step 2: 运行红灯**

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml --lib discovery_runtime_wake_delay`

Expected: 旧函数仍要求 legacy deadline，编译或断言失败。

- [x] **Step 3: 删除兼容心跳状态与发送路径**

删除 `MAX_LEGACY_COMPAT_PEERS`、compat interval/budget、target 选择、按 peer 发送、runtime deadline 和对应 metrics/tests。保留 v2 discovery announcement、启动突发、30 秒稳态发送与 75 秒离线判断。

- [x] **Step 4: 写 Windows packet-info 红灯测试**

增加 Windows-only 控制消息解析测试：构造包含 `IN_PKTINFO.ipi_ifindex` 的 ancillary buffer，断言得到正确 interface index；空/截断/其他 level/type 返回 `None`。

- [x] **Step 5: 运行 Windows 红灯编译**

Run: `rtk cargo check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc --no-default-features --features web --tests`

Expected: Windows packet-info helper/依赖尚不存在而失败。

- [x] **Step 6: 实现 Windows `IP_PKTINFO` 接收**

新增目标依赖 `windows-sys`。用 `setsockopt(IPPROTO_IP, IP_PKTINFO)` 开启元数据，用 `WSAIoctl(SIO_GET_EXTENSION_FUNCTION_POINTER, WSAID_WSARECVMSG)` 缓存 `WSARecvMsg`，在 Tokio `async_io(READABLE, ...)` 中解析 `IN_PKTINFO.ipi_ifindex`。将原 `not(unix)` fallback 收窄为 `not(any(unix, windows))`；现有 reply source selection 直接消费新的 ingress index。

- [x] **Step 7: 局部格式化并跑聚焦回归**

Run: `rtk rustfmt --edition 2021 src-tauri/src/network/discovery.rs src-tauri/src/peers.rs`

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml --lib discovery_`

---

## Task 3: 在正文发送前验证目标设备 UUID

**Files:**
- Modify: `src-tauri/src/network/messaging.rs`
- Modify: `src-tauri/src/network/protocol.rs`
- Modify: `src-tauri/src/web_server.rs`
- Modify: `src-tauri/src/workspace.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: any focused sender required by compiler errors

- [x] **Step 1: 写 WebSocket 身份握手红灯测试**

用本地 Axum server 覆盖：期望 UUID 匹配时发送成功；UUID 不匹配时服务端在 upgrade 前拒绝；响应缺少身份头、返回其他 UUID、连接超时或拒绝时，客户端不得写入消息正文。

- [x] **Step 2: 运行红灯**

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml --lib verified_websocket`

Expected: verified transport API 尚不存在而失败。

- [x] **Step 3: 实现服务端身份门禁**

对 peer WebSocket 使用 `?target_id=<expected UUID>`；server 在 upgrade 前读取本机 UUID，目标不匹配返回冲突响应，匹配时在握手响应加入 `X-XChat-Device-Id`。不带 target 的本机 Web UI event subscription 仍可连接，但不能被 Rust peer sender 当作已验证连接。

- [x] **Step 4: 实现客户端双向校验并加超时**

将底层发送 seam 改为 `send_json_via_ws(peer_addr, expected_peer_id, json)`。连接时携带目标 UUID，校验响应身份头完全相等后才写 JSON；缺失/错误/超时均返回错误。借助签名变更让编译器找出所有旧调用点。

- [x] **Step 5: 贯穿消息、控制帧、重发和群组扇出**

所有 direct/group protocol sender 传入 `Peer.id`。离线 peer 保持 pending，不调用 transport；发送失败只更新失败/离线状态，绝不退回无身份校验的旧地址发送。

- [x] **Step 6: 跑消息核心回归**

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml --lib messaging`

Run: `rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib`

Run: `rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features web --bin lanchat-web`

---

## Task 4: 手工地址身份探测、验证后保存和固定地址绑定

**Files:**
- Modify: `src-tauri/src/db.rs`
- Modify: `src-tauri/src/workspace.rs`
- Modify: `src-tauri/src/network/discovery.rs`
- Modify: `src-tauri/src/network/discovery_policy.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/web_server.rs`
- Modify: `src-tauri/src/main.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/permissions/commands.toml`
- Modify: relevant `src-tauri/capabilities/*.json`

- [x] **Step 1: 写固定地址 record 与迁移红灯测试**

验证结构化 record round-trip；保存项至少包含 endpoint、device_id、name/hostname、辅助 MAC/app version 与 last_verified_at。旧 raw endpoint 读取为“未验证”且不得进入自动 discovery target；移除仍按 endpoint 工作。

- [x] **Step 2: 写身份探测与保存红灯测试**

用本地 HTTP fixture 覆盖成功、无法连接、超时、无效 payload、期望 UUID 不匹配。保存 API 必须自行重新探测；只有本次 UUID 匹配才落库，不能只信任前端上一次结果。

- [x] **Step 3: 实现远端只读身份 endpoint**

新增 `GET /api/peer_identity`，只返回本机 UUID、名称、hostname、辅助 MAC、app version 和当前服务地址，不接收或返回聊天内容。

- [x] **Step 4: 实现 Tauri/Web 本地探测 seam**

新增 `test_custom_peer` Tauri command 与 `POST /api/test_custom_peer`，规范化 endpoint，使用短连接/总超时访问远端身份 endpoint，返回结构化结果和 latency。同步 command 注册、permission 与 capability。

- [x] **Step 5: 保存已验证 record 并绑定 discovery 回复身份**

`add_custom_peer` 接受期望 UUID、在后端重验后写结构化 record。discovery 只使用已验证 endpoint；固定地址收到的 announcement UUID 若不等于绑定 UUID则丢弃，不更新 peer 地址也不触发发送。保存成功后唤醒 discovery policy。

- [x] **Step 6: 跑持久化/API/固定地址回归**

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml --lib custom_peer`

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml --lib peer_identity`

---

## Task 5: 实现已批准的 React 交互与文案

**Files:**
- Modify: `frontend/src/xchat.js`
- Modify: `frontend/src/xchat.test.js`
- Modify: `frontend/src/App.jsx`
- Modify: `frontend/src/styles.css`
- Rebuild: `src/index.html`
- Rebuild: `src/assets/*`

- [x] **Step 1: 写 adapter 与验证状态红灯测试**

覆盖 Tauri/HTTP `testEndpoint` payload、带 expected device ID 的 save payload、结构化 fixed-peer normalization，以及输入变化后旧测试结果立即失效、未成功/身份不符不能保存的纯 helper。

- [x] **Step 2: 运行红灯**

Run: `rtk npm test`

Expected: 新 action/helper/adapter contract 尚不存在而失败。

- [x] **Step 3: 扩展 `XChatModule` adapters/actions**

增加 `device.testEndpoint`；`device.saveEndpoint` 传 expectedDeviceId；快照把 fixed peers 规范化为结构对象。Tauri 与 HTTP 保持同一返回形状。

- [x] **Step 4: 实现测试连接弹窗**

输入合法后允许“测试连接”；展示 testing、成功、无法连接和身份变化状态。成功显示设备名、设备 ID、hostname、辅助网卡地址、当前地址、延迟；只有当前输入的成功测试可保存。已有地址显示“身份已确认”或“尚未验证”，支持重新测试与删除。

- [x] **Step 5: 实现聊天与设备详情安全反馈**

direct peer 离线时显示“已停止发送，防止发错设备”；说明消息只保存在本机，状态为“等待对方上线”，不提供会向旧地址发送的重试动作。设备详情明确区分“设备 ID”“当前地址”“网卡地址（辅助）”和身份核验结果；发送成功中间态使用“已发出”。

- [x] **Step 6: 跑前端测试并构建生产资源**

Run: `rtk npm test`

Run: `rtk npm run build`

---

## Task 6: 全量验证与交付检查

- [x] **Step 1: Rust 全测试与双 feature 编译**

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml --lib`

Run: `rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib`

Run: `rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features web --bin lanchat-web`

- [x] **Step 2: Windows target 编译**

Run: `rtk cargo check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc --no-default-features --features web --tests`

- [x] **Step 3: 隔离桌面实例烟测**

Run: `rtk cargo tauri dev -- --port 18888 --db-path /tmp/lanchat-peer-identity-smoke`

验证离线停发、重新上线身份通过后发送、手工地址测试/保存/重测、身份不符禁止保存和设置页布局。

- [x] **Step 4: 检查 diff 与生成物**

确认没有 Android 生成工程的新增改动，没有无关格式化，没有旧 compatibility heartbeat，生产 bundle 与 React source 一致；更新 `task_plan.md`、`findings.md`、`progress.md`。

**验证记录（2026-08-19）：** 前端 96/96、Rust library 111/111、macOS desktop lib、Web bin 与 Windows MSVC desktop lib target 均通过。Windows 在 macOS 上以 Homebrew SQLite 路径和仅供 `cargo check` 的临时资源占位器越过本机缺失的 `llvm-rc`，WinSock 代码由真实 `x86_64-pc-windows-msvc` target 完整类型检查；没有宣称 Windows 真机运行。Tauri debug binary 已构建并启动，但被本机既有 Xchat 单实例接管；隔离 headless Web 实例进一步实测身份查询、测试连接、错误 UUID 禁止保存和正确 UUID 结构化保存，随后删除临时数据库。
