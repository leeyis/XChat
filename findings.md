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
- 2026-08-20：用户截图中的“最大并行通道”完整文案只存在于 `ui-ref/xchat-desktop-prototype.html`；生产 `SettingsWorkspace`、设置归一化、Tauri/HTTP 设置接口和可见 Git 历史均没有对应字段。
- 2026-08-20：当前并行文件 v2 并非缺少并行能力，而是 `parallel_chunk_ranges` 固定把大文件分成 4 份；旧端能力回退、失败回滚、断点恢复、SHA-256 校验和 finalize 幂等已经有稳定测试基础。
- 2026-08-20：跨网段端点修复提交没有修改 `frontend/`，因此该设置缺失不是本次端点修复造成；设计文档明确将 4/8/16 配置列为阶段 E。
- 2026-08-20：用户已基于现有原型要求完整实施。实现必须同时覆盖前端、Tauri/HTTP 设置、SQLite KV、发送端分块/并发调度和兼容语义，不能只恢复一个无效下拉框。
- 2026-08-20：`WorkspaceSettings` 已是桌面/Web 共用设置快照，Tauri 与 HTTP 也共享通用 SQLite settings KV；新增 `max_parallel_channels` 可以沿用现有 seam，无需数据库迁移或新增命令/路由。
- 2026-08-20：当前发现只声明 `parallel_file_v2`，PeerManager 会保存权威 capabilities；这足以把旧 v2 识别为固定 4 通道，但 8/16 必须增加可协商的新能力，不能直接向旧接收端发送不同 manifest。
- 2026-08-20：当前 `upload_parallel_chunks` 一次把全部四个范围放入 `FuturesUnordered`，没有跨文件共享的并发控制。完整实现需要进程级共享限流代际：同一设置代际的新传输共享上限，保存新值后创建新代际，已有传输继续持有旧代际，符合“只影响新开始传输”。
- 2026-08-20：为兼顾全局公平性，新协议不能继续只生成与通道数相同的超大范围；应生成有界的小范围队列，每个文件只运行至多协商通道数的 worker，每个范围分别取得全局公平信号量许可，使后到文件能在前一文件的范围完成后获得通道。
- 2026-08-20：`UploadJob` 目前只有 `parallel_v2: bool`；首次发送、离线恢复和显式重试都只根据对端是否声明 `parallel_file_v2` 决定协议。新实现应把它提升为显式传输计划（顺序 v1、固定四范围 v2、可调 worker 的 v3），确保所有入口使用相同协商结果。
- 2026-08-20：接收端不仅在 prepare 时要求 `chunks == parallel_chunk_ranges(file_size)`，加载落盘 manifest 时也再次要求固定四范围且 `version == 2`。支持公平的小范围队列必须新增 manifest 版本并用“索引连续、offset 无缝、长度正数、完整覆盖且数量有界”的独立校验，同时继续接受旧 v2 manifest。
- 2026-08-20：`send_path` 会为同一逻辑文件的所有在线收件人同时 spawn 独立上传；因此全局上限必须位于共享 transfer runtime，而不是单个 `UploadJob` 或单个会话内部。SHA-256 仍可按现有方式只计算一次并在各收件人任务间复用。
- 2026-08-20：`network::transfer` 已有进程级 `OnceLock<TransferCancellationRegistry>` 先例；可在同一模块加入可独立测试的 `TransferConcurrencyController` 和生产单例，避免把调度状态塞进 Tauri/Axum 两套 AppState。
- 2026-08-20：React `normalizeSettings`、`SETTINGS_PATCH_KEYS`、Tauri/Web `patchSettings` 与现有双 adapter 对等测试提供完整前端 seam。新增字段应统一命名为后端 `max_parallel_channels`、Tauri payload `maxParallelChannels`，非法或旧快照统一归一为默认 4。
- 2026-08-20：Rust `get_settings`、workspace snapshot、HTTP settings 和两条更新入口均可复用同一 transfer setting helper；允许值只接受 4/8/16，缺失或损坏的历史值读取为 4，显式非法更新返回错误而不覆盖旧值。
- 2026-08-20：接收端在 manifest 通过校验后，分块接收、恢复扫描、进度聚合、顺序合并和 SHA-256 校验均按 `manifest.chunks` 泛化工作；v3 的主要接收改动可以限制在新路由、manifest 版本与安全覆盖校验，不必重写稳定的数据落盘状态机。
- 2026-08-20：实施计划已固化为七个可独立验证阶段；协议选择明确为 v1 顺序、v2 固定四范围、v3 可调 worker，所有实际 HTTP 分块请求统一经过进程级代际信号量。
- 2026-08-20：`TransferConcurrencyGeneration::acquire` 将一次 semaphore 排队 future 固定到整个等待期，只用短周期定时器观察原子取消 token；这样既保留 Tokio FIFO 排队公平性，又能在取消时及时退出且不会拿走许可。
- 2026-08-20：设置更新入口必须先验证 `max_parallel_channels` 再写其他字段；HTTP/Tauri 当前并非整包事务，前置验证可确保非法 12 不会造成同一请求中其他设置的部分更新。
- 2026-08-20：v3 capability 采用 `parallel_file_v3:<max>`，当前广播 `parallel_file_v3:16` 并继续广播 v2；解析只接受 4/8/16，缺失或畸形 v3 能力会安全回退 v2 或 v1。
- 2026-08-20：v3 对大于 4 MiB 的文件至少生成 `channels × 4` 个平衡范围，并限制最多 4096 个；接收端不信任生成算法，而是独立校验连续索引、无缝 offset、正长度、checked-add 无溢出和完整覆盖。
- 2026-08-20：全局 permit 的正确生命周期必须覆盖请求 body 上传和响应 body 读取；只在创建 future 时取许可会过早释放。当前 v1/v2/v3 均在整个 HTTP 数据请求完成或被取消后才 drop。
- 2026-08-20：仅在 permit 排队时响应取消仍不足够；真实测试复现服务器已收完 body 但迟迟不回响应时旧逻辑会卡到 HTTP timeout。通用 cancellable future 包装现覆盖 prepare、send 与 response body，取消会 drop 网络 future 并同步释放许可。
- 2026-08-20：前端把设置差异计算抽成共享纯函数后，可同时约束 dirty/save/reset；生产控件沿用既有 `SettingRow` 和 select 样式，不需要新增 CSS，并按批准原型在 4/8/16 间动态解释资源占用与旧端四通道回退。
- 2026-08-20：推荐保留 `/api/uploads/v2/*` 的固定四范围契约，并新增 `/api/uploads/v3/*`；新 discovery capability 声明 v3 与最大 16，发送端仅在对端明确声明时使用 v3。这样旧接收端永远不会收到其无法解释的新清单。
- 2026-08-20：全局限制应覆盖 v1 顺序上传、v2 固定四范围和 v3 worker；v1 每次 HTTP 分块占一个许可，v2/v3 每个范围请求占一个许可，才能让“所有文件传输共享全局上限”在混合版本设备间成立。
- 2026-08-20：现有 Rust 测试已具备真实 Axum fake receiver、临时 SQLite 和 `run_upload` 调用 seam；可扩展为两个 v3 上传并记录同时处理的范围数，直接验证跨文件峰值不超过设置且第二个文件能在第一个完成前取得通道，而不是只测试信号量内部字段。
- 2026-08-20：现有 Web 测试直接调用 prepare handler 并检查 manifest/状态，适合先锁定 v2 仍只接受固定四范围、v3 接受安全覆盖清单，以及重启加载 v2/v3 manifest 的兼容行为。
- 2026-08-19：`2026-08-18-xchat-network-presence-message-reliability-design.md` 明确标注“产品方向与 UI 原型已确认，工程方案待评审”；因此原型门禁已满足，但仍需先审查工程分阶段与现有调用链，不能把 A–E 五阶段一次性合并实施。
- 2026-08-19：最新原型已覆盖按接口发现、代理 TUN 风险确认、固定地址入口、四态连接横幅、三档上下线提醒、真实消息状态文案及 4/8/16 最大并行通道。原型脚本中的定时状态推进有明确注释仅供原型演示，生产实现必须由持久化、传输写入和明确 ACK 事件驱动。
- 2026-08-19：当前工作树位于 `main`，领先 `origin/main` 1 个文档提交，且 `src/index.html` 有用户修改；后续实现必须避免覆盖该文件和已有工作。
- 2026-08-19：代码图确认发现热点集中在 `network/discovery.rs` 的 `get_smart_broadcast_addresses`、`start_announcing`、`send_single_broadcast`；在线状态仍由 `PeerManager.force_mark_offline`、`mark_stale_as_offline`、`get_all_peers` 等多入口承担，和设计提出的单一 Presence 写入者存在明确差距。
- 2026-08-19：现有可靠性基础仍可复用：`workspace.resend_for_peer`、数据库逐设备未送达消息/待回执查询以及单调消息状态写入均已存在。首个切片应避免提前重写 Outbox，而应先停止发现扇出并建立可测量的接口发送计划。
- 2026-08-19：知识图的符号和边仍可查询，但 `get_code_snippet` 返回的绝对路径指向旧目录 `/Users/eason/workspace/40-49_Code&Tech/XChat` 且源码不可用；必须用当前仓库 `/Users/eason/workspace/40-49_Code/XChat` 刷新索引后再做精确实现判断。
- 2026-08-19：刷新后精确源码确认：`start_announcing` 每 2 秒同时执行“每个本机 IP × 受限广播/组播”、约 260 个兜底目标、以及全部固定地址；`get_smart_broadcast_addresses` 仍明确生成 `192.168.0.255..192.168.255.255`，现有测试甚至要求保留该行为。阶段 A 必须先把这条测试改成“桌面永不生成 256 地址列表”的红灯约束。
- 2026-08-19：现有逐接口辅助函数只有 `Vec<String>` 本机 IP 和统一的 `255.255.255.255 + 224.0.0.167` 目标，没有前缀长度、稳定接口 ID、类型或用户选择，因此不能只删 256 地址循环就声称完成设计；阶段 A 至少需要把“发现发送计划”抽成可测试数据模型，并明确平台降级。
- 2026-08-19：`get_all_local_ips` 并不枚举系统接口，而是分别向多个公共地址做 UDP route probe 后收集源 IP；它天然拿不到接口名称、前缀和类型，也可能受默认路由/TUN 影响。要落实“按选择接口发现”，必须替换或补充这一数据源。
- 2026-08-19：手动单次发现 `send_single_broadcast` 也直接复用 260 目标列表；即使只改后台循环，用户主动刷新仍会触发同样的地址扫描，阶段 A 的发送计划必须同时覆盖稳态与单次刷新入口。
- 2026-08-19：`start_announcing` 同时由桌面 `main.rs`、移动/库 `lib.rs` 和 Web `server_main.rs` 启动，阶段 A 的共享核心改动会自然覆盖 Desktop/Web，但平台专用发送降级必须用窄 `cfg` 保留，不能让 Android 兼容逻辑重新污染桌面路径。
- 2026-08-19：`force_mark_offline` 由旧 Tauri `send_message`、Web `send_message_http` 和共享 `workspace.send_message` 三条路径直接调用；这证实 Presence 阶段必须先建立证据入口再逐步迁移调用者，不能直接删除布尔更新而破坏现有离线补发触发。
- 2026-08-19：项目已有统一 `get_settings/update_settings` 的 Tauri 与 HTTP 入口，设置扩展应复用该 seam；文件并行 v2 当前固定四分块，4/8/16 选项属于后续阶段 E，不应夹带进阶段 A。
- 2026-08-19：现有设置入口是参数式薄接口，目前只含下载路径、端口、数据库路径和自动下载。发现接口清单/选择是结构化数据，若直接继续堆 `Option<String>` 会迅速膨胀；更合适的是在共享核心定义窄的 `DiscoverySettings/NetworkInterfaceInfo` 数据模型，再由 Tauri/HTTP 入口薄封装。
- 2026-08-19：当前直接依赖只有 `socket2`，没有可枚举接口名称、索引、前缀和类型的网络接口 crate；`ipnet` 仅为间接依赖。阶段 A 若要求 Windows/macOS/Linux 一致实现，需要评审一个小型跨平台接口枚举依赖，或接受明显更大的平台 FFI 代码面。
- 2026-08-19 视觉核对：1440×900 下网络设置沿用现有设置卡片体系，主类别开关、固定地址入口、折叠式接口清单和风险标签层级清楚；接口行展示“名称 / 分类 / IPv4+前缀 / 推荐或排除 / 开关”，生产数据契约必须一次提供这些字段，不能靠前端从名称字符串猜分类。
- 2026-08-19 视觉核对：设置页对代理 TUN 和虚拟网卡默认关闭，对物理 LAN/组网 VPN 默认开启；折叠摘要显示启用/排除数量。该交互会直接依赖稳定接口 ID，否则 DHCP/IP 变化后无法保留用户选择。
- 2026-08-19 视觉/交互核对：离线会话在头部、横幅、详情和文件卡同时表现为离线/等待，用户新发消息立即显示“等待对方上线后发送”；点击重试是显式动作。生产实现必须让这些区域共享同一 Presence generation，而不是分别查询或本地猜测。
- 2026-08-19 视觉/交互核对：原型的离线横幅常驻且不挤压输入区，排队状态位于消息时间/回执位置；这说明阶段 B/C 的前端改动应以统一 snapshot/event merge 为主，不需要重做聊天布局。
- 2026-08-19：生产前端已经有 `mergeMessages` 的单调状态合并和 `statusLabel` 的离线 pending→“等待对方上线”文案，这两处可保留；主要缺口是后端未即时发专用 message status 事件、Presence 仍是布尔状态，以及 `peer.online/offline` 事件会无条件 Toast/系统通知。
- 2026-08-19：前端不是固定 `setInterval`，而是 `schedulePoll` 递归 `setTimeout`：存在活跃传输时缩短，否则可见页面走低频刷新。阶段 C 应保留它作为事件丢失后的修复路径，只让正常状态推进改走带 version 的事件。
- 2026-08-19 外部依赖评审：官方 docs.rs 显示 `getifaddrs 0.6.2` 可跨 Unix/macOS/BSD/Windows 返回接口 `name/index/flags`，地址对象提供 IP、netmask、关联地址和 MAC，最贴合原型及定向广播需要；`network-interface 2.0.5` 也提供 name/index/MAC/broadcast/netmask，但官方 README 仍声明 API 处于开发中；`if-addrs 0.15.0` 体积小且覆盖 Android/桌面目标，但接口模型只暴露 name+addr，不满足稳定 ID 和状态标志。
- 2026-08-19 外部依赖评审：`getifs 0.6.1` 功能最全并专门处理 Android 11+ SELinux 限制，但能力范围（路由、网关、MTU、multicast 等）明显超过阶段 A；若本轮只需桌面三平台接口清单和前缀，优先选择更窄的 `getifaddrs`，Android 可继续使用现有受限兼容路径并用 `cfg` 隔离。
- 2026-08-19：数据库已提供通用 `get_setting/set_setting`，可直接保存 JSON 结构的发现选择，不需要新增表或迁移；固定地址已有 `custom_peer:*` 复用路径。阶段 A 可以保持 SQLite 兼容，只扩展共享 settings 聚合接口。
- 2026-08-19：`src/index.html` 的现有用户改动仅是换行格式变化（内容和资源哈希未变）。后续前端验证应优先把 Vite 输出定向到临时目录，避免构建过程覆盖该文件；真正交付生产 bundle 时需显式保留这项用户改动并检查生成资产范围。
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

### 阶段 15 A0/A1 实施边界
- `start_announcing` 目前同时向每张探测到的源 IP 发送受限广播/组播，并再向 260 个“智能路由”地址发送；固定地址也跟随 2 秒循环，A0 必须统一替换这三条高频路径。
- 桌面与 Web 的 `start_listening` 都会对每一份非 reply announcement 立即单播回复；同一帧经广播和组播重复抵达时会重复回复，需共享一个短窗口回复去重器。
- discovery v2 帧没有 sequence 字段，因此 A0 不升级协议；去重键采用 peer ID、来源地址和原始帧摘要，并以短 TTL 只抑制同一轮重复包。
- 当前 `get_all_local_ips` 依赖 UDP 路由探针，无法提供接口名、索引、掩码或分类。A1 需要独立的接口清单模型；非 Android 使用系统接口枚举，Android 保留窄范围兼容回退。
- 现有设置接口只有下载路径、端口、数据库路径和自动下载；最小兼容扩展是在同一设置响应/更新请求中加入 `discovery_settings` 与只读 `network_interfaces`，避免新增 Tauri command 和权限面。
- React `SettingsWorkspace` 已有统一脏表单和保存动作，网络 UI 可以复用该保存链路；刷新接口列表则重新拉取 workspace 快照，不把运行时接口清单写回配置。
- Tauri 与 HTTP adapter 的 `patchSettings` 分别做 camelCase/snake_case 转换；二者必须同时发送完整发现配置，确保桌面和 headless Web 不漂移。
- A0 policy 的稳定接口 ID 使用规范化 system name（Windows 下为系统接口标识），不包含会在重启/热插拔后变化的 index 或 IP。开发期 `if:<index>:<system-name>` override 会迁移到新 ID，无法归属的 index-only override 会安全丢弃。
- 桌面/服务器使用 `getifaddrs` 的 name/index/flags/IPv4/netmask；Android fallback 只做 5 个本地路由探针且前缀未知时只组播，不恢复 256 地址发送或默认路由广播。
- 发送计划单轮接口数据报预算为 48；每个有效源地址最多一个定向广播和一个显式接口组播，`/31`、`/32`、未知或非法前缀不猜广播。
- A0 cadence 的三次启动发送累计时点为 0/400/1500ms，随后按 24–36 秒抖动；接口指纹每 5 秒检查，变化会立刻重置突发，而不会把发现继续当 2 秒在线心跳。
- 固定地址共享单个无绑定 UDP 单播 socket，但每个 endpoint 各自维护 5 秒起步、300 秒封顶的指数退避；日志只写 endpoint 哈希，不泄露地址或域名。
- v2 兼容去重在 listener 落库/发事件/回复之前执行，键为 peer ID、来源 IPv4 与原始帧摘要，TTL 2 秒；这能抑制广播/组播双路径重复包，待 v3 sequence/interface ID 后再替换为协议级键。
- 发现偏好无需 SQLite migration：现有 `settings` KV 以 `network.discovery.settings.v1` 保存 JSON；损坏 JSON 记录诊断后回到安全推荐值，未知字段被忽略以便前向兼容。
- 设置更新通过 `Notify` 唤醒同一公告循环，保存后立即重建计划并启动三包突发；接口自然变化仍由 5 秒清单指纹检查捕获。
- workspace 快照、旧 Tauri `get_settings` 与 HTTP `get_settings` 现在返回同一 `discovery_settings`/`network_interfaces`；两个更新入口也共同调用 policy 持久化函数，没有新增 command/route/权限。
- 24–36 秒发现节奏不能直接沿用旧版 10 秒离线阈值。A0 过渡期采用 75 秒本地阈值，并以 `is_reply=true` 的有界轮转单播心跳兼容旧客户端：32 台以内每 3 秒一批，规模增大时自适应加速，明确保证最多 128 台时丢一轮并叠加 2 秒解析阻塞仍低于旧版 10 秒阈值；Presence v3 上线后再移除该桥。
- 固定地址发送与 listener 白名单解析都按 16 个/轮轮转；失败独立指数退避，listener 在同一网络指纹内保留上次成功 IP，网络变化时清空 DNS 与 last-good，兼顾临时故障连续性和跨网络安全边界。
- Android 前缀受限 fallback 仅发送组播；Activity 在前台生命周期持有并在停止时释放 `WifiManager.MulticastLock`，避免 Wi-Fi 过滤使发现完全失效，同时不在后台长期关闭组播过滤。
- 只控制主动发送不够：监听器也必须按启用接口维护组播 membership，并在落库、事件和回复前验证真实 ingress interface。Unix 可依赖 kernel packet metadata；Windows 缺少等价路径时只允许启用子网或显式固定地址的保守匹配。
- 网络/设置变更不仅要重建发送计划，还要重置固定地址退避和 DNS 缓存；否则刚修复的路由仍可能等待旧的 300 秒 backoff。
- 接口枚举属于可失败的运行时事实，不能让 workspace/settings API 因此整体 500，也不能在失败时恢复默认扫描。API 返回空清单，announcer 保留 last-good 或 fail closed。
- `/1` 的定向广播数学结果是 `255.255.255.255`，等同禁止的 limited broadcast；因此宽前缀即使掩码合法也只生成绑定接口的组播目标。

### 阶段 16 用户确认与代码现状
- 用户明确要求 Windows 也达到严格接口隔离，Android 可以暂缓，并且不需要为旧版本客户端保留兼容心跳。
- 关闭发现后地址仍在数据库与 `PeerManager`，但约 75 秒后会标记 `is_offline=true`；`workspace::send_message` 等发送路径只选择 `!is_offline` peer，因此普通已知设备不会尝试直发。
- “128 台保障”只来自 A0 临时加入的旧版 v2 单播兼容桥：每批 32 台、自适应 3 秒轮转；删除该桥即可同时删除数量上限、调度、预算、指标与测试，不影响当前发现帧。
- Windows 当前 `recv_discovery_packet` 走 `recv_from` 并把 `ingress_index` 固定为 `None`，只能按来源子网猜接口；重叠子网下无法严格过滤禁用接口或保证原接口回复。
- Microsoft Winsock 官方接口满足严格实现：IPv4 UDP socket 开启 `IP_PKTINFO` 后，`WSARecvMsg` 的控制消息会返回 `IN_PKTINFO.ipi_ifindex`（实际 ingress）和本地目标地址；`WSASendMsg` 也可通过同一结构指定发送接口和源 IPv4。扩展函数指针必须用 `WSAIoctl(SIO_GET_EXTENSION_FUNCTION_POINTER, WSAID_*)` 取得。
- 当前项目只有 Unix 直接依赖 `libc`，Windows 尚无直接 Win32 API 依赖；可新增 target-only `windows-sys` 的 Winsock feature，避免影响 macOS/Linux/Android 构建。
- 官方依据：https://learn.microsoft.com/en-us/windows/win32/winsock/ip-pktinfo 、https://learn.microsoft.com/en-us/windows/win32/api/ws2ipdef/ns-ws2ipdef-in_pktinfo 、https://learn.microsoft.com/en-us/windows/win32/api/mswsock/nc-mswsock-lpfn_wsarecvmsg
- 用户否决向离线设备的历史 IP 试发：DHCP 地址可能已重新分配，单凭旧 IP 发送存在把协议载荷发给另一台设备的风险。最终设计必须保留 `is_offline` 硬门禁；若未来要安全离线直发，需要先建立经过认证的设备身份握手，而不是信任 IP 或可伪造 UUID。
- 因此“关闭发现后已添加设备仍可通过已知地址通信”的原型文案不再成立。安全语义应为：动态设备停止发现后约 75 秒离线并停止发送；显式固定地址仍通过发现握手刷新设备身份，而不是绕过离线门禁。
- 用户要求把上述后果改写成普通用户能直接理解的文案，并在“手动添加设备”中增加测试功能。
- 当前原型弹窗只有地址输入与“添加设备”，生产链路也只是把字符串保存为 custom peer；没有即时解析/握手结果，也没有展示或绑定远端设备 ID。
- 有意义的测试必须完成 XChat 身份协议而不是只做 DNS、ping 或 UDP `send_to`；最终采用专用只读 HTTP identity endpoint，使手工测试、保存前重验和文件发送前核验复用同一返回设备名、hostname、设备 ID 与耗时的 seam。
- 为回应 IP 复用风险，推荐测试成功后把 endpoint 与返回的稳定 device ID 一起保存；后续该地址若回复不同身份，应拒绝关联并提示重新测试，而不是静默接受另一台设备。

### 阶段 17 设备身份与地址复用诊断
- 当前并未把 IP 当作会话身份：`settings.user_id` 首次启动生成 UUID 并持久化；`users.id`、`PeerManager` 与稳定单聊 ID 均以该 UUID 为键，发现同一 UUID 出现在新地址时会更新 `addr`。
- 因此张三从 `.22` 变为 `.111` 后，只要新公告已到达，旧会话仍指向张三的 UUID，发送路径会使用更新后的 `.111`；新占用 `.22` 且 UUID 不同的设备不会被合并进张三的会话。
- 接收端会根据“本机 UUID + 发送者 UUID”重算稳定会话 ID，不匹配就拒绝保存/展示，所以当前格式消息通常不会出现在错误设备的聊天界面。
- 仍有真实安全窗口：旧地址尚未刷新且 75 秒离线租约未过期时，发送端会直接连接旧 IP；WebSocket 在发送正文前没有目标身份握手，`Ok(())` 只表示连接并写入成功。正文因此可能已经到达错误主机进程，再被会话 ID 校验拒绝，发送方还可能短暂标记为 `sent`。
- MAC 不能作为唯一应用身份：它标识网卡而非用户/设备安装，一个设备可能有多个网卡，随机 MAC、虚拟网卡、换网卡/重装与伪造都会破坏唯一性；跨子网/VPN 时也往往拿不到对端二层 MAC。当前 MAC 只是 discovery 自报元数据，不是可信证明。
- 推荐模型是三层分离：持久设备身份（最终为公钥指纹，现阶段 UUID）、短租约 endpoint（IP:port）、辅助网络证据（MAC/hostname）。发送正文前必须以轻量 XChat 身份握手确认 endpoint 返回的设备身份与会话 peer ID 相同；手工添加“测试连接”复用该握手。
- 桌面原型已按该模型改写：设备列表/详情/群成员以设备 ID 为身份，IP 改称当前地址，MAC 改称网卡地址（辅助）；离线横幅明确说明未向旧地址发送。
- 手工添加现为“输入地址 → 测试连接 → 展示设备 ID/主机名/地址/耗时 → 保存设备”，保存按钮在成功核验前禁用；连接失败与同地址身份变化均有独立阻断状态。
- 原型中的“身份核验”是目标协议语义，不代表当前 UUID 已具备抗伪造认证能力；生产协议至少要先比较预期 UUID，再演进到公钥挑战签名。

### 阶段 16 设备身份安全实施结论
- 上述“正文先到错误进程”的 DHCP 复用窗口已收敛：peer WebSocket 先携带期望 UUID 完成 upgrade，服务端在 upgrade 前核对本机 UUID，客户端再核对响应身份头；两边均通过后才写正文。
- `is_offline` 仍是发送硬门禁。离线 direct 消息与文件保存为 pending/`waiting_peer`，不连接历史 IP；发送过程中身份不符或连接失败会立刻停止并把 direct peer 标记离线，等待新的 discovery 上线跳变触发补发。
- UUID 是现有安装级设备主身份，IP:port 是短期 endpoint，MAC/hostname 只是辅助展示。MAC 不能解决多网卡、随机化、跨子网不可见和伪造问题，因此没有升格为主键。
- 手工地址保存现在是“探测 → 展示身份 → 后端按期望 UUID 重新探测 → 结构化持久化”。旧裸地址不会参与自动 discovery；固定来源发来其他 UUID 的 announcement 会在更新 PeerManager/数据库前被丢弃。
- Windows listener 已不再以来源子网猜 ingress：`IP_PKTINFO` + `WSARecvMsg` 提供真实 interface index，禁用接口过滤和回复源地址选择与 Unix 路径使用同一策略。
- 当前 UUID 握手防的是误投和普通地址复用，不是恶意设备冒充；对抗主动攻击仍需要公钥身份、挑战签名和首次信任/指纹确认。这是后续安全演进，不影响本轮“不要因为 IP 变化发给别人”的目标。

## 视觉/浏览器发现
- 主导航选中时仅图标使用绿色；会话选中时整行使用绿色。
- 编辑器是完整描边容器，高度 66–200px，Enter 发送。
- 设备身份需要备注、hostname、地址、MAC 和在线/发现信息。
- 文件行必须同时呈现阶段、大小或进度、速度或原因，以及可执行动作。
- 小于 1100px 收窄文件列，小于 1000px 隐藏详情栏，小于 860px 隐藏列表栏。
