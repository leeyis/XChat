# XChat Discovery Traffic and Interface Settings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 完成已批准阶段 A 的 A0/A1：停止高频 UDP 网段扫描，以真实接口和固定地址执行有预算的低频发现，并把接口选择按桌面原型完整接入设置页。

**Architecture:** 新增一个深 `discovery_policy` 模块作为接口清单、分类、用户选择、持久化和发送计划的唯一 seam。系统接口枚举是平台 adapter；`discovery.rs` 只消费已解析的发送计划并管理节奏、socket、去重、退避和指标。共享 `WorkspaceSettings` 把同一份发现状态暴露给 Tauri 与 Web，React 只提交设置并渲染后端事实。

**Tech Stack:** Rust 2021、Tokio、socket2、getifaddrs、SQLite/sqlx、Tauri 2、Axum、React 19、Vite 8、Node test runner。

**Spec:** `docs/plans/2026-08-18-xchat-network-presence-message-reliability-design.md`；已批准 UI：`ui-ref/xchat-desktop-prototype.html#settings-network`。

## Global Constraints

- 所有 shell 命令从仓库根目录运行并以 `rtk` 开头。
- 严格执行红灯 → 最小实现 → 绿灯；测试断言行为和结果，不断言源代码文本。
- 不升级 discovery v2 wire format；v3 sequence 属于后续 Presence 阶段。A0 用 `(peer_id, source, frame_digest)` 的短 TTL 兼容去重抑制同一轮重复包。
- 桌面/Web 共用 Rust 核心；Android 受限兼容路径使用窄 `cfg`，不得恢复 256 地址发送。
- 不新增 Tauri command：扩展现有 workspace 快照与 `update_settings`，避免扩大权限面。
- 不运行仓库级 `cargo fmt`；只对触及的 Rust 文件执行局部 rustfmt。
- 当前分支含用户对 `src/index.html` 的换行改动；构建产物更新时保留该改动，不清理、不 reset、不自动提交。
- A0/A1 之外的 Presence、消息状态、Outbox 与连接复用不在本计划范围。

---

## Task 1: 建立深 discovery policy 模块

**Files:**
- Create: `src-tauri/src/network/discovery_policy.rs`
- Modify: `src-tauri/src/network/mod.rs`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/Cargo.lock`

- [x] **Step 1: 写接口分类与选择红灯测试**

在新模块测试区用字面量接口 fixture 覆盖以下可观察行为：

1. `en0` / `Wi-Fi` / `Ethernet` 分类为 `physical_lan` 且默认开启。
2. `WireGuard` / `Tailscale` / `ZeroTier` 分类为 `mesh_vpn` 且默认开启。
3. `Meta Tunnel` / `Clash` / `tun0` 分类为 `proxy_tun`，Docker/WSL/Hyper-V/VMware 分类为 `virtual_machine`，两类默认关闭。
4. 未识别接口为 `unknown` 且默认关闭。
5. 顶层本地/VPN 总开关只改变有效启用状态，不删除逐接口 override；显式 override 能开启代理/虚拟接口。
6. 稳定 ID 由平台提供的 system name/GUID 产生，不含易变的接口 index 或当前 IPv4，因此重启、热插拔和地址变化后偏好仍命中。

- [x] **Step 2: 运行红灯**

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml --lib discovery_policy`

Expected: 新模块/类型/函数尚不存在而编译失败。

- [x] **Step 3: 实现最小类型与纯策略**

模块公开可序列化类型 `DiscoverySettings`、`InterfaceCategory`、`NetworkInterfaceView`、`DiscoveryNetworkSnapshot`；内部使用 `RawInterface`。`DiscoverySettings` 默认本地和组网 VPN 开启、override 为空。分类顺序必须先识别 mesh VPN，再识别代理 TUN，最后识别虚拟/物理/unknown，避免 WireGuard 的 TUN 实现被误排除。

- [x] **Step 4: 写发送目标与预算红灯测试**

用手算 fixture 验证：

- `192.168.10.178/23` 的唯一广播目标是 `192.168.11.255:8888`，另有 `224.0.0.167:8888`。
- `/31`、`/32` 或缺失/非法掩码不生成广播，但仍可生成该接口的组播。
- 关闭/未连接/回环/未选择接口没有目标。
- 40 张启用接口仍受常量预算限制，目标去重且不出现 `255.255.255.255`、`192.168.0.255..192.168.255.255` 或未绑定默认路由目标。

- [x] **Step 5: 实现发送计划**

提供窄接口 `build_send_plan(inventory, settings, port) -> DiscoverySendPlan`；每个目标都携带稳定接口 ID、源 IPv4、目标、`broadcast|multicast` 类型。实现 IPv4 前缀校验、定向广播、去重和固定发送预算。

- [x] **Step 6: 接入真实接口 adapter**

非 Android 使用 `getifaddrs = "0.6.2"` 枚举 name/index/flags/IPv4/netmask，过滤 loopback/unspecified 并按稳定 ID 聚合。Android 保留单独 fallback inventory，但只产出受预算接口目标，不生成 256 地址列表。系统 adapter 失败时返回空清单和可诊断错误，不回退默认路由扫描。

- [x] **Step 7: 运行模块测试并局部格式化**

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml --lib discovery_policy`

Run: `rtk rustfmt --edition 2021 src-tauri/src/network/discovery_policy.rs src-tauri/src/network/mod.rs`

Expected: 新模块测试全部通过。

---

## Task 2: 用测试固化发现节奏、回复去重和固定地址退避

**Files:**
- Modify: `src-tauri/src/network/discovery.rs`

- [x] **Step 1: 写 cadence 红灯测试**

新增测试验证首次三次发送发生在累计 `0ms / 400ms / 1500ms`；稳态抖动输入的上下界为 `24s / 36s`；检测到接口/设置变化后 cadence 重新进入立即三包突发。

- [x] **Step 2: 写 reply dedupe 红灯测试**

用同一 peer、来源和帧内容验证 TTL 内第一次允许、第二次拒绝；不同帧、不同来源或 TTL 后再次允许。期望值使用固定时间点和固定字符串，不复用生产摘要函数构造断言。

- [x] **Step 3: 写 fixed-peer backoff 红灯测试**

验证连续失败产生有上限的指数退避、退避窗口内不尝试、成功后恢复稳态尝试；一个固定地址失败不能阻塞其他地址。

- [x] **Step 4: 实现三个小状态机**

在 `discovery.rs` 内实现私有 `DiscoveryCadence`、`ReplyDeduper`、`FixedPeerRetryState`。它们接受时间/采样值并返回结果，不直接 sleep、查 DNS 或发 socket，作为 runtime 内部 seam。

- [x] **Step 5: 运行聚焦测试**

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml --lib discovery_`

Expected: cadence、dedupe、backoff 及原有协议测试全部通过。

---

## Task 3: A0 替换实际公告/回复运行时并增加诊断

**Files:**
- Modify: `src-tauri/src/network/discovery.rs`
- Modify: `src-tauri/src/network/discovery_policy.rs`

- [x] **Step 1: 先把旧扫描测试改成安全行为红灯**

删除旧的 `brute_force_fallback_still_covers_android_hotspot_ranges` 决策测试，替换为真实发送计划测试：任何平台计划都不得包含 256 个 `/24` 扫描目标；桌面计划只含被选择接口的定向广播/显式组播。运行后确认旧实现失败。

- [x] **Step 2: 重写 `start_announcing`**

公告循环每次从 policy 获取最新设置/接口/发送计划；启动立即发送三包，稳态 30 秒 ±20%，每 5 秒只检查一次接口指纹，网络或保存设置变化时重置突发。为每个计划项绑定源地址，组播明确设置 `IP_MULTICAST_IF` 与 TTL 1。删除 `get_smart_broadcast_addresses`、未绑定 limited-broadcast fallback 和 2 秒循环。

- [x] **Step 3: 改造固定地址单播**

保留 IP/域名和 60 秒 DNS 缓存；每个 endpoint 独立维护失败/下次尝试时间，DNS/解析/send 错误进入指数退避，成功回到稳态。固定地址受独立上限和全局每轮预算约束，不打印凭证或完整敏感信息。

- [x] **Step 4: 改造单次发送与接口缓存**

`send_single_broadcast` 复用 policy 计划，不再生成全网段目标。非 Android 的 `get_all_local_ips`、刷新和默认显示地址改从真实接口清单取值；Android fallback 保持窄 `cfg`。

- [x] **Step 5: 接入监听去重与一分钟指标**

桌面/Web listener 在解析有效 announcement 后先记录接收，再用共享规则跳过短窗口重复帧；只有首次非 reply 才写库、发事件并回复。累计并周期输出每接口/目标类型 attempts、success、failure，接收、去重、reply、排除原因和当前预算；日志不含消息正文、文件路径或 VPN 凭证。

- [x] **Step 6: 运行 A0 回归**

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml --lib`

Run: `rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib`

Run: `rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features web --bin lanchat-web`

Expected: 所有 Rust 测试、desktop lib 与 web bin 通过；日志/测试中不存在 260 目标发送行为。

---

## Task 4: A1 持久化发现设置并扩展共享快照

**Files:**
- Modify: `src-tauri/src/network/discovery_policy.rs`
- Modify: `src-tauri/src/workspace.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/web_server.rs`

- [x] **Step 1: 写设置 round-trip 红灯测试**

使用真实临时 SQLite 数据库验证：缺失 key 返回安全默认值；保存本地/VPN 开关和多个稳定 ID override 后读取完全一致；未知字段可忽略，损坏 JSON 返回错误或安全默认且不会 panic；超过 128 个 override、空/过长 ID 被拒绝。

- [x] **Step 2: 实现单一持久化入口**

用现有 `db::get_setting/set_setting` 保存版本化 JSON key `network.discovery.settings.v1`。实现 `load_settings`、`save_settings`、`network_snapshot`；成功保存后通知公告循环立即重建计划并进入突发。消失接口的 override 必须保留。

- [x] **Step 3: 扩展 workspace 快照**

`WorkspaceSettings` 增加 `discovery_settings` 与只读 `network_interfaces`。`get_snapshot` 从 policy 一次取得二者；序列化字段保持 snake_case，旧字段不改名。

- [x] **Step 4: 扩展 Tauri/Web 更新 seam**

现有 `commands::update_settings` 增加可选 `discovery_settings`；`UpdateSettingsRequest` 和 HTTP handler 同步增加。两条 adapter 都调用 `save_settings`，错误映射保持现有风格。兼容的 `get_settings`/`get_settings_http` 同样返回新字段，但不新增 command、route 或权限。

- [x] **Step 5: 跑共享核心回归**

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml --lib discovery_settings`

Run: `rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib`

Run: `rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features web --bin lanchat-web`

Expected: 设置持久化与两个入口通过。

---

## Task 5: 测试并实现前端设置契约

**Files:**
- Modify: `frontend/src/xchat.test.js`
- Modify: `frontend/src/xchat.js`

- [x] **Step 1: 写 workspace 归一化红灯测试**

通过真实 `createXChatModule` + Tauri snapshot adapter 启动，断言缺失发现字段时得到本地/VPN 开启、override 空、接口空；完整后端 fixture 保留类别、地址、推荐/排除和有效启用信息。

- [x] **Step 2: 写双 transport payload 红灯测试**

调用公开 `TauriAdapter.patchSettings` 与 `HttpWsAdapter.patchSettings`，验证前者发送 `discoverySettings`、后者发送 `discovery_settings`，值为完整合并后的设置；不通过 mock 自身存在性做断言，而验证 adapter 对 transport seam 产生的真实 payload。

- [x] **Step 3: 写 UI 选择 helper 红灯测试**

对 App 实际消费的 helper 验证：总开关关闭时接口无效但 override 保留；代理接口必须显式 override 才启用；“恢复推荐”回到本地/VPN true 与空 override；启用数量与全关 warning 由 form 的未保存状态即时计算。

- [x] **Step 4: 实现归一化、payload 与 helper**

保持旧快照兼容；`patchSettings` 只有发现配置变化时才触发现有 `update_settings`，同时与端口/路径合并为一次调用。helper 是 React 与测试共同使用的真实模块接口。

- [x] **Step 5: 运行前端红绿回归**

Run: `rtk node --test frontend/src/xchat.test.js`

Expected: 新旧 xchat 行为全部通过。

---

## Task 6: 按批准原型实现网络接口管理 UI

**Files:**
- Modify: `frontend/src/App.jsx`
- Modify: `frontend/src/styles.css`

- [x] **Step 1: 扩展 `SettingsWorkspace` 表单行为**

加入本地局域网、异地组网 VPN 总开关；接口摘要/展开；刷新网络列表；恢复推荐设置；固定地址入口；逐接口开关。保存 patch 增加 `discovery_settings`，深层 form 更新不能直接突变 `state.settings`。

- [x] **Step 2: 实现风险确认与禁用语义**

代理 TUN 从关闭切到开启时先显示原型中的内联风险确认，只有“仍然开启”才写 override；“保持关闭”不产生脏设置。类别总开关关闭时对应行 disabled 但保留 override。所有有效接口关闭时展示持续 warning。

- [x] **Step 3: 复用固定地址 modal**

从 `App` 向 `SettingsWorkspace` 传入 `onManageFixedPeers={() => setModal("endpoint")}`，复用现有 `EndpointModal`，不复制另一套固定地址 CRUD。

- [x] **Step 4: 补齐中英文文案与样式**

保持批准原型的信息层级、绿色推荐/橙色排除标签、连接状态点、compact 行与响应式布局；不改变设置页其余 section。控件具备 label、`aria-expanded`、`aria-live`、alert 和键盘可操作性。

- [x] **Step 5: 运行前端测试与临时生产构建**

Run: `rtk npm test`

Run: `rtk npm run build -- --outDir ../.tmp-xchat-a1-build --emptyOutDir`

Expected: Node 测试全部通过，Vite 可构建且暂不触碰 `src/index.html`。

---

## Task 7: 更新生产 bundle 并完成端到端验证

**Files:**
- Modify: `src/index.html`（保留用户已有换行风格，同时更新生成资源引用）
- Add/Remove: `src/assets/*`（仅 Vite 生成的 hash bundle）
- Modify: `task_plan.md`
- Modify: `findings.md`
- Modify: `progress.md`

- [x] **Step 1: 记录并核对生成目录状态**

Run: `rtk git status --short -- src/index.html src/assets`

确认 `src/assets` 没有用户改动；记录 `src/index.html` 当前换行/内容 diff，生产构建后确认该用户改动未被反向覆盖。

- [x] **Step 2: 生成正式静态 bundle**

Run: `rtk npm run build`

Expected: Vite 更新 `src/index.html` 和 hash assets；没有删除非生成文件。

- [x] **Step 3: 执行完整静态验证**

Run: `rtk npm test`

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml --lib`

Run: `rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib`

Run: `rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features web --bin lanchat-web`

Run: `rtk git diff --check`

- [x] **Step 4: 隔离启动与网络烟测**

Run: `rtk cargo tauri dev -- -- --port 18888 --db-path /tmp/lanchat-a0-a1`

验证启动日志只列真实接口和预算，首轮三包后进入低频稳态，日志不再显示 256 地址目标；若单实例锁转交给已安装应用，明确记录该环境限制，不终止用户进程。

- [x] **Step 5: 浏览器视觉与交互验收**

打开设置 → 网络，按批准原型验证 1440×900 和窄窗口：总开关、展开/收起、刷新、恢复推荐、TUN 风险确认、全关 warning、固定地址 modal、保存后刷新仍保持选择。检查浏览器控制台无错误并保存截图证据。

- [x] **Step 6: 更新执行记录并审查最终 diff**

逐项勾选本计划与 `task_plan.md`，在 `progress.md` 记录真实命令/结果，在 `findings.md` 记录平台差异。审查只包含 A0/A1、计划记录和必要生成产物；不提交、不清理用户改动。
