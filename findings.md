# 发现与决策

## 需求
- 旧 HTML 界面完全弃用，以 `ui-ref/xchat-desktop-prototype.html`、`ui-ref/DESIGN.md` 和对应素材为准。
- React 实现聊天、主机、文件、设置四个模块，不实现 AI 诊断。
- 现有 Rust 后端能复用则复用；群聊、已读回执、传输取消、截图快捷键等缺失能力本轮真实实现。
- 完成后提供运行、预览和验证命令。
- 0.1.0 追加范围：品牌与图标、空状态、设置导航、Dock 重开、表情、截图编辑、附件草稿、图片内联和严格原型文件中心。
- 截图首版必须支持矩形、椭圆、箭头、画笔、马赛克、文本和回退。

## 研究发现
- 当前前端是 `src/` 下无依赖的 HTML/CSS/JavaScript；桌面由 Tauri 加载，Web 由 `RustEmbed` 嵌入同一目录。
- `src/js/api.js` 已包含 Tauri invoke 与 HTTP 两条路径；Web 实时事件在旧 `app.js` 中自行维护 WebSocket。
- 后端已有设备发现、单聊、历史、离线重发、文件发送/接收/重试、下载设置和部分平台能力。
- 缺失能力包括真实群模型和协议、跨设备送达/已读回执、可枚举及可取消的传输、文件中心公开查询、设备备注/hostname/MAC，以及统一截图入口。
- 最新视觉为绿色强调色和冷中性色，桌面主结构为 `56 / 280 / flexible / 240`；旧 `plan/UI-DESIGN-PC.md` 是历史方案，不是本轮视觉来源。
- Git 工作区已有用户修改：`src-tauri/src/commands.rs`，以及未跟踪的 `.happycode/`、`AGENTS.md`、`plan/UI-DESIGN-PC.md`、`ui-ref/`。
- Tauri `main.rs` 与移动端 `lib.rs` 各自维护完整 command 注册列表；新增 command 必须同时接入。
- Axum `start_server` 在单个 `web_server.rs` 中注册静态资源、HTTP API 和 `/ws`；当前 `send_message` 的 Tauri/Web implementation 重复在线检查与数据库保存。
- `send_text_message` 当前直接序列化旧 `TextMessage`，WebSocket 失败后回落 TCP；新协议应复用其底层 JSON 发送，不改变旧调用者。
- 当前通知 implementation：Windows 使用 PowerShell、Linux 使用 `notify-send`、Android 使用 notification plugin；用户工作区已移除 macOS plugin 分支，因此能力矩阵把 macOS 通知设为不可用。
- 2026-07-29 官方稳定版本：React/React DOM `19.2.8`、Vite `8.1.5`；本机 Node `24.15.0` 与 npm `11.12.1` 满足 Vite 8 运行条件。
- 当前 `Message`/`MessageResponse` 只有一个构造点，增加可空 conversation/client ID 可以在 models 转换处集中兼容。
- settings 表已是通用 KV，但现有 Rust 只为个别键提供函数；新增两个通用 get/set helper 即可承载主题、头像引用和截图快捷键，不需要新配置层。
- Tauri 2 官方 Vite 接入使用 `beforeDevCommand`、`beforeBuildCommand`、`devUrl` 和 `frontendDist`；本项目保持生产输出 `src/`，开发端口固定为 1420。
- 现有 `upload_file_internal` 已集中分块、秒传、进度与离线保存，但参数绑定 Tauri `State`；Tauri 群文件可复用它逐成员读取同一源文件，Web 群文件需在 Axum staging 后复用 `upload_to_receiver`。
- 非 Android `/api/media` 当前为空实现；新文件预览应改为按数据库 message ID 解析路径，不能继续接受任意路径或只返回 404。
- React adapter 已把新 seam 固定为 12 个共享操作：workspace snapshot、群创建、会话消息读写、已读、搜索、会话状态、设备备注、会话文件、传输取消、本地副本删除和截图；旧 API 只作为 direct/旧后端 fallback。
- 前端 snapshot 会把 `capabilities` 映射为 camelCase，因此 Rust 可以继续输出现有 snake_case 字段；消息对象需要附带每成员 receipt 聚合计数，不能只返回旧 `MessageResponse`。
- 旧 `/api/upload` 在首块创建 `.downloading` 和数据库记录，因此共享取消必须同时中止发送端 token，并按稳定消息 ID 调接收端内部清理接口；只取消本机任务会留下孤儿临时文件。
- 当前 Tauri CLI 的应用参数转发需要 `cargo tauri dev -- -- --port ...`；单层分隔会让 `cargo run` 把 `--port` 当成自身参数。
- 浏览器截图和通知能力必须由浏览器 API 本地检测兜底，不能让服务端平台 capability 强行启用或禁用。
- 截图与浏览器上传使用受管临时目录；只有逻辑文件的全部子传输进入终态后才能清理，`waiting_peer` 必须保留源文件供重连恢复。
- 文件 trust boundary 必须使用数据库消息 ID 或稳定 transfer ID；HTTP `sender_addr`、任意 `save_path` 和宽泛 asset protocol 都不能进入文件或网络操作。
- 接收分块、取消和重试需要按逻辑文件串行化，并用条件状态迁移保证双击、迟到清理和 cancel/complete 竞态幂等。
- 设备信息闪动已用双实例复现：广播与 reply 分别调用非确定性的 `local_device_metadata()`，可选中两个不同网卡 MAC；reply 同时固定携带内存 `0`，两组值被 PeerManager 和数据库周期覆盖。
- `infoOpen` 默认展开仅来自 React `useState(true)`，不是后端状态。
- 表情功能在 React 重构时整套漏迁；原型已有 21 个 Unicode 表情和光标插入行为，后端无需改动。
- 截图当前写入 `$TMP/xchat-captures` 并立即发送；传输终态会删除受管临时源文件，但数据库仍保留路径，因此文件中心和发送方历史必然出现失效路径。
- 非 Android `/api/media` 只允许已接收下载文件；直接放宽会通过监听在 `0.0.0.0` 的本地服务暴露发送方源文件。桌面历史预览应改为按 message ID 校验后经 Tauri IPC 读取。
- 当前 Composer 只有文本 state；文件选择、拖放和截图都直接发送。附件草稿需要在 `XChatModule` dispatch 层形成一个统一批量动作。
- 当前文件中心以传输状态作为左侧筛选，缺少原型要求的来源导航、类型芯片、来源头像、预览模态框和双击预览。
- macOS `RunEvent::Reopen.has_visible_windows` 会把钉图窗口也计入；若据此跳过，主窗口隐藏时 Dock 点击仍可能不恢复，因此重开事件应无条件 show/unminimize/focus 主窗口。
- 双实例联调确认接收端图片可通过受控 media API 内联显示；Web 发送端源文件位于 `xchat-web-staging`，传输终态清理后数据库路径失效，因此同一发送端刷新后仍会退化为文件卡片。修复必须把“用户上传到 Web 的受管副本”持久化，而不能放宽任意发送源文件的 HTTP 读取。
- Web 上传已改为 `<download_path>/.xchat-outbox/<uuid>/<safe-name>`；只有由数据库消息 ID 定位且 canonical path 位于该受管目录内的 outgoing 文件可读，任意原始发送路径继续 403。
- 截图文本输入必须在导出前同步提交到操作 ref；只更新 React state 会在同一事件循环内重绘时丢失最后一次文本操作。

## 技术决策
| 决策 | 理由 |
|------|------|
| React 外部 seam 仅暴露状态快照、动作分发、订阅 | 让聊天、文件、已读、取消和截图共享一个状态来源 |
| 内部保留 Tauri 与 HTTP/WebSocket 两个真实 adapter | 这是现有桌面和 Web 的真实差异，不创建假抽象 |
| 新 Rust 行为放在共享核心，commands/handlers 只转换参数 | 防止桌面和 Web 功能漂移 |
| direct 会话 ID 由两端 UUID 排序生成，送达/已读按消息 ID 保存 ack | 不依赖本地行 ID或设备时钟，群聊人数可以真实推导 |
| 兼容迁移只增加表和可空字段 | 保留旧消息、用户和设置数据 |
| 搜索先使用 SQLite 普通查询 | 当前规模无需增加 FTS 基础设施 |
| macOS 用系统交互式截图，Web 用 `getDisplayMedia` | 优先使用已有平台能力，避免截图框架 |
| Windows/Linux/Android 截图 capability 本轮为 false | 不用 mock 或无效按钮伪装跨平台支持 |
| 表情使用固定 Unicode 集合 | 零依赖、离线可用，且与原型已有交互完全一致 |
| 截图编辑器为同一 React bundle 的独立 Tauri window | 可保持置顶、单例和主窗口草稿解耦，无需另建前端工程 |
| 桌面媒体按 message_id 读取，不接受任意历史路径 | 复用数据库和可信目录边界，避免扩大本地 HTTP 暴露面 |
| Web outgoing 只开放受管 `.xchat-outbox` | 兼顾刷新后发送方内联预览与本地文件安全边界 |

## 遇到的问题
| 问题 | 解决方案 |
|------|---------|
| 设计目录未被 Git 跟踪 | 只读取并引用，不删除、不移动、不顺带提交 |
| 旧 Rust 代码编译时内嵌 `src/css/vscode.css` | React 构建的 public 资源保留该兼容文件，避免无关后端改造 |

## 资源
- `ui-ref/DESIGN.md`
- `ui-ref/README.md`
- `ui-ref/SKILL.md`
- `ui-ref/xchat-desktop-prototype.html`
- `src-tauri/src/db.rs`
- `src-tauri/src/network/`
- `src-tauri/src/commands.rs`
- `src-tauri/src/web_server.rs`

## 视觉/浏览器发现
- 主导航选中时仅图标使用绿色；会话选中时整行使用绿色。
- 编辑器是完整描边容器，高度 66–200px，Enter 发送。
- 设备身份需要备注、hostname、地址、MAC 和在线/发现信息。
- 文件行必须同时呈现阶段、大小或进度、速度或原因，以及可执行动作。
- 小于 1100px 收窄文件列，小于 1000px 隐藏详情栏，小于 860px 隐藏列表栏。
