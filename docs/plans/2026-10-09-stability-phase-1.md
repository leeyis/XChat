# XChat 稳定性阶段 1 实施与交接记录

## 授权与目标

用户于 2026-10-09 批准功能推荐文档并要求先实施阶段 1，允许 planning-with-files 管理记忆和交接。基线 HEAD `6298408786dad1e7a8277faef49c6f872ae84fee`，版本 0.1.13。开始时已跟踪文件干净，`analysis/` 与推荐文档为前序任务生成的未跟踪成果，必须保留。

目标：原子入队、文件启动恢复、共享网络运行时健康与就绪、当前 ACK 优先及有界历史补发，并以隔离数据库和端口验证故障后的状态收敛。不是阶段 2 性能重构或阶段 3 远程协助。

## 实施顺序

| 子项 | 状态 | 完成条件 |
| --- | --- | --- |
| S1 原子入队 | complete | 正文/目标/提及/文件任务同事务；失败回滚及重复 ID 测试 |
| S2 文件接管 | complete | 租约/执行者保护；重启非终态可恢复；取消与完成不被旧 worker 覆盖 |
| S3 运行时 | complete | 三入口共享启动；监听 readiness；监督/退避/停止；可查询健康 |
| S4 回执 | complete | 当前正文不等待历史全量；批次限额；ReadAck 优先；写超时 |
| S5 验证 | complete | T1–T5 针对性测试、desktop/web 检查、隔离运行 |
| S6 交付 | complete | 最终 diff、实施总结、平台与验证限制 |

## 实施前的基线事实

- 现有 outbox 含 `message_delivery_attempts`，有原子领取、60 秒租约、退避与同连接关联 ACK；继续复用。
- `save_conversation_message` 和 `ensure_message_recipients` 各自提交事务，文件任务创建也与正文分离。
- 自动文件恢复只查询 `waiting_peer`；重启遗留 `queued/transferring/cancelling` 尚无统一接管。
- 三入口分别 spawn 网络任务；UDP bind 失败返回，HTTP bind/serve unwrap。
- 每帧先 flush 所有历史回执；回执 mpsc 64；已读可能排在反复待送达分支之后。
- 图谱可能漏掉 `messaging.rs` 的部分函数；先图谱定位，不足再回读已知文件。不要把启发式调用边当作准确依赖。
- 当前前端是 React/Vite，已有测试；AGENTS 的旧技术概览不完全符合源码。此次不新增前端依赖或产品 UI。

## 决策

- 优先增量扩展现有核心和表结构，保留现有 wire 兼容和用户数据。
- 消息受理成功只在完整业务事务提交后返回；网络副作用放在提交后。
- 健康状态先用结构化诊断/API，沿用界面；不把新 UI 作为底层稳定性修复前置条件。
- 原有根目录规划内容保留；当前状态置顶，具体进度集中于本文及 findings/progress 新区。
- 用户追加要求重要功能/优化及时提交 Git；每个通过相关测试的里程碑单独提交中文 Conventional Commit，记录哈希。未授权推送。

## 验证计划

针对事务回滚/幂等、重启接管/终态保护、绑定失败/停止、积压回执/慢连接做有用的故障测试，不写镜像实现的测试。共享 Rust 修改后检查 desktop lib/bin 和 web bin；执行相关测试，再根据影响运行已有库回归。运行实例使用替代端口和临时数据库；不运行 FeiQ、不接触真实聊天数据。

## 最终验收与提交

阶段 1 已完成。最终代码提交 `839823c`，此前依次为 `068505e`、`7c35bc2`、`bdbf46b`、`7fbfe38`；最初计划提交 `72b65c9`。所有改动按里程碑在本地提交，没有推送。

- web lib：158 passed。
- desktop lib：183 passed、2 ignored（原有交互式截图测试）；desktop lib/bin check 通过。
- web bin check/build 通过；隔离 headless 两轮启动、强制退出与重启验证通过。
- Android arm64：本机 NDK 27.1.12297006 临时环境 check 通过，9 个现有 unused 告警。未做 Android 实机和真实多机压测。
- 代码图谱已重建；不改生产 UI、不改 gen/android、不包含前序 analysis 大体积反编译产物。
- 实施与验收报告：`docs/stability-phase-1-implementation.md`；原始运行结果：`docs/verification/2026-10-09-phase1-headless-smoke.json`。

以下为过程记录，其中“进行中”只描述当时状态。后续从最终报告进入阶段 2，继续保留本文件用于故障边界和设计原因交接。

## 已执行记录

- 计划/推荐文档已提交：72b65c9（docs: 固化稳定性阶段一范围与验证计划）。
- 读取 SQLx 本地源码确认 Pool::begin_with 支持 BEGIN IMMEDIATE；嵌套 PowerShell 的变量展开导致一次读取失败，改用 Python 定位已解决。

### S1 当前验证

- 原子 enqueue 已接入文本/文件发送；即时事务防并发重复写；存量 ID 保留目标与提及。
- 新增真实 SQLite 收件人和第二个文件任务失败回滚测试、重开库测试、并发同 ID/旧单聊补全测试。
- web bin 与 desktop lib 编译通过；atomic_enqueue 两项故障测试通过，完整 web lib 146 项全部通过。仅格式化修改函数，未执行全库 fmt。

- S1 实现提交 068505e（fix(outbox): 原子保存消息目标与文件任务）。

### S2 设计收敛

- transfers 追加内部 lease_token/lease_until（保留现有序列化模型）；worker 原子领取、周期续租、按 token 更新进度/终态，失去租约停止 IO。
- 启动/周期恢复仅接管无有效租约的发送非终态；保留 ID/字节/接收分块，取消收敛为 cancelled；活跃租约不抢占。
- cancellation registry 需按执行 token 完成，避免旧 worker 清理新一代的取消句柄。

- 文件 worker 已接入 60 秒租约/20 秒续租；周期扫描恢复非终态、token 防旧写、取消句柄按执行代次清理。启动接管将在共享 runtime 内调用。
- 组件网络测试使用真实领取的租约；另加磁盘库重启/租约抢占/取消竞态测试。

- 验证：file_execution_lease 故障测试通过；web lib 全量 148 项通过；desktop lib check 通过。恢复界限为租约最长 60 秒加扫描周期，不能抢占仍有效的跨进程执行者。

- S2 提交 7c35bc2（fix(transfer): 用执行租约恢复中断任务并隔离过期回调）。

### S3 设计收敛

- 共享 supervisor 统一三入口；先恢复队列并绑定 UDP/TCP，监听就绪后启动广播/看门狗。
- 子任务退出或 panic 记录服务错误，取消同代任务并按上限 30 秒退避重建；关闭取消与任务生命周期统一管理。
- /api/health 返回运行代次、监听就绪和最近错误；不改产品 UI。

- 三入口已切换 NetworkRuntime；UDP/TCP 先绑定与 ready 信号、子任务 JoinSet 监督、最高 30 秒退避、headless Ctrl+C/桌面 Exit 关闭已接入。
- web bin 编译通过；占用端口释放后恢复/健康 API/停止释放监听，以及 panic/退出测试进行中。
- 一次 PowerShell 内嵌双引号导致入口编辑命令解析失败，未改文件；改成独立临时 Python 脚本后成功。

- 首次 runtime 故障测试中 Windows 允许已有 loopback 监听与 wildcard 监听共存，因此未触发预期 bind 错误；改为占用相同 wildcard 地址。测试下载目录也显式设为临时目录。
- 服务停止现在同时取消该代 HTTP 请求/升级 WebSocket，防止旧连接跨代残留。

- S3 验证：network::runtime 两项测试通过，覆盖同地址端口冲突、退避恢复、健康 API、WebSocket 随服务停止、监听释放、panic 观察和停止打断；desktop lib/bin 与 web bin check 通过。

- S3 提交 bdbf46b（feat(network): 统一服务监督就绪状态与受控关闭）。

### S4 设计收敛

- 当前 ACK 独立高优先队列；历史回执分页上限 16 条、低优先通道、游标推进，取代收帧前全量查询/阻塞入队。
- 回执发送选择最高 ReadAck；成功发送 ReadAck 同时覆盖 delivery 标记，反向补发仍持久兜底。
- 当前消息处理不等待反向连接；慢 socket 写 3 秒上限，退出同步清理 reader/forward/pager。

- S4 已接入当前帧与历史 worker 分离、游标分页、ACK 高优先转发、3 秒写超时；正在测试 80 条历史积压、最高回执、满通道不阻塞和慢写上限。
- 反向回执继续由既有控制调度器发送；ReadAck 成功同时完成 delivery 标记，兼容旧数据库只写 read 标记的记录。

- S4 验证：80 条积压下当前 ACK 优先、低优先通道满时不阻塞、分页 16 条、ReadAck 最高状态及两标记完成、慢写截止时间通过；完整 web lib 152 项通过，desktop lib/bin check 通过。

### S5 当前工作

- Android arm64 在本机 NDK 临时环境下 check 成功，0 错误、9 个现有未使用代码告警。没有改全局环境或 gen/android 文件。
- 最新生产代码再次执行 desktop lib/bin check；补入接收端存储失败测试和合并校验前退出后，再跑一次 desktop lib 回归以覆盖最终测试集。

- 最终 web lib 158 项通过；desktop lib 182 项通过、2 项交互式截图测试保持原有 ignore。
- headless 实际 build 通过；独立端口 21289、临时数据库和下载目录、关闭广播，两次强制退出/重启的 health 与 workspace 请求成功，证据见 docs/verification/2026-10-09-phase1-headless-smoke.json。
- Android 第一次 check 失败：找不到 aarch64-linux-android-clang。已定位本机 NDK 27.1.12297006，使用临时子进程 CC/AR/linker 环境重试，不改全局环境。
- Codebase graph 已重新索引，修正修改后 snippet 行号陈旧。

- 真实消息子进程退出测试 5 个边界已通过：未提交正文/目标不泄漏，已受理任务可领取，写出无 ACK 不误报，ACK 落盘后不重复投递。
- 群目标测试已改为真实群成员变更；接收文件测试改为子进程合并完成/发布后直接退出，再读磁盘 manifest 恢复与验证只有一份最终文件。
- 租约字段迁移显式检测列并传播非重复迁移错误；健康状态追加数据库探测和当前可用发现接口数。
- 最终 web/desktop 全库回归与 headless 实际构建进行中；隔离 smoke 脚本位于临时目录，数据库/下载目录/端口独立，发现广播关闭。

- S4 提交 7fbfe38（fix(receipts): 优先当前回执并限制历史补发与写等待）。
- 补足实际子进程退出后的事务/队列恢复、接收端发布后回执前重启、旧表迁移检查。
- 复核 runtime 数据库健康与网卡缺失的诊断边界，完成最终 desktop/web 回归和隔离 headless 运行。

### S1 设计收敛

- 抽取同连接消息写入 helper，保留现有公开保存函数供入站等调用；新增原子 outgoing 事务入口。
- 入队事务包含正文、收件人快照、提及及可选文件元信息/每对端任务，所有网络 spawn 在 commit 之后。
- 重试以存量收件人记录为准，避免用新群成员重解释旧消息；缺失目标的历史单聊可按原 receiver 修复，缺失快照的历史群任务明确报错，不能猜测原目标。
- 采用 SQLite 触发器注入收件人/文件任务写失败，验证实际事务回滚与重启后可领取；不依赖生产测试开关。

- 阅读 planning-with-files、RTK、AGENTS；session-catchup 无额外输出。
- 初始 git status：仅前序 analysis 与推荐文档未跟踪。
- 根规划合读输出超限；改为精确读取，不重写历史。

## 交接须知

从本文与根 `task_plan.md` 当前任务区恢复；看 `git diff` 识别真实变更。未完成项不得标完成。不要因文档批准请求再次批准同一底层实施。若需要新增 UI，则先完成原型及其他独立底层工作后请求具体原型评审。

文档收尾时内嵌 PowerShell 命令中的 Markdown 反引号导致一次 Python 解析失败；文件未改变，改用独立脚本完成。
