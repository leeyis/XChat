# XChat 设备身份显示版本信息设计

日期：2026-08-18

## 1. 背景

设备身份面板（主机详情页）当前展示对方的「可用内存」，这个数值来自局域网发现
协议（UDP 广播）首次权威发现时的内存快照。对用户而言该参数意义不大，且无法据此
判断对方应用版本的新旧。

本次将「可用内存」替换为「版本」，让对方设备的真实应用版本号（如 `0.1.5`）可见，
从而判断对方版本是否过旧。应用版本号目前并未在发现协议中传输，需要新增传输。

## 2. 目标

- 设备身份面板将「可用内存」一行替换为「版本」一行。
- 版本号来自发现协议传输的对方真实应用版本。
- 版本号持久化到数据库，离线设备也能看到其最后一次上线时的版本。
- 旧版本设备（不发送版本号）显示「未提供」，不影响现有功能。

## 3. 非目标

- 不做自动的「版本过低」警告或版本比较逻辑，仅展示版本号。
- 不删除协议与数据库中的 `available_memory_mb` 字段，仅停止展示。
- 不改变发现协议的前 10 段结构，仅在末尾追加新字段。
- 不为 Android 或 headless Web 增加额外行为（协议改动天然覆盖三端）。

## 4. 方案

### 4.1 网络协议层（`src-tauri/src/network/discovery.rs`）

发现协议是 `|` 分隔字符串，历史上 `hostname`、`mac_address`、`capabilities` 均以
在末尾追加字段的方式扩展。本次沿用该模式，在第 11 段追加 `app_version`：

- `DiscoveryAnnouncement` 新增字段 `app_version: Option<String>`。
- `encode()` 格式串末尾追加一段 `{}`，输出
  `LANChat|ONLINE|...|capabilities|app_version`。
- `parse()` 用 `optional_part(parts.get(11))` 读取；旧消息只有 10 段时解析为 `None`。
- `local_announcement()` 增加 `app_version` 参数；announce 时传入本机版本，
  来源为编译期常量 `env!("CARGO_PKG_VERSION")`（即 `Cargo.toml` 的 `0.1.5`）。

向后兼容性：旧版本对方只读前 10 段，收到 11 段消息无感；本端收到旧版本 10 段消息，
`app_version` 为 `None`，显示「未提供」。

### 4.2 Peer 内存结构（`src-tauri/src/peers.rs`）

- `Peer` 新增 `app_version: Option<String>`，带
  `#[serde(default, skip_serializing_if = "Option::is_none")]`，与 `hostname` 同款。
- `load_from_db` 从 `UserRecord` 映射 `app_version`。
- `add_or_update_with_details` 增加 `app_version: Option<String>` 参数，仅在
  `authoritative` 时写入（与 `hostname`/`mac_address` 一致）。

### 4.3 数据库层（`src-tauri/src/db.rs`）

- `UserRecord` 新增 `app_version: Option<String>`。
- 迁移：`ALTER TABLE users ADD COLUMN app_version TEXT`，沿用现有迁移风格，
  忽略「列已存在」错误。
- `update_user_metadata` 增加 `app_version: Option<&str>` 参数，INSERT 与
  `ON CONFLICT` 更新子句中带上该列，用 `COALESCE` 保留首次权威值。
- `save_or_update_discovered_user` 增加 `app_version: Option<&str>` 参数并透传。
- `get_user_metadata` / `list_users_with_metadata` 的 SELECT 增加 `app_version` 列。
- 更新上述签名变更涉及的所有调用点。

### 4.4 WorkspaceDevice 桥接层（`src-tauri/src/workspace.rs`）

前端拿到的设备对象是 `WorkspaceDevice`（而非直接序列化 `Peer`），经 `device_from_peer`
与 `devices()` 两条路径构造。需为其增加 `app_version` 字段并映射：

- `WorkspaceDevice` 新增 `app_version: Option<String>`。
- `device_from_peer` 映射 `app_version: peer.app_version`。
- `devices()` 直接构造映射 `app_version: user.app_version`。

### 4.5 发现流程接线（`src-tauri/src/network/discovery.rs`）

announce 响应与 reply 响应两处处理循环中：

- `add_or_update_with_details(...)` 传入 `announcement.app_version.clone()`。
- `save_or_update_discovered_user(...)` 传入 `announcement.app_version.as_deref()`。
- `emit("new-peer", ...)` 的 JSON 增加 `"app_version"` 字段。

### 4.6 前端展示（`frontend/src/App.jsx`）

- 设备身份面板「可用内存」一行改为「版本」，显示
  `device.app_version || labels.notProvided`。
- i18n：删除 `availableMemory` 文案（中英文两处），复用已有的 `version`
  （「版本」/「Version」）文案。

## 5. 数据流

```text
本端 announce
  -> local_announcement(app_version = CARGO_PKG_VERSION)
  -> encode 追加第 11 段
  -> UDP 广播

对端收到 announce
  -> DiscoveryAnnouncement::parse 取 parts[11] 为 app_version
  -> add_or_update_with_details(..., app_version) 更新 Peer
  -> save_or_update_discovered_user(..., app_version) 持久化到 users 表
  -> devices() 经 WorkspaceDevice 映射 app_version 供前端拉取
  -> emit("new-peer", { ..., app_version }) 增量推送前端
  -> 设备身份面板显示 device.app_version 或「未提供」
```

## 6. 兼容性

- 协议变更：在末尾追加字段，`DISCOVERY_PROTOCOL_VERSION` 保持不变（仍是 2），
  因为旧版本可安全忽略新增的第 11 段，不存在解析歧义。
- 数据库变更：仅新增可空列，旧库经 `ALTER TABLE` 平滑升级，历史数据保留。
- 旧版本对方在本端显示「未提供」；本端版本对新旧对方均正常。

## 7. 测试

- `discovery.rs`：现有 `DiscoveryAnnouncement` 构造补 `app_version` 字段；新增
  round-trip 测试验证 `app_version` 的编码/解析，以及缺少第 11 段时的 `None` 回退。
- `peers.rs`：现有测试补 `app_version` 断言，验证 authoritative 传播与持久化读取。
- `db.rs`：现有测试补 `app_version` 断言，验证迁移列与首次权威值保留。
