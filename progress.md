# 进度日志

## 会话：2026-08-20 最大并行通道完整实现

### 阶段 18：范围确认与实施规划
- **状态：** in_progress
- 执行的操作：
  - 对照用户截图、原型、设计文档、生产 React 设置页、Rust 设置接口和 Git 历史完成诊断。
  - 确认截图内容来自已批准原型；生产端从未接入对应字段，当前并行 v2 固定使用 4 个范围。
  - 确认本轮必须覆盖真实设置持久化、Tauri/Web 双入口、新传输调度、旧端回退与生产 UI，不能只显示下拉框。
  - 用户明确要求开始完整实现；继续沿用当前仓库建分支、不建 worktree 的工作方式。
  - 已创建 `agent/parallel-transfer-channels` 分支。
  - 代码图确认设置快照和 SQLite KV 可直接扩展；发现 capability 可承载新协议协商。
  - 确认当前每个文件独立并发四个大范围且没有全局限流；拟采用共享限流代际与小范围 worker 队列，同时保持旧 v2 固定四范围回退。
  - 审计首次发送、离线恢复和显式重试，确认三条路径都必须从布尔 `parallel_v2` 升级为同一协商传输计划。
  - 审计接收 manifest，确认 v2 的 prepare 与恢复都锁死四范围；可调并发需要显式 v3 manifest 校验，不能静默改变 v2 语义。
  - 建立干净基线：`rtk npm test` 98/98 通过；`rtk cargo test --manifest-path src-tauri/Cargo.toml --lib` 125/125 通过。
  - 审计设置双入口和前端 adapter，确认字段可无迁移接入现有 KV，并由现有 Tauri/Web 对等测试覆盖。
  - 确认接收端稳定状态机可复用任意合法 manifest；决定新增显式 v3 路由/capability，完整保留 v2 固定四范围契约。
  - 找到现有真实 fake receiver 与 prepare handler 测试 seam，可用端到端峰值/交错记录验证全局并发与公平性。
  - 已写入 `docs/superpowers/plans/2026-08-20-xchat-configurable-transfer-channels.md`，把实现拆为设置持久化、代际限流、双 API、v3 协议、全局调度、前端和最终验证七个测试先行阶段。
  - RED：新增设置/限流测试首次编译出现 14 个预期缺失符号错误，确认测试确实约束未实现行为。
  - GREEN：实现设置键、默认值、4/8/16 校验、损坏值回退，以及共享 `Semaphore` 的进程级代际限流器；`rtk cargo test --manifest-path src-tauri/Cargo.toml --lib max_parallel_channels` 5/5 通过。
  - 限流许可等待保留 Tokio semaphore 的原始排队位置，并每 25ms 检查取消 token；取消会丢弃等待 future，不占用或泄漏 permit。
  - RED：Workspace/Web 新测试首次得到缺失 `max_parallel_channels` 字段的预期编译错误；同时修正测试中 opaque `IntoResponse` 必须先显式转换的调用方式。
  - GREEN：Workspace 快照、旧版 `get_settings` Tauri JSON、Tauri `update_settings`、HTTP get/update 已全部接入同一验证/持久化 helper；非法值在任何其他字段写入前返回。
  - `rtk cargo test --manifest-path src-tauri/Cargo.toml --lib parallel_channels` 8/8 通过，覆盖默认快照、HTTP 解析、有效保存和非法值不覆盖。
  - RED：v3 协商/分块/manifest 测试首次出现 34 个预期缺失符号错误；随后逐步补齐协议模型并用编译错误找出全部旧布尔调用点。
  - GREEN：新增 `parallel_file_v3:16` capability、显式 v1/v2/v3 `UploadPlan`、有界公平分块生成器、严格覆盖校验和 v3 prepare/chunk 路由；v2 固定四范围与原路由保持不变。
  - 初次发送、等待上线恢复、显式恢复和失败重试现统一读取当前本地设置并对 peer capabilities 运行同一协商函数；并行发送按协商 channel 数限制单文件 worker 数。
  - 新增 Web handler 测试确认 v3 manifest 落盘为 version 3，v2 prepare/chunk 路由拒绝该布局；`rtk cargo test --manifest-path src-tauri/Cargo.toml --lib` 全量 138/138 通过。
  - RED：双 v3 fake receiver 测试先因 `UploadJob` 尚未捕获 limiter 代际而编译失败；在途取消测试随后稳定复现请求会等待 5 秒服务端响应、1 秒内无法退出。
  - GREEN：每个新 job 捕获当前设置对应的 limiter 代际；v1 每个 multipart chunk、v2/v3 每个 range 都在真实 HTTP 请求期间持有同一全局 permit。
  - v3 双传输实测峰值不超过 4，且第二个 transfer ID 在第一个文件的全部范围启动前已出现；v1 与 v2 在许可池耗尽时均无数据请求越过门禁。
  - 对 prepare、分块 send 和响应体读取统一增加 25ms 原子取消轮询；在途取消现于 1 秒断言内返回并允许替代请求立即取得释放的 permit。
  - 4/8/16 三档均新增“持满后额外 try_acquire 失败、释放后恢复完整许可数”测试；最终 Rust 全量回归 142/142 通过，三档设置定向测试 9/9 通过。
  - RED：前端归一化、脏状态/保存/reset 和双 adapter 参数测试先因缺失导出而失败；生产设置控件静态契约测试先因缺失原型文案与字段绑定而失败。
  - GREEN：设置归一化仅接受数字 4/8/16，旧/非法快照回退 4；共享 patch helper 保证新字段参与保存和重载，Tauri 发送 `maxParallelChannels`、HTTP 发送 `max_parallel_channels`。
  - 已在“自动接收文件”正下方接入批准的 4/8/16 下拉框、中英文文案和随选项变化的资源提示；`rtk npm test` 101/101 通过，`rtk npm run build` 成功刷新生产资源。
  - desktop library 与 headless Web feature 均通过 `cargo check`；Rust library 全量回归最终为 144/144，前端全量回归为 101/101。
  - 浏览器生产 UI 已验证默认 4、保存/重载 8 与 16、重置回 4、窄桌面宽度无横向溢出且控制项与批准原型一致。
  - 独立 Tauri WebView 实例已验证 8 的保存与进程重启持久化、16 的动态提示及重置回 4；未替换或关闭 `/Applications/Xchat.app` 中用户原有实例。
  - 固定端点双实例真实传输已验证 Web→Tauri v3/8，以及双 Web v3c16 的 39,283,312 字节文件；后者发送/接收 SHA-256 均为 `9b1b377d0db55de24fae63f0bdc9b18c38b427e77e31e1039844cef350911e5a`。
  - 最终代码审查发现并修复 manifest 布局复用边界：v3 ID 编码通道布局，恢复保持原 v2/v3 计划，设置/能力导致布局变化时使用新 retry ID，避免接收端永久返回 manifest conflict。
  - v3 双上传 fake receiver 回归现逐档执行 4/8/16，均验证全局峰值不超限且第二个文件在第一个耗尽工作队列前取得通道；旧 v1/v2 回退、恢复和取消继续由确定性集成测试覆盖。
  - 本机没有可供运行的真实旧二进制和第二台跨网段真机；因此未把同机验证表述为完整真机矩阵。真实测试可在两端安装本次新版本以验证 8/16，混合旧版时预期固定回退 4。
- 下一步：
  - 提交最终兼容修复与验证记录，合并到本地 `main`，并在合并结果上复跑全量测试。

## 会话：2026-08-19 Windows A0/A1 收敛

### 阶段 16：范围确认与短设计
- **状态：** in_progress
- 执行的操作：
  - 用户确认 Windows 必须优化、Android 暂缓、无需旧版兼容。
  - 通过代码知识图确认 Windows listener 目前没有 ingress index，发送路径会过滤 75 秒后离线的普通已知 peer。
  - 确认旧版 128 台保障来自独立兼容心跳调度器，可整体删除。
  - 核对 Microsoft Winsock 官方文档，确认 `IP_PKTINFO + WSARecvMsg/WSASendMsg` 能提供严格 ingress index 与原接口/源地址回复。
  - 核对 Cargo 依赖，Windows 尚无直接 Win32 API 依赖，后续采用 target-only `windows-sys`。
  - 用户指出历史 IP 可能被 DHCP 分配给其他设备；确认保留离线硬发送门禁，不实现历史地址试发，并准备修正原型/生产文案。
  - 用户要求进一步简化关闭发现文案，并为手工添加设备增加测试功能。
  - 核对现状：手工添加目前只保存地址字符串；推荐改为真实 XChat 握手，展示并绑定返回的设备身份。
  - 开始只读追踪设备身份、会话 ID、最新地址和接收校验；一次组合图查询因局部变量覆盖输出 helper 失败，已改名后继续。
- 下一步：
  - 核对 Windows 官方 packet-info API 与当前依赖，提交短设计供用户确认。

## 会话：2026-08-19 网络与消息可靠性优化

### 阶段 15：设计、原型与工程现状审计
- **状态：** in_progress
- 执行的操作：
  - 完整恢复并读取现有 `task_plan.md`、`findings.md`、`progress.md`。
  - 阅读网络发现、Presence、消息可靠性设计与最新原型的网络设置、连接横幅和消息状态交互。
  - 确认代码知识图 `XChat` 可用，当前约 1,980 个节点、7,376 条边。
  - 记录工作树保护边界：`main` 领先远端 1 个提交，`src/index.html` 存在用户修改。
  - 通过代码图定位发现流量入口、PeerManager 多状态写入入口和可复用的补发/回执基础。
  - 发现知识图源码路径仍指向旧工作目录，决定刷新当前仓库索引后继续审计。
  - 使用当前路径重建中等深度代码图（2,280 节点、8,864 条边）并取得精确源码。
  - 确认现有测试固化了 256 地址兜底，且接口模型缺少前缀、稳定 ID、分类和选择状态。
  - 确认本机 IP 通过默认路由探针推断而非真实接口枚举，单次广播入口同样复用 260 目标列表。
  - 调用链确认发现循环覆盖桌面、移动库与 Web，发送失败又从三条消息路径直接修改离线状态。
  - 审计设置 seam 与依赖，确认结构化接口配置需要共享模型，当前无可直接复用的接口枚举依赖。
  - 使用本地无网络浏览器实际渲染并检查设置页和完整网络卡片；无控制台错误。
  - 实际选择离线设备并发送消息，核对连接横幅、文件暂停和“等待对方上线后发送”状态的一致表现。
  - 审计生产前端的消息单调合并、状态文案、事件处理与递归轮询，确认可复用边界。
  - 依据官方 Rust 文档比较接口枚举依赖，暂定 `getifaddrs` 为桌面阶段 A 的最小推荐。
  - 确认通用 settings KV 足以保存发现配置，无需数据库 schema 迁移。
  - 只读检查 `src/index.html` 用户 diff，确认是换行格式变化，并将其列为前端构建保护边界。
- 下一步：
  - 向用户提交阶段 A 工程切片与依赖选型，等待明确确认后进入测试优先实现。
- 创建/修改的文件：
  - `task_plan.md`
  - `findings.md`
  - `progress.md`

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

### 阶段 15：A0/A1 网络发现优化
- **状态：** complete
- 用户已批准在当前 `main` 分支按 A0 → A1 顺序实施；不创建 worktree，不提交或覆盖现有用户改动。
- 已加载 `writing-plans`、`executing-plans` 与 TDD 流程，并完整阅读测试质量约束。
- 已用代码知识图定位桌面/Web 监听器、公告循环、Tauri/Web 设置入口、前端设置归一化与 `SettingsWorkspace`。
- 已确认 A0 不升级 discovery wire protocol；先用短 TTL 帧去重止住重复 reply，再把发送目标收敛为用户允许的真实接口与固定地址。
- 下一步：写入可执行实施计划，然后逐项执行红灯、最小实现与回归。
- 已写入并自审 `docs/superpowers/plans/2026-08-19-xchat-discovery-traffic-and-interface-settings.md`。
- A0 policy 红灯产生 40 个缺失符号错误，证明测试先于实现；补入最小策略后 6 项通过。
- netmask 连续性测试先因 `prefix_length` 缺失失败；实现真实 `getifaddrs` adapter 与 Android 窄 fallback 后，policy 7/7 通过。
- cadence/dedupe/backoff 测试先出现 6 个缺失符号错误；最小状态机实现后 discovery 聚焦测试 13/13 通过。
- metrics 测试先因窗口类型缺失失败；实现每接口/目标类型计数、接收/去重/reply 与排除原因报告后通过。
- 已删除 256 地址目标生成器、未绑定 limited-broadcast fallback、默认路由 IP 探针与 2 秒公告循环；单次发送也复用同一 policy 计划。
- A0 回归：Rust lib 87/87；desktop lib 编译通过（移除唯一 dead-code warning）；web bin 编译通过。
- A1 设置红灯因缺失 key/load/save 共出现 10 个编译错误；实现版本化 KV、校验和损坏回落后 3/3 通过。
- workspace、Tauri command 与 HTTP handler 已接入同一发现快照/保存 seam；Rust lib 90/90、desktop lib 与 web bin 编译通过。
- 前端发现配置先以缺失导出形成红灯，再完成快照归一化、Tauri/Web payload 与共享选择 helper；聚焦测试 49/49、全量前端测试 92/92。
- 生产设置页已接入主类别开关、接口清单、代理 TUN 风险确认、恢复推荐、网络刷新与现有固定地址 modal；临时及正式 Vite bundle 均构建通过并更新 `src/` 生成资源。
- 独立首轮审查指出低频公告与旧版 10 秒离线阈值冲突、监听器仍接收禁用接口、socket 绑定与 DNS 退避等边界；已逐项修正并补回归测试。
- A0 兼容桥把离线阈值提高到 75 秒；reply 形态兼容心跳每批最多 32 台，并按在线规模从 3 秒自适应到 750ms，测试锁定 128 台全覆盖且丢一轮加 2 秒阻塞仍低于旧版 10 秒阈值，新客户端不会触发回复风暴。
- Unix listener 通过 `IP_PKTINFO`/`IP_RECVIF` 读取真实 ingress interface，动态加入/离开启用接口的组播成员；回复 socket 绑定批准的源 IP，Apple 平台同时绑定 interface index。
- 固定地址 DNS 改为并发、单项 2 秒超时、60 秒缓存与独立退避；网络/设置变化会清缓存并重置退避。socket 配置错误不再被忽略，缓存键包含稳定接口 ID 与源 IP。
- 稳定接口 ID 改为 `if:name:<system-name>`，不含易变 index 或当前 IP；开发期 index+name override 自动迁移，旧的 index-only override 安全丢弃。`/1` 不再生成 limited broadcast。
- listener 固定地址解析增加 16 个/轮轮转、独立退避和同网络 last-good IP，并在网络指纹变化时清空旧地址；公告端固定地址与旧版兼容心跳也轮转覆盖预算外设备，避免稳定排序导致长期饥饿。
- Android Activity 增加前台生命周期 `WifiManager.MulticastLock`，为 prefixless fallback 的组播收包补齐平台前提；Android target/真机仍需在具备 SDK/设备的环境验证。
- 接口枚举失败时 API 返回保留设置的空清单，运行时保留 last-good snapshot 或 fail closed；本机显示 IP 只从有效启用接口选择。
- 前端补齐 1–65535 端口校验、错误提示、恢复原值后清除 dirty、暂停接口计数和 TUN 确认的可访问语义。
- 最终验证：`rtk npm test` 93/93；`rtk cargo test --manifest-path src-tauri/Cargo.toml --lib` 105/105；desktop lib 与 web bin 编译无警告通过；生产 Vite bundle 构建通过；`rtk git diff --check` 通过。
- 隔离 Tauri 实例实测保存全关后发送计划立即从 2 个目标降到 0、workspace 本机地址 fail-closed，恢复设置后目标回到 2；另以真实 `en0` 源地址向 headless Web listener 注入 v2 公告，验证 ingress 接受、reply、设备元数据持久化与 workspace 输出。
- Android Kotlin 编译未执行：本机没有 Java Runtime/`kotlinc`，Rust Android target 也未安装；`MulticastLock` 仍需 Android 构建环境和真机验证。Windows target 仍受本机缺少 MSVC C 工具链阻塞。
- 隔离 Tauri 与 headless Web 均成功启动：en0 生成 2 个接口目标，utun4 默认排除，发送预算为接口 48 + 固定地址 16；运行中关闭/恢复本地发现分别立即切换为 0/2 个接口目标。
- 浏览器在 1440×900 与 720×900 验证网络设置、TUN 风险确认、全关 warning、固定地址 modal、端口错误、保存/恢复和无横向溢出；控制台无错误。
- Windows web target 交叉检查在 `libsqlite3-sys` C 构建前置阶段因本机缺少 MSVC C 工具链/`stdlib.h` 失败；Android target 未安装。本机 macOS desktop/web 均已覆盖。
- Native Computer Use 调试桥无法连接未打包 Tauri 窗口；使用真实 Tauri 启动日志、HTTP 快照和同一生产 bundle 的浏览器交互完成替代验证。

### 阶段 16：Windows ingress、设备身份与动态 IP 安全收敛
- **状态：** complete
- 用户最终确认离线状态统一为“等待对方上线”；Windows 纳入本轮，Android 暂缓，不保留旧版协议兼容。
- 删除 128 台预算相关的旧版兼容心跳、deadline、metrics 与测试；75 秒只保留为 Presence 过期阈值，显示离线后消息/文件只落本地 pending，不尝试旧地址。
- Windows UDP listener 开启 `IP_PKTINFO`，通过 `WSAIoctl` 获取并缓存 `WSARecvMsg`，解析真实 ingress interface index 后复用严格接口过滤与回复源地址选择。
- Windows MSVC target 首次真实编译捕获并修复 `RawSocket(u64)`/WinSock `SOCKET(usize)` 和 std/Tokio UDP socket 泛型边界；最终 `x86_64-pc-windows-msvc` desktop lib 检查通过。
- peer WebSocket 在 upgrade 前校验 `target_id`，握手响应返回本机 UUID；客户端确认响应 UUID 后才写正文，缺失/不符/超时均停止，旧 TCP fallback 已删除。消息、控制帧、群组、重发和文件发送链路均传递预期设备 ID。
- 固定地址改为结构化记录；旧裸地址保留为“需重新测试”但不参与自动发现。新增只读身份 endpoint、Tauri/Web“测试连接”、后端保存前二次探测，以及固定来源 IP 对已绑定 UUID 的 announcement 门禁。
- 正式 React UI 已实现测试/成功/不可达/身份不一致状态，只有当前输入的成功结果能保存；设备详情区分设备 ID、当前地址和辅助网卡地址，离线横幅明确不会发往旧地址。
- 前端 `rtk npm test` 96/96、Rust `cargo test --lib` 111/111、macOS desktop lib、Web bin、Windows desktop lib target、Vite production build 与 `git diff --check` 均通过。
- Tauri debug binary 成功构建并执行，但被本机既有 Xchat 单实例接管；隔离 Web 实例实测身份查询、连接测试、错误 UUID 保存返回 400、正确 UUID 保存并回读结构化记录，测试数据库随后删除。
- Windows 编译仅为交叉 target 类型检查，不等同 Windows 真机运行；Android 本轮未继续优化，保留此前 A0 的前台组播锁改动。
- 磁盘空间不足时只删除了本轮临时目录、Windows target debug 缓存和数个明确属于 XChat 的旧 Rust incremental 缓存；均为可再生构建产物，未删除源码、用户数据或应用数据库。

### 阶段 17：设备身份与动态 IP 风险诊断
- **状态：** complete
- 通过代码知识图核对用户 ID 生成、PeerManager 更新、发现落库、稳定单聊 ID、发送与接收校验链路。
- 结论：正常 DHCP 变更后会按持久 UUID 更新地址并沿用原会话，现有实现不是按 IP 识别用户。
- 确认一个窄但真实的错投窗口：75 秒在线租约内可能向旧 IP 写出正文；错误设备会因稳定会话 ID 不匹配而拒绝入库，但发送前无目标身份握手，无法阻止载荷先到达错误进程。
- 确认 MAC 只适合作为辅助网络证据，不适合作为唯一用户或设备主键；后续设计应以持久设备身份绑定短期地址，并让手工测试与发送前检查复用同一身份握手。
- 已更新 `ui-ref/xchat-desktop-prototype.html`：重写发现关闭与离线文案，增加设备 ID/辅助网卡地址/身份核验信息，并补齐手工地址测试、成功、不可达、身份不一致和保存状态。
- 浏览器实测 `192.168.10.111:8888` 核验成功后才启用保存；`10.8.0.13:8888` 身份不一致时明确阻断且保存保持禁用；离线会话新消息显示“仅保存在本机 · 尚未发送”。控制台无错误。
- 原型 5 段脚本语法检查通过，`rtk npm test` 93/93，`rtk git diff --check` 通过；等待用户审批原型后再改生产 UI/协议。
