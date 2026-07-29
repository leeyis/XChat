# XChat React 全量重构设计

- 日期：2026-07-29
- 状态：规格已确认，按方案 A 实施中
- 视觉来源：`ui-ref/DESIGN.md`、`ui-ref/SKILL.md`、`ui-ref/xchat-desktop-prototype.html`
- 目标运行时：Tauri 2 桌面应用与 `lanchat-web` Web 应用

## 1. 目标

彻底放弃 `src/` 下旧的手写 HTML/CSS/JavaScript 界面，使用 React 重建最新桌面设计。聊天、主机、文件和设置四个一级模块必须连接真实状态；现有 Rust 能力继续复用，缺失的群聊、已读回执、传输取消、文件中心、设备元数据和截图发送在共享 Rust 核心中实现，不以 mock 数据或无效按钮代替。

完成后的桌面和 Web 前端使用同一套 React 代码，并分别通过 Tauri invoke/event 与 HTTP/WebSocket adapter 访问相同业务能力。

## 2. 本轮范围

### 2.1 聊天

- 单聊和群聊会话列表。
- 创建群聊；一个群至少包含当前设备和两台远端设备。
- 打开会话、分页加载历史、本地搜索消息。
- 发送文本、文件和截图。
- 离线排队、重试、发送中、已发送、已送达、已读和失败状态。
- 直接会话显示对方已读；群聊显示已读人数。
- 会话置顶、手动标记未读和草稿，均为本机状态。
- 删除单条本地消息、清空本地会话历史和删除联系人。
- 群聊详情展示标题和成员；创建后的成员增删与管理员权限不在本轮。

搜索针对完整本地 SQLite 历史，而不是只搜索当前已加载气泡。后端对文本内容和文件名执行大小写不敏感查询，单次最多返回最近的 100 条；点击结果打开对应会话并加载目标消息附近的历史。

### 2.2 主机

- 展示自动发现和手工添加的设备。
- 展示设备名、用户备注、hostname、IP/端口、可选 MAC、发现来源、在线状态、最后在线时间和可用内存。
- 编辑本机名称和头像。
- 编辑对端备注，新增/删除手工 endpoint，删除本地联系人。
- 从主机页直接打开单聊。
- 现有 UUID 继续作为唯一身份；MAC 只是可能变化或缺失的辅助属性。

### 2.3 文件

- 聚合所有带附件的消息和独立传输记录。
- 按全部、发送、接收、进行中、失败筛选。
- 展示文件名、类型、来源/目标、大小、进度、速度、状态、失败原因和时间。
- 支持接收、重试、取消、打开文件、打开目录和删除下载副本。
- 删除动作只接受文件或消息 ID；后端解析可信路径。
- 删除接收文件的本地副本后保留消息记录；不会删除发送方原始文件。

### 2.4 设置

- 本机名称与头像；头像复制到应用数据目录并仅在本机界面使用，本轮不通过局域网传播头像二进制。
- 浅色、深色、跟随系统主题。
- 中文和英文界面。
- 通知开关。
- 下载目录和自动下载。
- 服务端口、数据库目录和手工设备地址。
- 截图快捷键；仅在运行时声明支持截图的平台启用，macOS 默认为 `Command+Shift+A`，Web 默认为 `Ctrl/Command+Shift+A`。
- 对需要重启才能生效的设置给出明确提示。

### 2.5 明确不做

- AI 诊断或任何 AI 入口。
- 云账号、中心服务器、跨公网中继。
- 端到端加密、设备证书或权限角色系统。
- 群管理员、入群审批、创建后增删成员。
- 消息编辑、表情回应、语音或视频通话。
- 为未来扩展预建事件存储、代码生成器、Redux 或通用 repository 层。

## 3. 方案选择

采用一个深 `XChatModule` 作为 React 与运行时之间的唯一外部 seam：

```ts
interface XChatModule {
  getSnapshot(): Readonly<XChatSnapshot>;
  dispatch(action: XChatAction): Promise<Outcome>;
  subscribe(notify: () => void): () => void;
}
```

React 使用 `useSyncExternalStore` 读取快照。聊天、主机、文件和设置页面不直接调用 Tauri、`fetch` 或 WebSocket。

`XChatModule` 的 implementation 内部有两个真实 adapter：

- `TauriAdapter`：映射现有及新增的 Tauri commands 和 events。
- `HttpWsAdapter`：映射 Axum HTTP routes 和 `/ws` 事件。

这是一个真实 seam，因为桌面和 Web 两种运行方式都存在。不会公开 Chat、Host、File、Settings 四套浅 interface，也不会增加仅有测试用途的生产 mock adapter。

## 4. 运行结构

```text
React 页面与视图
        │
        ▼
XChatModule
getSnapshot / dispatch / subscribe
        │
        ├─────────────┐
        ▼             ▼
 TauriAdapter    HttpWsAdapter
 invoke/events   HTTP/WebSocket
        │             │
        └──────┬──────┘
               ▼
     现有 Rust DB/network/file 核心
       + 本轮共享业务扩展
               │
               ▼
       SQLite / UDP / peer HTTP
```

commands 和 HTTP handlers 只负责输入转换、权限检查与结果序列化。群扇出、逐消息送达/已读回执、传输状态和可信文件解析放在共享 Rust implementation 中，保证桌面与 Web 行为一致。

## 5. React 工程和构建

- 仓库根新增 `package.json`、锁文件和 Vite 配置。
- React 源码位于 `frontend/`。
- Vite 的生产输出目录为现有 `src/`。
- `src/` 从手写前端变为可嵌入的构建产物，并保留在版本控制中，使直接执行 Rust 编译时仍存在 `RustEmbed` 目录。
- Tauri 开发模式连接 Vite dev server；生产仍加载 `src/`。
- `lanchat-web` 继续从 `src/` 嵌入相同生产资源。
- `src/css/vscode.css` 作为旧 Rust 自定义主题命令的兼容资源保留，但新 React 界面不依赖旧主题结构。
- 只增加 React、React DOM 和 Vite 所需依赖；状态、路由、图标和测试优先使用 React、CSS、SVG 与 Node 标准能力。

## 6. 前端状态 interface

`XChatSnapshot` 至少包含：

```ts
type XChatSnapshot = {
  phase: "booting" | "ready" | "offline" | "error";
  self: LocalProfile;
  activeSection: "chat" | "hosts" | "files" | "settings";
  activeConversationId: string | null;
  conversations: ConversationSummary[];
  messagesByConversation: Record<string, MessageView[]>;
  devices: DeviceView[];
  files: FileView[];
  transfers: TransferView[];
  searchResults: MessageSearchResult[];
  settings: SettingsView;
  capabilities: Capabilities;
  notices: Notice[];
};
```

状态未变化时 `getSnapshot()` 保持引用稳定。`dispatch()` 成功表示动作已被本机持久接受，不表示远端传输已经完成；后续状态通过订阅进入快照。

主要动作：

- `bootstrap`
- `navigation.open`
- `conversation.open`
- `conversation.createGroup`
- `conversation.pin`
- `conversation.markUnread`
- `conversation.saveDraft`
- `conversation.loadOlder`
- `message.sendText`
- `message.sendFiles`
- `message.sendCapture`
- `message.search`
- `message.markRead`
- `message.deleteLocal`
- `message.clearConversation`
- `device.saveRemark`
- `device.saveEndpoint`
- `device.removeEndpoint`
- `device.remove`
- `file.accept`
- `file.retry`
- `file.open`
- `file.reveal`
- `file.deleteLocalCopy`
- `transfer.cancel`
- `settings.patch`

动作采用可辨识联合类型集中定义。可恢复错误返回带 `code`、`message` 和 `retryable` 的 `Outcome`；程序错误才抛异常。

## 7. 事件归一化

Tauri events 和 WebSocket 消息在 adapter 内归一化为：

- `device.changed`
- `conversation.changed`
- `message.changed`
- `receipt.changed`
- `transfer.changed`
- `settings.changed`

adapter 负责重连、去重、snake_case/camelCase 转换和旧事件名兼容。React 页面不自行监听 WebSocket，也不自行合并 Tauri events。

消息发送时前端生成 `clientMessageId` 并立即显示 pending 消息；重试沿用同一 ID。后端以该 ID 幂等保存，收到确认后更新原记录，不创建重复气泡。

## 8. 视觉和交互

### 8.1 桌面结构

- 主导航宽 `56px`。
- 列表栏宽 `280px`。
- 主内容区域自适应。
- 详情栏宽 `240px`。
- 主导航选中项只有图标和细指示线使用 `#18ac71`。
- 当前会话整行使用绿色选中态。
- 浅色和深色保持相同层级与空间关系。

### 8.2 聊天

- 列表顶部包含标题、搜索和建群动作。
- 消息区支持日期分隔、发送者头像、文本、图片/文件卡片和状态。
- 编辑器是完整描边容器，高度在 `66–200px` 自动增长。
- Enter 发送，Shift+Enter 换行；输入法组合期间 Enter 不发送。
- 文件选择或截图开始时冻结目标会话，避免用户切换会话后发错对象。
- 详情栏显示直接设备信息或群成员和文件摘要。

### 8.3 主机

- 自动发现与手工设备使用相同卡片层级，发现来源用标签区分。
- 设备离线时保留最后地址与最后在线时间。
- MAC 不可用时显示“设备 ID”，不伪造 MAC。

### 8.4 文件

- 文件表格或列表始终同时展示阶段、数据量、速度或原因，以及当前可执行动作。
- 进行中状态展示进度条；失败状态展示可读原因和重试。
- 取消是明确按钮，并在请求后显示“正在取消”，直到最终事件到达。

### 8.5 设置

- 按身份、外观、通知、文件、网络和快捷键分组。
- 保存后立即可生效的字段即时更新。
- 端口和数据库目录等需要重启的字段显示“重启后生效”。

### 8.6 响应式与可访问性

- `<1100px` 隐藏次要文件列。
- `<1000px` 隐藏详情栏。
- `<860px` 隐藏列表栏，通过返回动作切换列表与内容。
- 点击区域不小于 `44px`。
- 所有图标按钮提供可访问名称和键盘焦点态。
- 尊重 `prefers-reduced-motion`。
- 颜色不是状态的唯一表达手段。

## 9. SQLite 兼容迁移

迁移采用幂等建表和添加可空字段，不删除或重写现有行。

### 9.1 conversations

- `id TEXT PRIMARY KEY`
- `kind TEXT NOT NULL`：`direct` 或 `group`
- `peer_id TEXT NULL`：直接会话的对端 UUID
- `title TEXT NULL`
- `created_by TEXT NULL`
- `pinned INTEGER NOT NULL DEFAULT 0`
- `forced_unread INTEGER NOT NULL DEFAULT 0`
- `draft TEXT NOT NULL DEFAULT ''`
- `created_at INTEGER NOT NULL`
- `updated_at INTEGER NOT NULL`

旧直接消息首次查询时按对端 UUID 懒创建直接会话，不批量复制消息。

直接会话 ID 使用两个设备 UUID 排序后得到的稳定值：

```text
direct:<较小 UUID>:<较大 UUID>
```

因此双方对同一直接会话使用相同 ID，不依赖本地数据库行号。群聊 ID 是创建端生成的 UUID。

### 9.2 conversation_members

- `conversation_id TEXT NOT NULL`
- `peer_id TEXT NOT NULL`
- `display_name TEXT NOT NULL`
- `role TEXT NOT NULL`
- `joined_at INTEGER NOT NULL`
- 复合主键：`conversation_id, peer_id`

### 9.3 message_receipts

- `message_client_id TEXT NOT NULL`
- `reader_id TEXT NOT NULL`
- `delivered_at INTEGER NULL`
- `read_at INTEGER NULL`
- `updated_at INTEGER NOT NULL`
- 复合主键：`message_client_id, reader_id`

送达和已读按稳定消息 ID 记录，不使用设备时间推导顺序，也不假定不同设备看到的群消息顺序完全相同。重复 ack 通过复合主键幂等合并；`read_at` 一旦存在就不会被清空。

### 9.4 messages 扩展

- `conversation_id TEXT NULL`
- `client_message_id TEXT NULL`
- 对非空 `client_message_id` 建唯一索引。

旧消息字段和序列化保持可读；缺少稳定 ID 的历史消息不推导远端已读状态。

### 9.5 transfers

- `id TEXT PRIMARY KEY`
- `message_id INTEGER NULL`
- `conversation_id TEXT NOT NULL`
- `peer_id TEXT NOT NULL`
- `direction TEXT NOT NULL`
- `status TEXT NOT NULL`
- `bytes_total INTEGER NOT NULL`
- `bytes_transferred INTEGER NOT NULL`
- `error TEXT NULL`
- `created_at INTEGER NOT NULL`
- `updated_at INTEGER NOT NULL`

群文件在聊天中是一条逻辑消息，在 `transfers` 中为每个成员保存一条子传输。这里的“独立传输记录”是指生命周期与消息行分表保存；所有用户可见传输仍属于一个直接或群会话，因此 `conversation_id` 保持非空。旧文件消息首次查询时映射到对应的稳定直接会话。

### 9.6 users 扩展

- `hostname TEXT NULL`
- `mac_address TEXT NULL`
- `remark TEXT NULL`
- `discovery_source TEXT NULL`

remark 只在本机保存，不通过局域网广播。

## 10. 局域网协议

### 10.1 兼容原则

- 旧单聊和文件 payload 保持可解析。
- 新字段全部可选。
- UDP discovery 保留旧字段前缀，并允许追加协议版本、hostname、MAC 和 capabilities。
- 接收端同时接受旧 discovery 和扩展 discovery。
- 未声明群聊能力的旧客户端不会被加入新群。

### 10.2 新消息类型

- `group_sync`：群 ID、标题、创建者、成员快照和版本。
- `group_message`：群 ID、稳定消息 ID、发送者、正文或附件元数据、时间戳。
- `delivery_ack`：会话 ID、接收者和已落库的稳定消息 ID 列表。
- `read_ack`：会话 ID、读取者和已实际展示的稳定消息 ID 列表。

发送群消息时，Rust 根据成员表向除本机外的成员扇出。在线成员立即发送；离线成员进入现有持久重试路径。前端永远只发一次 `conversationId + body`。

收到重复 `clientMessageId` 时更新现有状态，不插入第二条消息，并再次发送对应 ack，使丢失的确认能够恢复。

精确状态语义：

- `pending`：前端已创建乐观消息，后端尚未持久接受。
- `sent`：本机消息已持久化，并已进入即时发送或离线队列。
- `delivered`：目标设备已将消息持久化并返回 `delivery_ack`。
- `read`：目标设备已在可见活动会话中渲染消息并返回 `read_ack`。

直接会话只有一个目标。群聊对每个远端成员分别保存 delivery/read 行，界面显示 `已送达 n/m` 与 `已读 n/m`。接收端落库后立即发送 delivery ack；read ack 只发送给相应消息作者。无法立即发送的 ack 使用现有持久重试路径，重连后再次发送。

### 10.3 已读

- 只有消息已成功渲染、应用窗口可见且会话处于活动状态时，才为尚未确认的可见消息发送 read ack。
- 打开预加载数据不会标记已读。
- ack 以稳定消息 ID 的有界列表批量发送，不依赖发送者时钟、接收者时钟或本地自增 ID。
- ack 状态持久保存；作者恢复在线后补发未确认 ack。
- 直接聊天显示 `pending/sent/delivered/read`。
- 群聊按每条消息的成员 receipt 行推导“已读 n/m”。

## 11. 文件传输与取消

Transfer 状态：

```text
queued → waiting_peer → offering → awaiting_acceptance
       → transferring → completed
       → cancelling → cancelled
       → failed
```

- 排队状态取消：数据库直接改为 `cancelled`，不启动任务。
- 活动状态取消：共享传输注册表保存 `transferId -> Arc<AtomicBool>`；上传和下载循环在每个分块之间检查。
- 接收端写入 `.part` 文件；取消或失败后清理临时文件。
- 完成与取消竞争时以第一个不可逆状态为准，返回 `completed` 或 `cancel_too_late`，不谎报取消成功。
- 重试复用逻辑消息，但创建新的 transfer ID。
- 文件中心查询通过数据库公开，不从 DOM 或消息列表临时推导。

## 12. 截图

截图最终生成普通 PNG 附件，后续沿用文件传输状态机，不增加独立消息类型。

### 12.1 Tauri macOS

- Rust 使用系统 `screencapture` 的交互式选择能力。
- 输出路径由后端在应用缓存目录创建，用户参数不能影响命令或路径。
- 用户按 Escape 视为取消，不显示错误。
- 成功后生成临时 asset，发送结束或取消后清理。

### 12.2 Web

- 在用户点击或快捷键产生的用户手势中调用 `navigator.mediaDevices.getDisplayMedia`。
- 用户通过浏览器系统面板选择屏幕或窗口。
- 从视频流捕获一帧为 PNG 后立即停止所有 track。
- 权限拒绝视为取消；真正的设备或编码错误显示可恢复错误。

### 12.3 其他平台

- Windows、Linux 或 Android 只有在运行时能力检测成功时才显示截图动作。
- 不可用时 `capabilities.capture` 为 `false`，不会注入 mock 图片。
- 快捷键仅在 XChat 窗口获得焦点时生效；本轮不注册系统级全局快捷键。
- 设置中只在 `capabilities.capture` 为 `true` 时允许编辑和启用截图快捷键。

浏览器不支持“在系统文件管理器中显示文件”。此时 `capabilities.revealFile` 为 `false`，文件中心保留下载/打开动作并隐藏“打开目录”，不会显示点击后无效的按钮。Web 通知同样使用浏览器 Notification capability；权限或运行时不支持时明确禁用。

### 12.4 平台能力矩阵

| 运行时 | 截图 | 应用内截图快捷键 | 打开所在目录 | 通知 |
|--------|------|------------------|--------------|------|
| Tauri macOS | 系统 `screencapture` | 支持 | 支持 | 当前实现不声明支持，设置禁用 |
| Tauri Windows | 本轮不支持 | 禁用 | 支持 | 复用现有 PowerShell 通知 |
| Tauri Linux | 本轮不支持 | 禁用 | 支持 | 仅检测到 `notify-send` 时支持 |
| Tauri Android | 不支持 | 禁用 | 不支持 | 复用现有 notification plugin |
| Web | `getDisplayMedia` 存在时支持 | 截图能力存在时支持 | 不支持 | Notification API 存在且获授权时支持 |

能力由启动响应和浏览器运行时检测合并产生。界面只呈现可执行动作；设置项若需要解释平台限制则显示为禁用并附原因。

## 12.5 头像持久化

- 用户选择头像后，后端将文件复制到应用数据目录中的受管 profile 路径，并在现有 settings KV 中保存相对引用。
- 替换或移除头像只清理受管副本，不删除用户选择的原文件。
- 本轮头像仅用于当前设备 UI；discovery、handshake 和消息协议不发送头像二进制。
- 对端设备继续使用名称首字母生成的确定性占位头像。

## 13. 文件访问安全

- HTTP 和 Tauri 删除、预览、打开、定位接口都接受数据库 ID，不接受任意文件路径。
- 后端查询数据库后对路径进行规范化。
- 接收文件必须位于配置下载目录或应用 staging/cache 目录。
- 发送方原始文件可以打开但不能由“删除下载副本”动作删除。
- 媒体响应只服务已知文件记录，并正确设置类型；不存在、未完成或越界记录返回明确错误。
- 用户可见错误不暴露数据库路径、堆栈或局域网内部实现细节。

## 14. Tauri 与 Web 接入

新增或改名的 Tauri command 必须同步：

- `src-tauri/src/main.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/permissions/commands.toml`
- 相关 capability JSON

Axum 增加与之等价的 HTTP routes，并通过现有 WebSocket 推送归一化事件。两个 adapter 返回相同的领域字段和错误码。

现有旧 commands/routes 保留为兼容入口，直到新 React 完全不再调用后再判断是否删除；本轮不做无关后端清理。

## 15. 错误处理

- 启动时各数据域独立加载；某个非关键域失败不会让整个应用白屏。
- transport 断开时快照进入 `offline`，保留最后状态和可离线排队动作。
- 表单校验错误在字段附近显示。
- 后端拒绝、对端不支持、源文件缺失、权限拒绝和网络中断使用不同错误码。
- 重试只对 `retryable` 错误开放。
- 所有破坏性动作需要明确目标；清空历史和删除下载副本需要确认。

## 16. 测试策略

### 16.1 前端

- 使用 Node 内置测试运行器验证纯状态归一化逻辑：事件去重、pending 确认、已读单调推进、取消竞态和断线重连后的快照。
- `npm run build` 必须成功，并确认 `src/index.html` 和构建资源存在。
- 在浏览器中覆盖四个一级模块、空状态、加载状态、错误状态、浅色/深色和三个响应式断点。
- 键盘验证 Enter、Shift+Enter、焦点顺序、Escape 和截图快捷键。

### 16.2 Rust

- 迁移测试：旧数据库可以无损打开，重复初始化不会失败。
- 群聊测试：群成员扇出、离线排队、重复稳定 ID 幂等。
- 回执测试：delivery/read ack 幂等，重复消息会重发 ack，旧消息不伪造送达或已读。
- 传输测试：排队取消、活动取消、完成竞争和 `.part` 清理。
- 文件安全测试：任意路径和越界记录不能删除或读取文件。

### 16.3 编译与冒烟

```bash
rtk npm run build
rtk npm test
rtk cargo test --manifest-path src-tauri/Cargo.toml --lib
rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib
rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features web --bin lanchat-web
rtk cargo tauri dev -- -- --port 18888 --db-path /tmp/lanchat-agent
```

冒烟使用非默认端口和临时数据库目录，不污染真实应用数据。

## 17. 验收标准

1. 运行界面不再包含旧 HTML 布局或 AI 入口。
2. 四个一级模块与最新原型的层级、配色、间距、选择态和响应式行为一致。
3. Tauri 和 Web 均能加载同一 React 构建。
4. 单聊、真实群聊、历史分页、离线发送和本地搜索可运行。
5. 已读状态来自真实对端回执，不能用计时器模拟。
6. 文件中心来自 SQLite，发送、接收、重试和取消具有真实状态。
7. 主机页展示真实 UUID、地址和在线状态；hostname/MAC 缺失时明确降级。
8. macOS 和受支持 Web 浏览器可以截图并作为普通附件发送。
9. 所有设置持久化；需要重启的字段明确提示。
10. 旧 SQLite 数据和旧直接消息协议仍可读取。
11. 用户现有的 `src-tauri/src/commands.rs` 修改和未跟踪设计文件不被覆盖。
12. 前端构建、Rust 测试、desktop/web 编译和隔离冒烟结果全部记录。

## 18. 实施里程碑

全部里程碑都属于本轮，不把批准范围推迟到以后。每个里程碑独立构建和验证，失败时不会让后续工作掩盖根因。

### M1：React 运行链路与现有能力回接

- 建立 React/Vite 构建、`XChatModule` 和两个 adapter。
- 完成最新设计的四模块静态结构、主题和响应式布局。
- 回接现有单聊、主机发现、历史、基本文件收发和设置。
- 出口条件：前端构建成功，Tauri/Web 都能加载 React，旧 UI 不再出现。

### M2：会话、群聊和真实回执

- 添加 conversations、members、stable IDs 和 receipts 迁移。
- 实现稳定 direct ID、建群、群同步、群消息扇出、delivery ack 和 read ack。
- 接入离线重试、幂等、搜索、置顶、未读和草稿。
- 出口条件：Rust 协议测试通过，两实例可完成群发并看到真实送达/已读人数。

### M3：文件中心与传输状态机

- 添加 transfer 记录和完整文件中心查询。
- 实现群文件子传输、接收、重试、取消、竞态处理和临时文件清理。
- 增加基于数据库 ID 的预览、打开、定位和删除安全检查。
- 出口条件：取消及路径安全测试通过，前端文件中心能反映真实进度和终态。

### M4：设备元数据、设置与截图

- 扩展 discovery 能力、hostname/MAC/remark 和本机受管头像。
- 实现 macOS 系统截图、Web `getDisplayMedia`、运行时 capabilities 和应用内快捷键。
- 补齐主题、语言、通知、下载和网络设置状态。
- 出口条件：平台矩阵行为与界面动作一致，不支持的平台没有无效按钮。

### M5：整体验收

- 完成空、加载、离线、错误、浅色、深色及响应式状态。
- 运行 Node 检查、Rust 测试、desktop/web 编译和隔离数据冒烟。
- 对照最新原型逐模块完成视觉与键盘验证。
- 出口条件：第 17 节十二项验收标准全部有可复查结果。
