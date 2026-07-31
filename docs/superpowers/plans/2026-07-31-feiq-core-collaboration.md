# 飞秋核心协作与共享文件实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 交付飞秋式群协作和共享文件闭环，同时复用 XChat 已有消息回执、离线补发、断点续传、并行分块和文件中心。

**Architecture:** 控制消息继续走 `src-tauri/src/network/messaging.rs` 的现有 WebSocket/TCP 控制面；群公告和共享文件元数据进入 SQLite，并由 `workspace.rs` 统一聚合到 snapshot。文件内容不新建传输引擎，直接调用 `conversation_file.rs` 的受管分块核心；React 只通过 `XChatModule` dispatch/subscribe 更新 UI。

**Tech Stack:** React 19.2.8、Node test、Rust/Tokio、Axum、SQLite/sqlx、现有 conversation-file transfer core。

## Global Constraints

- 不实现 IPMSG/2425 兼容层。
- 共享文件路径必须 canonical 到应用管理的共享根目录；HTTP/Tauri 操作只接受稳定共享文件 ID。
- 群公告、共享文件、下载授权和撤销动作必须幂等；迟到事件不能恢复已撤销或已完成状态。
- 保持 `ui-ref/DESIGN.md` 的四栏布局、文件阶段/进度/速度/动作表达和浅深色层级。

### Task 1: 固定核心事件、能力和错误码

**Files:**
- Modify: `src-tauri/src/network/protocol.rs`
- Modify: `src-tauri/src/network/messaging.rs`
- Modify: `src-tauri/src/workspace.rs`
- Modify: `frontend/src/xchat.js`
- Test: `src-tauri/src/network/protocol.rs` tests, `frontend/src/xchat.test.js`

**Interfaces:**
- Produces `group.announcement.v1`, `shared_file.offer.v1`, `shared_file.request.v1`, `shared_file.revoked.v1` payload names.
- Produces capability keys `group.announcement`, `shared_files`, `shared_file_passwords`.
- Produces error codes `shared_file_not_found`, `shared_file_revoked`, `shared_file_password_invalid`, `shared_file_path_forbidden`, `group_announcement_forbidden`.

- [ ] 写 serde 兼容测试：旧 Text/File/receipt payload 忽略未知事件字段，新事件缺少可选字段时使用默认值。
- [ ] 写前端能力归一化测试：snake_case/camelCase 两种字段都映射为 `sharedFiles`、`groupAnnouncement`。
- [ ] 在协议模块增加版本字段和事件类型常量，不修改旧消息序列化字段。
- [ ] 在 workspace snapshot 输出第一期能力位；legacy adapter 继续把能力标记为 false。
- [ ] 运行 `rtk cargo test --manifest-path src-tauri/Cargo.toml --lib network::protocol` 与 `rtk node --test frontend/src/xchat.test.js`。
- [ ] 提交 `feat: define collaboration capability contracts`。

### Task 2: 群公告与群文件来源

**Files:**
- Modify: `src-tauri/src/db.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/web_server.rs`
- Modify: `src-tauri/src/workspace.rs`
- Modify: `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`
- Modify: `src-tauri/permissions/commands.toml` and relevant capability JSON
- Modify: `frontend/src/xchat.js`, `frontend/src/App.jsx`, `frontend/src/styles.css`
- Test: Rust DB tests, `frontend/src/xchat.test.js`

**Interfaces:**
- Rust: `set_group_announcement(pool, conversation_id, author_id, content) -> GroupAnnouncement`.
- Rust: `get_group_details(pool, conversation_id) -> GroupDetails`.
- Adapter actions: `conversation.setAnnouncement`, `conversation.openDetails`.
- HTTP: `POST /api/conversations/:id/announcement`, `GET /api/conversations/:id/details`.
- Tauri: `set_group_announcement`, `get_group_details`.

- [ ] 先写数据库迁移测试：新表 `group_announcements` 只允许 group conversation，重复写入使用稳定 announcement ID 更新而不是重复插入。
- [ ] 实现 `group_announcements` 表和查询/写入函数；公告内容限制为 4 KiB，空白内容拒绝。
- [ ] 在群同步帧中携带最新公告版本与摘要；收到旧版本时不覆盖本地新公告。
- [ ] 接入 Tauri/Web 薄入口和双注册；把错误转换为上面的固定错误码。
- [ ] 在 `App.jsx` 群详情面板增加公告区、成员在线状态和“群文件”来源入口；小于 1000px 通过模态承载详情。
- [ ] 运行 `rtk cargo test --manifest-path src-tauri/Cargo.toml --lib db::tests`、`rtk node --test frontend/src/xchat.test.js` 和 `rtk npm run build`。
- [ ] 提交 `feat: add group announcements and details`。

### Task 3: `shared_files` 受管共享目录模型

**Files:**
- Create: `src-tauri/src/shared_files.rs`
- Modify: `src-tauri/src/db.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/shared_files.rs` tests and DB migration tests

**Interfaces:**
- `SharedFileRecord { id, owner_id, conversation_id, root_kind, relative_path, name, size, sha256, mime_type, password_required, password_hint, download_count, status, created_at, updated_at }`.
- `create_shared_file(pool, owner_id, conversation_id, path, password, hint) -> SharedFileRecord`.
- `list_shared_files(pool, scope) -> Vec<SharedFileRecord>`.
- `revoke_shared_file(pool, shared_file_id) -> SharedFileRecord`.
- `resolve_shared_file_path(pool, shared_file_id) -> Result<PathBuf, SharedFileError>`.

- [ ] 写失败测试：路径越界、目录而非文件、文件已删除、空密码提示、重复共享同一 canonical path。
- [ ] 添加 `shared_files` 表和 `shared_file_downloads` 统计表；密码使用现有 `sha2` 计算带随机 salt 的摘要，禁止明文保存。
- [ ] 实现 canonical 根目录校验、SHA-256 计算和稳定 ID；共享撤销只改状态，不删除用户文件。
- [ ] 为已接收/已发送路径区分 `root_kind`，禁止通过共享 ID 读取 outgoing 临时目录之外的任意路径。
- [ ] 运行 `rtk cargo test --manifest-path src-tauri/Cargo.toml --lib shared_files` 和 `rtk cargo test --manifest-path src-tauri/Cargo.toml --lib db::tests`。
- [ ] 提交 `feat: add managed shared file records`。

### Task 4: 共享文件请求与传输复用

**Files:**
- Modify: `src-tauri/src/network/conversation_file.rs`
- Modify: `src-tauri/src/network/messaging.rs`
- Modify: `src-tauri/src/shared_files.rs`
- Modify: `src-tauri/src/commands.rs`, `src-tauri/src/web_server.rs`
- Modify: `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`
- Modify: `src-tauri/permissions/commands.toml` and relevant capability JSON
- Modify: `frontend/src/xchat.js`
- Test: Rust transfer tests, `frontend/src/xchat.test.js`

**Interfaces:**
- Rust: `request_shared_file(shared_file_id, requester_id, password) -> TransferRecord`.
- Rust: `record_shared_file_download(shared_file_id, requester_id) -> SharedFileRecord`.
- Adapter actions: `sharedFile.request`, `sharedFile.revoke`, `sharedFile.refresh`.
- HTTP: `GET /api/shared-files`, `POST /api/shared-files/:id/request`, `POST /api/shared-files/:id/revoke`.
- Tauri commands with the same names.

- [ ] 写请求状态测试：密码错误不创建 transfer；撤销/不存在共享文件返回固定错误；重复请求复用 active transfer。
- [ ] 把共享文件请求转换为现有 `conversation_file` job，复用 4 MiB 分块、v2 四分块并发、断点续传、取消和远端终态清理。
- [ ] 在请求成功后只通过稳定 shared file ID 解析路径，并在完成后原子增加下载次数。
- [ ] 为请求/取消/重试广播 `shared_file.*` 事件，旧客户端忽略未知事件。
- [ ] 运行双实例 HTTP 测试：共享 → 请求 → 100% 接收 → SHA-256 一致 → 下载次数为 1；再覆盖取消、重试和撤销。
- [ ] 提交 `feat: reuse transfer core for shared file downloads`。

### Task 5: 文件中心与群文件 UI

**Files:**
- Modify: `frontend/src/App.jsx`
- Modify: `frontend/src/xchat.js`
- Modify: `frontend/src/styles.css`
- Modify: `src-tauri/src/workspace.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/web_server.rs`
- Modify: `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`
- Modify: `src-tauri/permissions/commands.toml` and relevant capability JSON
- Test: `frontend/src/xchat.test.js`, `frontend/src/styles.test.js`

**Interfaces:**
- Snapshot adds `sharedFiles` and `sharedFileSources`.
- Actions: `sharedFile.openSource`, `sharedFile.request`, `sharedFile.revoke`, `sharedFile.deleteLocalCopy`.

- [ ] 写 reducer/normalizer 测试：共享文件状态映射为 `available`、`password_required`、`requesting`、`transferring`、`revoked`、`expired`。
- [ ] 文件中心左侧来源增加“我的共享文件”、好友和群共享来源；保留现有类型筛选和来源搜索。
- [ ] 共享文件行同时显示名称、密码状态、下载次数、大小、来源、时间和请求/撤销动作。
- [ ] 在聊天文件卡中显示“共享已撤销/需要密码/请求中”等阶段；删除仍只删除本机副本。
- [ ] 按 `ui-ref/DESIGN.md` 做 1100/1000/860px 响应式检查，不新增第五栏。
- [ ] 运行 `rtk npm test && rtk npm run build`。
- [ ] 提交 `feat: expose shared files in file center`。

### Task 6: 核心协作验收

**Files:**
- Inspect: `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`, `src-tauri/permissions/commands.toml`, relevant capabilities
- Inspect: `ui-ref/DESIGN.md`, `ui-ref/ui_kits/app/chat-workspace.html`, `ui-ref/ui_kits/app/file-center.html`

- [ ] 运行 `rtk cargo test --manifest-path src-tauri/Cargo.toml --lib`。
- [ ] 运行 desktop library/bin 与 web bin 编译检查。
- [ ] 用两个隔离 Web 实例验证群公告、群文件、共享文件请求、密码错误、取消/重试和撤销。
- [ ] 在 360×800、820×1180、1024×768、1366×768、1440×900 下检查无横向溢出和可恢复错误文案。
- [ ] 执行 `rtk git diff --check`，确认只包含本子计划范围的文件。
- [ ] 提交 `test: verify FeiQ collaboration milestone`。
