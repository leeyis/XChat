# 发现与决策

## 需求
- 旧 HTML 界面完全弃用，以 `ui-ref/xchat-desktop-prototype.html`、`ui-ref/DESIGN.md` 和对应素材为准。
- React 实现聊天、主机、文件、设置四个模块，不实现 AI 诊断。
- 现有 Rust 后端能复用则复用；群聊、已读回执、传输取消、截图快捷键等缺失能力本轮真实实现。
- 完成后提供运行、预览和验证命令。
- 0.1.0 追加范围：品牌与图标、空状态、设置导航、Dock 重开、表情、截图编辑、附件草稿、图片内联和严格原型文件中心。
- 截图首版必须支持矩形、椭圆、箭头、画笔、马赛克、文本和回退。
- 二次测试反馈要求：表情数量增加且点击后自动收起；截图编辑工具栏参考微信并悬浮在选区下方；默认本地名称取系统机器名。
- 文件补充要求：从系统拖文件到输入区后先进入附件草稿，点击发送才传输；发送时显示进度和速度并支持并行传输；完成文件的“打开”使用可选菜单同时提供打开文件和打开目录。

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
- 真实运行实例的同一 peer 在 12 秒内出现两个非空 MAC 和多个非零内存值；进程级 OnceLock 只能保证单个发送进程稳定，接收端仍会被相同 device ID 的多个详细来源覆盖。
- 现有 `legacy_heartbeats_do_not_erase_discovered_metadata` 只验证 0/空字段不覆盖，不能捕获两个详细来源携带不同非空值的周期跳动。
- 微信桌面截图的常见结构是：完成选区后把工具栏贴在选区下方；工具顺序通常为方框、椭圆、表情、箭头、画笔、马赛克、文字、撤销，绘制类工具再显示颜色和粗细。参考：https://www.shujuwa.net/weixin/wechat-screenshot-shortcut-keys
- 用户提供的新版微信截图图进一步显示：工具条为浅色圆角悬浮条，主工具与撤销/重做、取消、钉图、保存、完成之间用分隔线分组；当前 Xchat 的整页顶部表单式工具栏与该层级明显不符。
- Snipaste 的更新记录说明工具条需要根据截图区域和屏幕边界调整位置，并把文字/画笔作为直接快捷标注工具；本轮只复用这些已明确的交互，不引入 OCR、长截图或二次编辑等新增范围。参考：https://zh.snipaste.com/download.html
- 当前 `EMOJI_SET` 固定为 21 个 Unicode；`insertEmoji` 已正确按 selectionStart/selectionEnd 插入并恢复光标，但点击后没有调用 `setEmojiOpen(false)`。
- 当前截图编辑器把工具栏作为 `grid` 的第三行全宽 footer，全部使用带文字的普通按钮；与微信式选区下方圆角悬浮工具条的差异主要在布局和控件层级，Canvas 操作序列、导出、完成和钉图数据流无需重写。
- 现有 `capture-drawing.test.js` 已覆盖六类绘制命令，悬浮工具栏改版可只增加“工具选择/撤销重做状态”的轻量测试，不需要引入新的画布库。
- 当前新数据库在 `init_db_with_path` 中调用 `generate_random_name()` 写入 `username`，因此设置页显示 `Happy-Fox-662` 等随机名；系统 hostname 已由 `local_device_metadata()` 读取，但没有用于默认用户名。
- “默认取机器名”需要同时覆盖新数据库和历史自动生成名：新库直接写 hostname；旧库只在名称匹配内置 `形容词-动物-三位数` 生成规则时迁移，不能覆盖用户自行修改过的名称。
- 用户截图显示发送方文件卡只有“传输中 · 总大小”和取消按钮，底部另有全局传输条，但两处均缺少 `bytes_transferred / bytes_total`、百分比和速度；接收完成卡只有单一“打开”动作。
- React Composer 的浏览器 `onDrop` 已能把 `dataTransfer.files` 加入草稿，但尚未证明 Tauri WebView 的系统文件拖入一定会进入 DOM drop；桌面路径需要同时核对 Tauri drag-drop 事件。
- 当前稳定传输协议按 4 MiB 分块串行发送，接收端对同一 `client_message_id` 加全局异步锁、按当前文件长度校验预期 offset，并以 append 模式写入；直接让多个 chunk 并发会被串行锁抵消，或在去锁后造成乱序/损坏。
- `TransferRecord` 已包含 `bytes_transferred`、`bytes_total`、`created_at`、`updated_at`，前端可以用连续快照差分计算瞬时速度并直接显示进度，无需先扩数据库 schema。
- 真正的单文件并行分块需要把接收端改为随机 offset 写入、记录已完成 chunk 集合并在全部完成后原子 finalize；它不是只把一个 `for` 换成 `join_all` 的前端优化。
- 截图现有 macOS 流程先用系统交互式选区生成裁剪图，再打开独立 Tauri 编辑窗；本轮可以把工具栏贴在编辑窗内图片下方，但若要求保留原屏选区控制点并让工具栏随原屏选区移动，则必须重写成透明全屏捕获层并处理多屏/Retina 坐标。
- 精细采样和 SQLite 对照排除了 React：reply 帧携带 `memory=0` 但仍带另一网卡 MAC，权威 announcement 携带动态 available memory；两类帧交替写入 PeerManager 与 users 表。MAC 应只允许非 reply 权威帧更新，内存只接受首个正值。
- MAC 不能简单“首值锁定”，因为首包可能是错误网卡 reply；权威 announcement 必须可以修正历史错误值。内存则可锁定首个正值，保持当前字段语义而不再闪动。
- Tauri 2 的 Webview drag-drop 事件会提供原生文件 `paths` 和指针位置，可按坐标确认落点在 Composer 后复用现有附件草稿。官方接口：https://v2.tauri.app/reference/javascript/api/namespacewebview/#ondragdropevent
- Tauri 官方还明确要求组件卸载时调用返回的 unlisten，并提示调试器停靠时 drop position 可能不准；实现会清理 listener，QA 时关闭/分离 DevTools 后验证输入区坐标。
- 用户已明确要求单文件 4 分块并行，因此必须新增能力协商的协议 v2，并保留 v1 顺序 append 回退；只做多个文件并发不满足需求。
- 协议 v2 推荐先 prepare/accept，再并发最多 4 个带 index/offset 的 chunk。接收端每块独立临时文件并幂等落盘，全部到齐后按序合并、校验总长度并原子改名；避免跨平台随机写 API和乱序 append。
- 活动传输速度可由连续 transfer 快照的 byte delta / monotonic elapsed 计算；活动时约 1 秒刷新、空闲停止，不需要持久化瞬时速度。
- 本机 IP 不能用公网目标推导默认路由：Clash TUN 会返回 `198.18.0.1`；连接 Xchat 已使用的局域网组播地址可稳定选中物理 LAN 接口，本机实测返回 `172.27.94.249`。

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
| v2 prepare 冲突会先复活失败任务 | 在修改 transfer/message 状态前校验既有 manifest，并用回归测试锁定 |
| v2 高并发失败路径存在状态与临时文件不一致 | 分别补能力降级、分块回滚、远端失败和 finalize 恢复的聚焦测试与最小状态机修复 |

### v2 稳定性修复笔记
- `receive_parallel_chunk` 只有读取流、长度越界和普通 write 失败会清临时文件并回滚；进度 SQL、flush、锁后 DB 查询、rename 和最终校正仍会直接 `?` 返回。
- 聚焦测试使用 SQLite trigger 令第二次进度更新失败：第一次上报成功，第二次失败，验证本次已上报字节回到 0 且不遗留 `.tmp`。
- 现有远端终态 endpoint 对 `status=failed` 本就保留 v2 part；发送端遗漏来自 `run_upload` 的 `!job.parallel_v2` 条件，移除即可复用安全边界。
- finalize 的重复文件来自“hard link 已成功、DB 尚未完成”的崩溃窗口；重试时校验并复用 manifest 预期路径即可保持幂等，无需引入新存储层。

### 阶段 13 根因与集成边界
- 当前主线缺少完整钉图窗口行为、系统通知/托盘提醒和设备上线通知；这些能力已经集中存在于提交 `9c27425`，完整合入比逐段重写更小且能保留同一旁枝的协议与权限配套。
- 当前文本操作没有稳定标识，历史模型只支持末尾追加/弹出，因此无法对既有文本做一次性的替换或移动，也无法把一次编辑作为单个撤销步骤。
- 设置导航粉红色来自 `.settings-nav-row.selected` 的显式混色背景；删除该背景即可保留绿色文字/字重和灰色 hover，无需新增状态。
- 用户已暂存 `AGENTS.md`、`plan/UI-DESIGN-PC.md`，并保留 `.happycode/`、`ui-ref/`；集成过程不得把这些内容纳入产品提交或覆盖。
- 通知旁枝原先会在“页面可见但窗口失焦”时先启动托盘提醒，又被自动已读立即清除；消息提醒、自动已读和清除注意力必须统一使用 `visible && hasFocus()`。
- Web 截图编辑器的 pending 数据来自 localStorage，可用受控图片完成无真实截图权限的浏览器交互回归；本轮实测二次编辑、原子撤销/重做、拖动定位和钉图菜单均通过。

### 阶段 14 截图交互根因
- 预览重绘用 `hiddenTextId === operation.id` 隐藏编辑中的文本；无文本编辑时两边均为 `undefined`，因此所有没有 ID 的马赛克、画笔、矩形等已提交操作都会被跳过。拖动草稿可见，松手进入 history 后立即消失。
- 文本单击后变成 `.capture-text-input`，该输入框直接阻止 `pointerdown` 冒泡，所以编辑态拖动无法进入 `beginInsideSelection → movePointer → endPointer`。
- 空文本当前被 `createTextOperation` 返回 `null`，`commitText` 随即退出并恢复原操作；用户最新语义要求空值删除，历史模型需要一个可撤销的 remove 操作。
- 钉图视图仅加载已经扁平化的 PNG，没有 Canvas/history；现有缩放小工具条不是截图编辑工具条。最小完整方案是“显示工具条”时把当前钉图重新送入同一个 `CaptureOverlay` 编辑会话，编辑后再次钉图覆盖当前钉图。
- 修复后浏览器真实回放中，马赛克 Canvas 哈希由基线 `3844884312` 变为 `1185266231`，松手后仍保持 `1185266231`；文本编辑框实时从 `(320,299)` 移到 `(420,359)`，清空后消失，撤销后在新位置恢复。
- 钉图右键“显示工具条”现在挂载同一个 `CaptureOverlay`，整图选区锁定为 viewport，原独立 `.capture-pin-toolbar` 不再存在；取消编辑返回原钉图且 pending 不变。
- 浏览器 `HttpWsAdapter.pinCapture` 仍只用 `open(dataUrl)` 模拟钉图，不能验证原位覆盖；Tauri `pin` 已支持 existing-pin 原位替换并用会话校验保留失败前旧图。

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
