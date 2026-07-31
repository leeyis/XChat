# 进度日志

## 会话：2026-07-29

### 阶段 12：测试反馈二次收敛
- **状态：** in_progress
- 执行的操作：
  - 开始收敛并行传输 v2 审查发现：权威空能力降级、接收分块失败回滚、发送失败远端状态，以及 finalize 崩溃恢复。
  - 权威 discovery 现在会用空 capabilities 清除旧 `parallel_file_v2`，而非权威 reply 仍不会覆盖权威能力；聚焦测试 2/2 通过。
  - `receive_parallel_chunk` 的进度 SQL、flush、锁后 DB、rename 与校正失败统一清理本次临时/已发布 part 并回滚本次已上报增量；SQLite trigger 故障注入测试由红转绿。
  - v2 chunk 失败现在与 v1 一样通知接收端 `status=failed`；接收端只更新终态并保留已完成 parts，fake receiver 聚焦测试由红转绿。
  - finalize 重试会校验并认领已经落盘且长度/SHA-256 匹配的预期最终文件，不再在 crash window 后生成 `(1)` 副本。
  - Rust library 40/40、desktop library 与 headless Web 编译、相关 diff whitespace 检查全部通过。
  - 读取用户三张反馈图：表情只有 21 个且选择后面板不关闭；截图工具栏需要改为截图选区下方悬浮；设置默认名称应取 macOS 机器名。
  - 对正在运行的 `127.0.0.1:8888/api/workspace` 连续轮询 20 次，每次间隔 0.6 秒，成功复现同一设备 MAC 在 `82:ae:17:28:c4:04` 与 `ac:de:48:00:11:22` 间切换，内存在 2356–2367 MB 间周期变化。
  - 现有回归只覆盖“旧心跳的 0 MB 不抹掉已知值”，没有覆盖两个非零详细发现来源用同一 device ID 相互覆盖。
  - 联网核对微信桌面截图资料与用户提供的新截图：工具栏应贴在选区下方，绘制工具在左、历史操作居中、取消/钉图/保存/完成在右，并按当前工具展开颜色与粗细。
  - 代码图确认表情面板的插入和光标恢复逻辑可复用，只缺扩展集合与选择后关闭。
  - 代码图确认截图工具栏是全宽 footer；Canvas、导出、钉图与图片草稿链路可原样复用，本轮应集中改 JSX 控件层和 CSS 定位。
  - 默认名称根因定位到数据库首次初始化时的 `generate_random_name()`；系统 hostname 已可复用，历史数据需只迁移明确匹配旧自动生成规则的名称。
- 待执行：
  - 联网研究微信截图工具栏，提出最小可实施设计并取得用户确认。
  - 设计确认后补充红灯单测、修复、视觉 QA 与构建验证。
  - 新增文件反馈：桌面外部拖放需进入附件草稿；发送卡需展示真实进度与速度；接收完成后的“打开”需提供打开文件/打开目录两项。
  - 并行传输需先审计现有 4 MiB 分块、接收端 offset 校验、取消与重试协议，避免只在发送端增加线程导致顺序写入损坏。
  - 初步协议审计确认当前接收端按文件加锁并顺序 append；单文件并行必须同步升级随机 offset 写入与完成位图，进度/速度本身可直接复用现有 transfer 字段在前端差分计算。
  - 截图设计审计形成三案：推荐保留系统选区，把独立编辑窗改成图片下方微信式悬浮胶囊；若要求原屏可调选区，则需要全屏 overlay 重写。
  - 用户确认“多线程”明确指单个大文件拆分为最多 4 个并行分块，因此不采用仅多文件并发的兼容简化方案。
  - 深入诊断确认 MAC 翻转来自 reply 与权威 announcement 交替覆盖；内存变化来自每 2 秒刷新 available memory。修复需要 PeerManager 与 SQLite 同步采用来源优先/首个正值规则。
  - 文件链路审计确认 Finder 拖入需要 Tauri Webview drag-drop 事件；打开文件和 Finder 定位的安全后端命令已经存在。
  - 复核 Tauri 2 官方 `onDragDropEvent` 文档：事件覆盖 over/drop/cancel，drop 载荷包含 paths，listener 必须在组件卸载时解除。

### 阶段 7：0.1.0 稳定性与品牌修复
- **状态：** complete
- 执行的操作：
  - 用户确认 `docs/plans/2026-07-29-xchat-0.1.0-desktop-completion-design.md`，实现门槛解除。
  - 将截图首版范围固定为矩形、椭圆、箭头、画笔、马赛克、文本和回退。
  - 只读复现并定位 MAC/内存闪动、会话信息默认展开和截图临时路径失效根因。
  - 为本轮新增阶段 7–11，并划分稳定性、React 表面、截图媒体和集成验证批次。
  - 并行完成设备元数据稳定、`0` 内存保护和 macOS `RunEvent::Reopen` 根因修复；子任务的 25 项 Rust 测试及 desktop/web 编译均通过。
  - 将 Tauri 产品名、版本、identifier、默认配置/数据库目录和 Android package/JNI 标识切换为 Xchat 0.1.0 / `com.xchat.app`。
  - `imagegen` 去白底结果因像素对比显示重绘了蓝色区域而被弃用；改为在原图中央方形裁切上执行边缘连通近白背景去除，保留纸鹤和蓝色图形原始像素。
  - 从透明母版重新生成 Tauri Windows/macOS/iOS/Android 图标，抽查 PNG 四角 alpha 均为 0。
  - 通过进程级 `OnceLock` 固定 hostname/MAC，并在 PeerManager 与 SQLite 更新处拒绝用未知的 `0 MB` 覆盖已知内存。
  - 将主窗口显示逻辑集中为 show/unminimize/focus，单实例、托盘与 macOS Dock 重开统一复用。
- 创建/修改的文件：
  - `docs/plans/2026-07-29-xchat-0.1.0-desktop-completion-design.md`
  - `task_plan.md`
  - `findings.md`
  - `progress.md`

### 阶段 8–10：React 交互、截图媒体与文件中心
- **状态：** complete
- 执行的操作：
  - 恢复 Unicode 表情面板、光标插入、Esc/点外关闭。
  - 建立按会话隔离的附件草稿，支持选择、粘贴、拖放图片预览，并在一次发送动作中依次发送文字和附件，失败项留在草稿。
  - 新增独立截图编辑窗口，Canvas 原生实现矩形、椭圆、箭头、画笔、马赛克、文本和回退；完成后回填输入区，钉图使用单例置顶窗口。
  - 新增受管 outbox、按消息 ID 的可信图片读取和草稿清理边界，消息气泡直接渲染本地图片，失效路径回退为不可用文件卡片。
  - 会话详情默认收起，全部空状态按剩余可用空间垂直居中；设置导航增加卡片式选中态与滚动同步。
  - 文件中心按最新原型恢复来源导航、类型筛选、来源头像、表格列、预览、双击和安全动作。
- 验证：
  - `rtk npm test`：7/7 通过。
  - `rtk npm run build`：Vite 8.1.5 构建成功。
  - 截图媒体后端合入后 `rtk cargo test --lib`：27/27 通过；desktop lib/bin 和 web bin 编译通过。

### 阶段 11：集成验证与交付
- **状态：** complete
- 执行的操作：
  - 刷新代码知识图并复核截图命令、Tauri 事件、附件草稿、文件可用性和 Dock 重开调用链。
  - `npm test` 7/7、React production build、Rust 27/27、desktop lib/bin 与 web bin 编译全部重新通过。
  - capability/Tauri JSON 与 `git diff --check` 通过；前端和 Tauri 关键 PNG 四角 alpha 均为 0。
  - 使用两个隔离 Web 实例完成真实发现与消息联调：MAC/内存在三个 5.5 秒刷新周期内保持一致；会话信息默认关闭；表情插入和发送成功。
  - 通过浏览器剪贴板粘贴项目 PNG，确认图片草稿缩略图和文字可同时存在、一次发送会落成相邻消息；接收端聊天和文件中心均直接显示图片，文件预览无路径错误。
  - 空会话、空文件中心在剩余区域垂直居中；设置导航选中态和点击滚动同步；768px 窄屏隐藏列表栏；截图编辑器七种工具和操作栏均可见。
  - 浏览器联调发现 Web 发送端在 staging 清理后丢失历史内联预览；现已把新上传持久化到下载目录内的受管 `.xchat-outbox`，并只允许数据库消息 ID 解析到该目录的 outgoing 文件读取，任意发送源文件仍返回 403。
  - 截图绘制抽成纯 Canvas seam，矩形、椭圆、箭头、画笔、马赛克、文本均有可运行测试；统一同步提交文本，修复“完成/钉图”时最后一段文字丢失。
  - 使用隔离端口 `18890` 和临时数据库启动 Tauri desktop，Vite、桌面进程、UDP、HTTP/WebSocket 均正常；macOS 自动化工具无法绑定未打包 debug binary，因此 Dock 点击仍以调用链审查覆盖。
  - 测试接收产生的 `/Users/eason/Downloads/Xchat/clipboard.png` 与项目 logo 哈希一致，已移回本轮隔离临时目录，未在用户下载目录留下测试文件。
  - 最终 `npm test` 9/9、Rust library 28/28、React production build、desktop lib/bin 与 web bin 编译全部通过。
  - 实际生成调试版 macOS bundle：`src-tauri/target/debug/bundle/macos/Xchat.app`，Info.plist 产品名与版本为 Xchat 0.1.0。

### 阶段 1：需求与发现
- **状态：** complete
- 执行的操作：
  - 确认真正仓库和最新原型路径。
  - 审计设计资源、现有前端、Tauri/Web 运行链路和 Rust 后端能力。
  - 比较深 Workspace、分模块 service、通用 Query/Command/Event 三种方案。
  - 用户批准方案 A，并确认所有缺失能力纳入本轮。
- 创建/修改的文件：
  - 无产品代码修改。

### 阶段 2：设计固化与实施规划
- **状态：** complete
- 执行的操作：
  - 初始化持久化计划、发现和进度文件。
  - 将已批准设计写入 `docs/plans/2026-07-29-xchat-react-redesign-design.md`。
  - 运行占位符扫描与 `git diff --check`；没有发现占位符或空白错误。
  - 自审修复了五项：拆分可验证里程碑、稳定 direct ID、逐消息 delivery/read ack、平台能力矩阵，以及传输/头像/完整历史搜索契约。
  - 复审确认设计不再依赖设备时钟或本地消息行 ID，且不支持的平台不会显示无效动作。
  - 设计文档已通过提交 `97a87e4` 单独提交；未包含工作区内任何用户改动。
  - 用户已确认书面规格，实施门槛解除。
- 创建/修改的文件：
  - `task_plan.md`
  - `findings.md`
  - `progress.md`
  - `docs/plans/2026-07-29-xchat-react-redesign-design.md`

### 阶段 3：React 前端与运行链路
- **状态：** complete
- 执行的操作：
  - 将实施拆成 React、SQLite、网络协议、双 adapter 集成和验证五条可独立检查的工作流。
  - 为并行实现划分互不重叠的文件所有权。
  - 刷新代码图，确认 Tauri 双注册、Axum 单文件 router、旧消息发送与 WebSocket/TCP 回退链路。
  - 并行启动 React、SQLite 和 network protocol 三个互不覆盖的实现任务。
  - 通过 React/Vite 官方发布信息确认稳定版本，并验证本机 Node/npm 版本。
  - 更新 Tauri 为 Vite dev server 运行链路，并将默认窗口调整为最新桌面设计需要的尺寸与 CSP。
  - 网络子任务已落地兼容协议：群同步/群消息、送达/已读 ack、稳定 direct 消息 ID、可取消传输注册表，以及兼容旧帧的设备元数据发现；其 6 项网络测试和 desktop/web 编译均通过。
  - SQLite 子任务已完成兼容迁移、带版本的群快照、幂等消息、逐成员回执及可靠 ack outbox、文件/传输、设备元数据和 settings seam；DB 测试 2/2，desktop/web 编译通过。
  - `workspace.rs` 已集中实现 snapshot、群创建/同步、会话消息、receipt 聚合/补送、文件中心、安全本地副本删除和设备备注，Tauri 与 Web 薄入口正在并行接入。
  - Discovery 现在会持久化 hostname/MAC/source，并在设备重连时同时补发旧 pending 队列、新群快照、稳定消息和 receipt outbox；历史设备加载会恢复已存备注与元数据。
  - React/Vite 已正式构建到 `src/`，完全替换旧静态界面；兼容的 `css/vscode.css` 随 public 资源保留。
  - WebSocket 入站已处理带版本群同步、群消息、稳定 direct、delivery/read ack，并保留旧 Text/File/流式协议；重复消息集成测试与 desktop/web 编译通过。
  - Tauri 已注册 workspace、会话、回执、搜索、文件中心、传输取消、设备备注、安全删除和截图 commands，并同步更新 desktop/mobile permissions；仅会话文件 command 等待共享分块核心接入。
  - 文件发送改为单一共享核心设计：一条逻辑文件消息、每成员一条 transfer、4 MiB 可取消分块和重连恢复；Tauri/Web 只负责提供可信本地或 staging 路径。
  - Axum 已接入除会话文件 multipart 之外的全部新路由；workspace capability 会按浏览器 transport 覆写，download/media 只能读取数据库定位且位于下载目录内的接收文件，发送源文件返回 403。
  - 共享会话文件核心已完成：单逻辑消息、每成员 transfer、离线恢复、群版本同步、4 MiB 分块、真实进度、双端取消和文件状态聚合；network tests 7 项及 desktop/web 编译通过。
  - Web multipart 已使用受控 UUID staging，稳定接收分支按会话/成员校验、顺序写入 `.downloading`、完成后原子改名并回 delivery ack；接收端取消只按稳定消息 ID 清理可信下载目录中的临时文件。
  - 共享发送核心会在全部子传输完成、失败或取消后清理受管截图/Web staging 源文件；离线等待传输被取消时也会同步刷新逻辑文件状态。
  - 隔离 Web 服务在 `18888` 端口启动成功，首页、workspace、文件、传输、搜索和输入校验接口均返回预期结果。
  - 浏览器实际操作通过聊天、主机、文件、设置、群聊弹窗、设置保存/刷新持久化及 375/768/1280 三档布局；控制台无错误，资源/API 请求无失败。
  - Tauri 桌面端使用隔离数据库和 `18889` 端口成功启动，React dev server、桌面二进制、UDP 发现及 HTTP/WebSocket 服务均正常。
  - README 中的开发命令、React/Vite 工程结构和技术栈已同步到当前实现。
- 创建/修改的文件：
  - `src-tauri/src/network/protocol.rs`
  - `src-tauri/src/network/transfer.rs`
  - `src-tauri/src/network/discovery.rs`
  - `src-tauri/src/network/messaging.rs`
  - `src-tauri/src/network/mod.rs`
  - `src-tauri/src/peers.rs`
  - `src-tauri/src/models.rs`
  - `src-tauri/src/workspace.rs`
  - `src-tauri/tauri.conf.json`

## 测试结果
| 测试 | 输入 | 预期结果 | 实际结果 | 状态 |
|------|------|---------|---------|------|
| React completion logic | `npm test` | 全部通过 | 9/9 通过 | pass |
| shared core desktop compile | `cargo check --no-default-features --features desktop --lib` | 编译通过 | 编译通过 | pass |
| React logic | `npm test` | 全部通过 | 6/6 通过 | pass |
| React production build | `npm run build` | 产出到 `src/` | Vite 8.1.5 构建成功 | pass |
| SQLite focused tests | `cargo test --lib db::tests` | 迁移与文件元数据测试通过 | 2/2 通过 | pass |
| Full Rust library tests | `cargo test --lib` | 全部通过 | 28/28 通过 | pass |
| Desktop shared compile | `cargo check --no-default-features --features desktop --lib` | 编译通过 | 编译通过 | pass |
| Headless Web compile | `cargo check --no-default-features --features web --bin lanchat-web` | 编译通过 | 编译通过 | pass |
| Tauri/capability JSON | `jq empty ...` | JSON 有效 | 有效 | pass |
| Diff whitespace | `git diff --check` | 无错误 | 无错误 | pass |
| Isolated Web smoke | `lanchat-web --port 18888 --db-path /tmp/...` | 页面与新 API 可用 | 页面/API/设置持久化通过 | pass |
| Browser visual QA | desktop/tablet/mobile | 四模块和响应式布局可用 | 无控制台或网络错误 | pass |
| Tauri desktop smoke | `cargo tauri dev -- -- --port 18889 --db-path /tmp/...` | 桌面运行链路启动 | Vite、Tauri、发现与 HTTP 服务启动 | pass |
| File boundary smoke | legacy upload / traversal / forged address | 保留兼容且拒绝越界与 SSRF | 合法分块成功，穿越 400，伪造地址不外连 | pass |
| Final Web dual-instance QA | 18888 ↔ 18889 | 发现、聊天、图文、文件中心运行 | 接收端内联与文件预览通过；发送端持久预览由真实 handler 回归覆盖 | pass |
| Device metadata stability | 三次、每次间隔 5.5 秒读取主机详情 | MAC/内存不交替 | 两项均完全一致 | pass |
| Final Tauri startup | `cargo tauri dev -- -- --port 18890 --db-path /tmp/...` | 原生链路启动 | Vite、binary、UDP、HTTP/WS 正常 | pass |
| macOS app bundle | `cargo tauri build --debug --bundles app` | 生成可打开的 `.app` | `Xchat.app` 生成，版本 0.1.0 | pass |

## 错误日志
| 时间戳 | 错误 | 尝试次数 | 解决方案 |
|--------|------|---------|---------|
| 2026-07-29 | Tauri CLI 将单层 `--` 后参数传给了 `cargo run` 自身 | 1 | 使用双层分隔：`cargo tauri dev -- -- --port ...` |
| 2026-07-29 | 删除旧截图兼容入口的首个补丁未匹配 mobile capability 尾部 | 1 | 读取实际 JSON 后使用精确上下文，第二次成功 |
| 2026-07-29 | Chrome 扩展未开放本地文件 URL，file chooser QA 返回 `Not allowed` | 1 | 使用浏览器会话剪贴板粘贴 PNG，草稿与发送验证成功 |
| 2026-07-29 | Computer Use 无法识别 raw Tauri debug binary | 2 | 不继续切换自动化技术；以原生启动日志和双实例浏览器 QA 验证 |
| 2026-07-30 | 将 `permissions/commands.toml` 误传给 `jq` | 1 | JSON 与 TOML 分开验证；TOML 交由 Cargo metadata/check 解析 |

## 五问重启检查
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 12：反馈实现与验证完成，准备提交 |
| 我要去哪里？ | 提交本轮实现并交付运行、构建和验证命令 |
| 目标是什么？ | 用最新设计彻底替换旧 UI，并真实实现四模块及缺失协议能力 |
| 我学到了什么？ | 见 `findings.md` |
| 我做了什么？ | 见上方记录 |

### 阶段 12：测试反馈二次收敛
- **状态：** complete
- 执行的操作：
  - 通过真实运行实例和 SQLite 复现 MAC 与内存周期跳动，确认根因在 discovery → PeerManager → DB 的元数据覆盖规则。
  - 审计默认名称、表情、截图、Finder 拖放、文件进度、文件打开和现有顺序分块协议。
  - 研究微信式截图悬浮工具栏与 Tauri 原生拖放事件。
  - 用户确认方案 A 和“单个大文件拆成 4 个分块并行传输”。
  - 写入并自审 `docs/plans/2026-07-29-xchat-feedback-round-2-design.md`。
  - 独立审查后修正分块语义、SHA-256/manifest 恢复校验、拖放 `leave` 事件、取消/finalize 互斥和截图失败恢复。
  - 设计文档已通过提交 `ca41d08` 单独提交；未包含用户已暂存内容。
  - 修复 reply/announcement 元数据覆盖规则，MAC、内存和能力降级保持稳定；新设备名优先读取本机名称。
  - 表情扩充到 108 个并在选择后关闭；DOM/Tauri 外部拖放统一进入附件草稿。
  - 文件卡和全局传输条显示真实字节、百分比与速度；完成文件提供打开和 Finder 定位菜单。
  - 截图编辑改为全屏选区和跟随选区的微信式悬浮工具栏，覆盖矩形、椭圆、箭头、画笔、马赛克、文本、撤销/重做、保存、钉图和完成。
  - 大文件 v2 固定四分块并发，保留 v1 回退、SHA-256 校验、断点续传、失败回滚、远端失败同步和 finalize 幂等恢复。
  - 设置身份页新增只读本机 IP/MAC；IP 优先局域网组播路由，避免被 Clash TUN 默认路由替换。
  - 双实例完成 128 MiB 实传，收发均 100%，接收文件 SHA-256 与源文件一致。
  - 最终 `npm test` 15/15、Rust tests 40/40、desktop/web 编译、Vite build、debug `.app` 打包和 `git diff --check` 全部通过。
  - macOS 窗口关闭后重新激活可恢复；截图真实捕获受当前测试环境“屏幕录制”权限限制，失败时现会给出明确授权提示并恢复主窗口。
- 创建/修改的文件：
  - `docs/plans/2026-07-29-xchat-feedback-round-2-design.md`
  - `docs/plans/2026-07-29-xchat-feedback-round-2-implementation.md`
  - `task_plan.md`
  - `progress.md`

### 阶段 13：截图钉图、通知与文本二次编辑
- **状态：** complete
- 已执行：
  - 恢复并完整阅读 `task_plan.md`、`findings.md`、`progress.md`。
  - 确认代码知识图 `XChat` 已存在并覆盖当前 React/Tauri 架构。
  - 记录文件规划技能缺失 `scripts/catchup.py`；改用手工五问恢复上下文。
  - 用户确认完整合入 `9c27425`，并批准文本工具方案 A 与通知整条旁枝。
  - 红灯 `rtk npm test` 同时捕获钉图 helper、文本编辑/移动 helper 与设置透明选中态缺失。
  - 已完整 cherry-pick `9c27425`；保留 `45d5f7c` 的首次文本输入与 Finder 拖放修复。
  - 文本操作增加稳定 ID、顶层命中、边界内移动和快照式历史；点击旧文本编辑、拖动超过 4px 移动，均可一次撤销/重做。
  - 设置选中态移除背景，仅保留绿色文字与字重；静态 CSS 回归通过。
  - 审计并修复后台可见但失焦时通知刚启动即被自动已读清除的竞态，统一采用 `visible && hasFocus()`。
  - 受控 Web 截图编辑器实测首次聚焦、二次编辑、撤销/重做和拖动；钉图右键菜单 10 项完整且边缘位置受限。
  - `rtk npm test` 23/23、Vite 生产构建、Rust library 41/41、desktop lib 与 web bin 编译、JSON 与 whitespace 检查均通过。
  - Tauri dev 完成 Vite 与 Rust 构建并运行到二进制；由于已安装 Xchat 正在运行，单实例插件将调试启动交给旧进程后退出，未擅自终止用户应用。

### 阶段 14：截图交互反馈修复
- **状态：** complete
- 已执行：
  - 读取三项用户反馈并对照当前截图编辑器、钉图视图与历史模型。
  - 浏览器真实指针回放确认：马赛克拖动时 Canvas 哈希从 `3844884312` 变为 `908550699`，松手后恢复 `3844884312`，而撤销按钮已启用，证明操作已入 history 但被预览过滤。
  - 建立 `isCaptureOperationHidden` 与 `removeCaptureOperation` 红灯测试；`rtk node --test frontend/src/capture-drawing.test.js` 因两个导出尚不存在而按预期失败。
  - 并行审计确认文本编辑框截断拖动事件，空文本沿用旧“取消”语义；钉图小工具条与截图工具条没有共享编辑状态。
  - 用显式非空 ID 判断替代 `undefined === undefined`，马赛克及其他无 ID 标注松手后继续渲染。
  - 编辑态文本增加独立拖动手柄，拖动实时更新但松手只写入一次历史；清空旧文本变为可撤销删除。
  - 删除独立钉图小工具条；右键“显示工具条”挂载同一个截图 `CaptureOverlay`，取消不改图，完成/保存通过现有 `capture.pin` 原位更新。
  - Rust 钉图命令支持已有 `capture-pin` 会话原位替换；失败保留旧状态和文件，成功后再清理旧图，且不关闭钉图窗口或恢复主窗口。
  - 浏览器回放通过：马赛克松手后哈希保持变化；文本移动、清空、撤销恢复通过；钉图复用 12 项原工具按钮，取消返回且 pending 不变。
  - `rtk npm test` 25/25、`rtk npm run build`、Rust library 42/42、desktop lib、web bin 与 `rtk git diff --check` 全部通过。
  - Tauri dev 已构建运行到二进制；已安装 Xchat 的单实例锁接管启动，因此未擅自终止用户正在运行的应用。
- 遇到的错误：
  - `browse eval/js` 执行异步复现脚本时无结果输出；没有重复同一路径，改为分步哈希采样和 Node 红灯测试。
  - 浏览器 Web mock 的 `capture.pin` 只会新开图片窗口，不能验证原位覆盖；真实路径由 Rust 状态测试覆盖。
