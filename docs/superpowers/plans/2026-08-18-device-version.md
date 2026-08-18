# 设备身份显示版本信息 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 设备身份面板用「版本」替换「可用内存」，显示对方的真实应用版本号。

**Architecture:** 在局域网发现协议的 `|` 分隔字符串末尾追加第 12 段 `app_version`，本端用编译期常量 `CARGO_PKG_VERSION` 发送；对端解析后存入 `Peer` 结构并持久化到 `users` 表新列，前端展示 `device.app_version`，缺失时显示「未提供」。旧设备只发 11 段，降级为「未提供」。

**Tech Stack:** Rust（Tauri 2 后端）、SQLite（sqlx）、React（Vite 前端）

**关键约定：**
- 测试命令：`cargo test --manifest-path src-tauri/Cargo.toml --lib`
- 编译检查：`cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib`
- 前端构建：`npm run build`
- `app_version` 全程为 `Option<String>`，仅在 `authoritative` 时写入；缺失统一显示「未提供」。

---

### Task 1: 发现协议新增 `app_version` 字段（parse/encode/构造）

**Files:**
- Modify: `src-tauri/src/network/discovery.rs`

本任务只改协议层：`DiscoveryAnnouncement` 结构体、`parse()`、`encode()`、`local_announcement()` 及其四处调用点。完成后协议测试通过，其余模块尚未消费该字段。

- [ ] **Step 1: 写失败测试**

在 `discovery.rs` 的 `#[cfg(test)]` 模块中，找到现有测试 `discovery_extension_round_trips_after_legacy_prefix`（约 911-930 行），在其后新增一个 round-trip 测试：

```rust
#[test]
fn app_version_round_trips_and_defaults_to_none() {
    let with_version = DiscoveryAnnouncement::parse(
        "LANChat|ONLINE|peer-1|Alice|8888|512|0|2|alice-mac|01:02:03:04:05:06|group_chat|0.1.5",
    )
    .unwrap()
    .unwrap();
    assert_eq!(with_version.app_version.as_deref(), Some("0.1.5"));
    assert_eq!(with_version.encode().split('|').count(), 12);

    let legacy = DiscoveryAnnouncement::parse(
        "LANChat|ONLINE|peer-1|Alice|8888|512|0|2|alice-mac|01:02:03:04:05:06|group_chat",
    )
    .unwrap()
    .unwrap();
    assert_eq!(legacy.app_version, None);
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib app_version_round_trips`
Expected: 编译失败，报 `no field 'app_version' on type 'DiscoveryAnnouncement'`。

- [ ] **Step 3: 实现 `DiscoveryAnnouncement` 字段、`parse`、`encode`**

在结构体定义（约 25-36 行）`capabilities` 字段后追加：

```rust
    pub capabilities: Vec<String>,
    pub app_version: Option<String>,
```

在 `parse()`（约 66-85 行）的 `Ok(Some(Self { ... }))` 中，`capabilities` 之后追加：

```rust
            app_version: optional_part(parts.get(11)),
```

将 `encode()`（约 88-101 行）整体替换为：

```rust
    pub fn encode(&self) -> String {
        format!(
            "LANChat|ONLINE|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
            self.peer_id,
            self.name,
            self.port,
            self.available_memory_mb,
            u8::from(self.is_reply),
            self.protocol_version,
            self.hostname.as_deref().unwrap_or_default(),
            self.mac_address.as_deref().unwrap_or_default(),
            self.capabilities.join(","),
            self.app_version.as_deref().unwrap_or_default()
        )
    }
```

- [ ] **Step 4: 实现 `local_announcement` 及四处调用点**

将 `local_announcement()`（约 232-253 行）签名与结构体构造改为：

```rust
fn local_announcement(
    peer_id: String,
    name: String,
    port: u16,
    available_memory_mb: u64,
    is_reply: bool,
    hostname: Option<String>,
    mac_address: Option<String>,
    app_version: Option<String>,
) -> DiscoveryAnnouncement {
    DiscoveryAnnouncement {
        peer_id,
        name,
        port,
        available_memory_mb,
        is_reply,
        protocol_version: DISCOVERY_PROTOCOL_VERSION,
        hostname,
        mac_address,
        capabilities: DISCOVERY_CAPABILITIES
            .iter()
            .map(|capability| (*capability).to_string())
            .collect(),
        app_version,
    }
}
```

在 announce 心跳调用点（约 392-401 行）追加参数：

```rust
        let msg = local_announcement(
            user_id.clone(),
            username,
            port,
            available_memory_mb,
            false,
            hostname.clone(),
            mac_address.clone(),
            Some(env!("CARGO_PKG_VERSION").to_string()),
        )
        .encode();
```

在 reply 调用点（约 622-631 行）追加参数：

```rust
                    let reply = local_announcement(
                        my_id.clone(),
                        reply_name,
                        port,
                        0,
                        true,
                        hostname.clone(),
                        mac_address.clone(),
                        Some(env!("CARGO_PKG_VERSION").to_string()),
                    )
                    .encode();
```

同样在另外两处调用点追加 `Some(env!("CARGO_PKG_VERSION").to_string())` 作为最后一个参数：

- web 版 reply（约 751 行）：与桌面版 reply 结构相同，`is_reply = true`。
- `send_single_broadcast`（约 870 行）：`is_reply = false`，实参为 `user_id, username, port, 0, false, hostname, mac_address`。

（`local_announcement` 共 4 处调用点：心跳、桌面版 reply、web 版 reply、`send_single_broadcast`，签名加了必填参数后需全部更新才能编译。）

- [ ] **Step 5: 修现有构造测试**

在现有测试 `discovery_extension_round_trips_after_legacy_prefix`（约 913-923 行）的 `DiscoveryAnnouncement { ... }` 字面量中，`capabilities` 字段后追加 `app_version: Some("0.1.5".into()),`。

- [ ] **Step 6: 跑测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib`
Expected: 全部通过（含新增 `app_version_round_trips_and_defaults_to_none`）。

- [ ] **Step 7: 提交**

```bash
git add src-tauri/src/network/discovery.rs
git commit -m "feat(protocol): 发现协议新增 app_version 字段"
```

---

### Task 2: Peer 结构新增 `app_version` 并传播

**Files:**
- Modify: `src-tauri/src/peers.rs`
- Modify: `src-tauri/src/network/discovery.rs`（两处 `add_or_update_with_details` 调用点补参数）

本任务改 `Peer` 结构、`load_from_db` 映射、`add_or_update_with_details` 签名与传播，并同步 `discovery.rs` 两处调用点。

- [ ] **Step 1: 写失败测试**

在 `peers.rs` 的测试 `replies_do_not_replace_authoritative_discovery_metadata`（约 270-334 行）中，为每个 `add_or_update_with_details(...)` 调用补一个新参数（在 `capabilities` 之后、`authoritative` 之前）：

- 第一处（273-283，reply，`false`）：`None,`
- 第二处（284-294，authoritative，`true`）：`Some("0.1.5".into()),`
- 第三处（295-305，reply，`false`）：`None,`
- 第四处（306-316，authoritative，`true`）：`Some("0.1.5".into()),`
- 第五处（317-327，非权威清空，`false`）：`None,`

在断言区（330-333 行）追加：

```rust
        assert_eq!(peer.app_version.as_deref(), Some("0.1.5"));
```

在测试 `authoritative_empty_capabilities_clear_stale_parallel_v2`（约 336-364 行）中，为两处 `add_or_update_with_details(...)`（339-349、350-360）在 `capabilities` 之后、`authoritative` 之前各补 `Some("0.1.5".into()),`。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib`
Expected: 编译失败，报 `this function takes 9 arguments but 8 were supplied` / `no field 'app_version' on type 'Peer'`。

- [ ] **Step 3: 实现 `Peer` 字段**

在 `peers.rs` 的 `Peer` 结构体（约 13-31 行）`capabilities` 字段后追加：

```rust
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_version: Option<String>,
```

- [ ] **Step 4: 实现 `load_from_db` 映射**

在 `load_from_db`（约 62-74 行）的 `Peer { ... }` 构造中，`capabilities: Vec::new(),` 之后追加：

```rust
                app_version: user.app_version,
```

- [ ] **Step 5: 实现 `add_or_update_with_details` 传播**

将 `add_or_update_with_details` 签名（约 110-121 行）改为在 `capabilities` 与 `authoritative` 之间插入 `app_version: Option<String>`：

```rust
    #[allow(clippy::too_many_arguments)]
    pub fn add_or_update_with_details(
        &self,
        id: String,
        name: String,
        addr: String,
        available_memory_mb: u64,
        hostname: Option<String>,
        mac_address: Option<String>,
        discovery_source: Option<String>,
        capabilities: Vec<String>,
        app_version: Option<String>,
        authoritative: bool,
    ) -> bool {
```

在已存在 peer 的更新分支（约 139-150 行）的 `if authoritative { ... }` 块内，`peer.capabilities = capabilities;` 之后追加：

```rust
                if app_version.is_some() {
                    peer.app_version = app_version;
                }
```

在新用户构造（约 163-187 行）的 `capabilities: if authoritative { capabilities } else { Vec::new() },` 之后追加：

```rust
                app_version: if authoritative { app_version } else { None },
```

- [ ] **Step 6: 实现 `add_or_update_with_memory` 传递 `None`**

在 `add_or_update_with_memory`（约 89-107 行）调用 `add_or_update_with_details` 的参数列表中，`Vec::new(),` 之后、`false,` 之前补 `None,`：

```rust
        self.add_or_update_with_details(
            id,
            name,
            addr,
            available_memory_mb,
            None,
            None,
            Some("lan".to_string()),
            Vec::new(),
            None,
            false,
        )
```

- [ ] **Step 7: 同步 `discovery.rs` 两处调用点**

在 `discovery.rs` 第一处（约 527-537 行）`add_or_update_with_details(...)` 中，`announcement.capabilities.clone(),` 之后、`announcement.has_authoritative_metadata(),` 之前补：

```rust
                    announcement.app_version.clone(),
```

在第二处（约 679-689 行）同样位置补：

```rust
                    announcement.app_version.clone(),
```

- [ ] **Step 8: 跑测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib`
Expected: 全部通过（peers 测试断言 `app_version == Some("0.1.5")`）。

- [ ] **Step 9: 提交**

```bash
git add src-tauri/src/peers.rs src-tauri/src/network/discovery.rs
git commit -m "feat(peers): Peer 结构新增 app_version 并传播"
```

---

### Task 3: 数据库层新增 `app_version` 列并持久化

**Files:**
- Modify: `src-tauri/src/db.rs`
- Modify: `src-tauri/src/network/discovery.rs`（两处 `save_or_update_discovered_user` 调用点补参数）

本任务改 `UserRecord`、迁移、`update_user_metadata`、`save_or_update_discovered_user`、`get_user_metadata`/`list_users_with_metadata` 的 SELECT，并同步调用点。

- [ ] **Step 1: 写失败测试**

在 `db.rs` 测试 `discovered_user_metadata_keeps_the_first_authoritative_memory_snapshot`（约 3549-3638 行）中，为每处 `save_or_update_discovered_user(...)`（3573、3594、3607、3620）在 `Some("lan"),` 之后、`authoritative` 参数之前补一个新参数：

- 3573（reply，`false`）：`None,`
- 3594（authoritative，`true`）：`Some("0.1.5"),`
- 3607（reply，`false`）：`None,`
- 3620（authoritative，`true`）：`Some("0.1.5"),`

在断言区（3634-3637 行）追加：

```rust
        assert_eq!(peer.app_version.as_deref(), Some("0.1.5"));
```

在测试 `machine_name...`（约 4236-4244 行）的 `update_user_metadata(...)` 调用中，`Some("udp"),` 之后补 `Some("0.1.5"),`。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib`
Expected: 编译失败，报函数参数数量不匹配 / `no field 'app_version' on type 'UserRecord'`。

- [ ] **Step 3: 实现 `UserRecord` 字段**

在 `db.rs` 的 `UserRecord`（约 122-134 行）`discovery_source` 字段后追加：

```rust
    pub discovery_source: Option<String>,
    pub app_version: Option<String>,
```

- [ ] **Step 4: 实现迁移**

在 `users` 表 ALTER 迁移区（约 421-432 行），`discovery_source` 迁移之后追加：

```rust
    let _ = sqlx::query("ALTER TABLE users ADD COLUMN app_version TEXT")
        .execute(&pool)
        .await;
```

- [ ] **Step 5: 实现 `update_user_metadata`**

将 `update_user_metadata`（约 3308-3346 行）整体替换为：

```rust
pub async fn update_user_metadata(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    user_id: &str,
    hostname: Option<&str>,
    mac_address: Option<&str>,
    discovery_source: Option<&str>,
    app_version: Option<&str>,
) -> Result<UserRecord, String> {
    if user_id.trim().is_empty() {
        return Err("user id is required".to_string());
    }
    let now = unix_timestamp();
    sqlx::query(
        "INSERT INTO users
            (id, name, addr, last_seen, is_offline, available_memory_mb,
             hostname, mac_address, discovery_source, app_version)
         VALUES (?, ?, '', ?, 1, 0, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
            hostname = COALESCE(excluded.hostname, users.hostname),
            mac_address = COALESCE(excluded.mac_address, users.mac_address),
            discovery_source = COALESCE(excluded.discovery_source, users.discovery_source),
            app_version = COALESCE(excluded.app_version, users.app_version)",
    )
    .bind(user_id)
    .bind(user_id)
    .bind(now)
    .bind(hostname.map(str::trim).filter(|value| !value.is_empty()))
    .bind(mac_address.map(str::trim).filter(|value| !value.is_empty()))
    .bind(
        discovery_source
            .map(str::trim)
            .filter(|value| !value.is_empty()),
    )
    .bind(
        app_version
            .map(str::trim)
            .filter(|value| !value.is_empty()),
    )
    .execute(pool)
    .await
    .map_err(|e| format!("保存设备元数据失败: {}", e))?;

    get_user_metadata(pool, user_id)
        .await?
        .ok_or_else(|| "user metadata was not saved".to_string())
}
```

- [ ] **Step 6: 实现 `save_or_update_discovered_user` 透传**

将 `save_or_update_discovered_user`（约 1158-1187 行）签名与 `update_user_metadata` 调用改为：

```rust
#[allow(clippy::too_many_arguments)]
pub async fn save_or_update_discovered_user(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    id: &str,
    name: &str,
    addr: &str,
    available_memory_mb: u64,
    hostname: Option<&str>,
    mac_address: Option<&str>,
    discovery_source: Option<&str>,
    app_version: Option<&str>,
    authoritative: bool,
) -> Result<(), String> {
    save_or_update_user(
        pool,
        id.to_string(),
        name.to_string(),
        addr.to_string(),
        false,
        if authoritative {
            available_memory_mb
        } else {
            0
        },
    )
    .await?;
    if authoritative {
        update_user_metadata(pool, id, hostname, mac_address, discovery_source, app_version).await?;
    }
    Ok(())
}
```

- [ ] **Step 7: 实现 SELECT 列**

将 `get_user_metadata`（约 3382-3384 行）的 SELECT 改为：

```sql
        "SELECT id, name, addr, last_seen, is_offline, available_memory_mb,
                hostname, mac_address, remark, discovery_source, app_version
         FROM users WHERE id = ?",
```

将 `list_users_with_metadata`（约 3395-3398 行）的 SELECT 改为：

```sql
        "SELECT id, name, addr, last_seen, is_offline, available_memory_mb,
                hostname, mac_address, remark, discovery_source, app_version
         FROM users ORDER BY last_seen DESC",
```

- [ ] **Step 8: 同步 `discovery.rs` 两处调用点**

在 `discovery.rs` 第一处（约 540-551 行）`save_or_update_discovered_user(...)` 中，`Some("lan"),` 之后、`announcement.has_authoritative_metadata(),` 之前补：

```rust
                    announcement.app_version.as_deref(),
```

在第二处（约 692-703 行）同样位置补：

```rust
                    announcement.app_version.as_deref(),
```

- [ ] **Step 9: 跑测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib`
Expected: 全部通过（db 测试断言 `app_version == Some("0.1.5")`）。

- [ ] **Step 10: 提交**

```bash
git add src-tauri/src/db.rs src-tauri/src/network/discovery.rs
git commit -m "feat(db): users 表新增 app_version 列并持久化"
```

---

### Task 4: WorkspaceDevice 层新增 `app_version`

**Files:**
- Modify: `src-tauri/src/workspace.rs`

前端拿到的 `device` 对象是 `WorkspaceDevice`（不是直接 `Peer`）。本任务改 `WorkspaceDevice` 结构及两处构造（`device_from_peer` 与 `devices()` 直接构造），使前端 `device.app_version` 可用。无单元测试，靠编译检查验证。

- [ ] **Step 1: 实现 `WorkspaceDevice` 字段**

在 `workspace.rs` 的 `WorkspaceDevice`（约 89-102 行）`capabilities` 字段后追加：

```rust
    pub capabilities: Vec<String>,
    pub app_version: Option<String>,
```

- [ ] **Step 2: 实现 `device_from_peer` 映射**

在 `device_from_peer`（约 209-223 行）的 `capabilities: peer.capabilities,` 之后追加：

```rust
        app_version: peer.app_version,
```

- [ ] **Step 3: 实现 `devices()` 直接构造映射**

在 `devices()`（约 237-249 行）的直接构造中，`capabilities: Vec::new(),` 之后追加：

```rust
                app_version: user.app_version,
```

- [ ] **Step 4: 编译检查**

Run: `cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib`
Expected: 编译通过，无错误。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/workspace.rs
git commit -m "feat(workspace): WorkspaceDevice 新增 app_version"
```

---

### Task 5: 发现事件推送 `app_version`

**Files:**
- Modify: `src-tauri/src/network/discovery.rs`

本任务把 `app_version` 加入 `emit("new-peer", ...)` 事件 JSON，使前端增量推送也携带版本。无单元测试，靠编译检查验证。

- [ ] **Step 1: 修改事件 JSON**

在 `discovery.rs` 第一处 `emit("new-peer", ...)`（约 601-614 行）的 JSON 中，`"protocol_version": announcement.protocol_version` 之后追加：

```rust
                            "app_version": announcement.app_version,
```

- [ ] **Step 2: 编译检查**

Run: `cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib`
Expected: 编译通过，无错误。

- [ ] **Step 3: 提交**

```bash
git add src-tauri/src/network/discovery.rs
git commit -m "feat(network): 发现事件推送 app_version"
```

---

### Task 6: 前端展示版本并更新 i18n

**Files:**
- Modify: `frontend/src/App.jsx`

本任务替换设备身份面板的「可用内存」为「版本」，删除 `availableMemory` 文案，复用已有 `version` 文案。

- [ ] **Step 1: 替换展示行**

将设备身份面板（约 2864-2871 行）的「可用内存」`div` 替换为：

```jsx
            <div>
              <dt>{labels.version}</dt>
              <dd>{device.app_version || labels.notProvided}</dd>
            </div>
```

- [ ] **Step 2: 删除中文 `availableMemory` 文案**

删除中文 i18n 对象中的 `availableMemory: "可用内存",`（约 172 行）。

- [ ] **Step 3: 删除英文 `availableMemory` 文案**

删除英文 i18n 对象中的 `availableMemory: "Available memory",`（约 422 行）。

- [ ] **Step 4: 构建前端**

Run: `npm run build`
Expected: 构建成功，无报错。

- [ ] **Step 5: 提交**

```bash
git add frontend/src/App.jsx src/
git commit -m "feat(ui): 设备身份面板显示版本"
```

---

## 完成标准

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib` 全部通过。
- [ ] `cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib` 编译通过。
- [ ] `npm run build` 成功。
- [ ] 设备身份面板显示对方版本号；旧设备（不发送版本）显示「未提供」。
- [ ] 六个 task 各自独立提交。
