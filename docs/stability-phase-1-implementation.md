# XChat 稳定性阶段 1：实施与验收

日期：2026-10-09。对应[飞秋借鉴建议](feiq-inspired-stability-performance-recommendations.md)的阶段 1，基线版本 0.1.13、提交 `6298408`。

## 已落地的行为

| 项目 | 实现后的行为 | 实现提交 |
| --- | --- | --- |
| 原子入队 | 消息正文、收件人快照、提及及文件任务在同一 SQLite 事务提交；提交之后才启动网络任务。重复 ID 沿用原目标与提及，冲突返回错误。 | `068505e` |
| 文件恢复 | 文件 worker 原子领取执行租约，每 20 秒续租；启动及后台扫描接管租约失效的非终态任务。旧执行者不能覆盖新进度或完成状态，取消句柄也按执行代次隔离。 | `7c35bc2` |
| 网络运行时 | desktop、移动入口和 headless 共用监督器。UDP/TCP 监听就绪后启动广播；监听失败、任务退出或 panic 进入退避重建；停止时清理该代请求和 WebSocket。 | `bdbf46b` |
| 回执处理 | 当前 ACK、历史回执、广播分别排队并按优先级转发。历史查询每页最多 16 条，慢连接写入最多等待 3 秒；ReadAck 覆盖较低的送达状态。 | `7fbfe38` |

验收补强提交 `839823c`，包括租约字段的显式迁移错误反馈、数据库运行时探测、发现网卡数量诊断，以及实际子进程退出测试。计划与交接记录见[实施计划](plans/2026-10-09-stability-phase-1.md)。

## 恢复与兼容规则

- 文件执行租约有效期 60 秒。进程异常退出后，接管等待剩余租期及下一轮扫描；仍有效的执行者不会被另一实例抢占。
- 接管保留传输 ID、源文件信息及记录的字节数。接收端从磁盘 manifest 和已提交分块恢复，最终发布仍经过校验；恢复进度不能仅凭发送端字节计数推断。
- `awaiting_acceptance` 保留用户确认语义；已完成、已取消和明确失败的任务不会被后台恢复改写为自动重发。
- 旧单聊缺少目标记录时，可按原接收者补全。旧群消息若没有原始目标快照，返回明确错误，不能按今天的群成员猜测旧目标。
- 租约字段属于内部数据库元数据，现有消息/传输的 JSON 格式和协议保持兼容。原有消息 outbox 的稳定 ID、回执核验和退避机制继续复用。
- 入站连接上的 ACK 仍保留持久补发记录，以兼容不读取原连接回执的旧端；当前消息处理不再同步等待反向连接。

## 健康诊断

在实际运行端口访问 `GET /api/health`，可查看：

| 字段 | 含义 |
| --- | --- |
| `state` / `generation` | 启动、就绪、停止或退避状态，以及服务重建代次 |
| `http_ready` / `discovery_ready` | TCP 消息/文件入口和 UDP 发现入口是否就绪 |
| `announcing` / `watchdog_running` | 广播任务和恢复调度器是否运行 |
| `database_ready` | 启动恢复及周期数据库探测结果；探测失败会停止该代服务并进入恢复 |
| `eligible_discovery_interfaces` | 最近一次发现快照中可用的接口数量；可用于区分监听正常与缺少可用网卡 |
| `last_error` / `retry_in_seconds` | 最近一次故障及本次重试间隔；成功恢复后保留最近错误供排查 |

`ready` 表示服务已就绪。对端是否可达仍由设备身份核验、连接和 ACK 决定。退避间隔为 1、2、5、10、30 秒，之后上限保持 30 秒；端口占用期间没有可用的 HTTP 健康入口，错误仍会写入结构化运行日志。

## 故障验收

| 场景 | 实际验证内容 |
| --- | --- |
| T1 消息边界 | 子进程在正文未提交、目标未提交、已受理、已写出无 ACK、ACK 已落盘等边界直接退出。未提交事务回滚，已受理任务可恢复，无 ACK 不误报送达。接收端回执写失败时不发送 ACK，重放仍只有一条正文。 |
| T2 重启与旧数据 | 磁盘数据库重开、旧 transfers 表补迁移、真实群成员变更后的目标冻结、并发领取、租约过期接管、旧 worker 完成回调与取消竞态。 |
| T3 文件边界 | 发送任务排队/传输/取消等状态恢复；接收端子进程在合并校验前、合并后、文件发布后直接退出，再从磁盘恢复并完成，最终字节正确且只有一份文件。 |
| T4 服务故障 | 相同地址端口占用后释放，验证退避恢复；后台 panic 和停止中断；健康 API；停止释放监听和升级后的 WebSocket；数据库探测失败。网卡变化继续使用既有接口快照刷新逻辑，并增加可用接口数量诊断。 |
| T5 回执积压 | 反向不可达时原连接 ACK；80 条历史积压下当前 ACK 优先；最高 ReadAck；分页上限；满通道立即返回；慢写有截止时间。 |

进程退出测试使用测试子进程直接 `exit`，不执行 Rust/SQLx 的正常清理。租约到期通过修改测试库时间字段模拟，避免每个用例等待一分钟。慢写使用永不完成的 Sink 注入，不等同于真实网络吞吐测量。

## 命令与结果

| 验证 | 命令 / 方式 | 结果 |
| --- | --- | --- |
| Web 库回归 | `rtk cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features web --lib` | 158 项通过 |
| Desktop 库回归 | `rtk cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib` | 183 项通过；原有 2 项交互式截图测试 ignored |
| Desktop 编译 | `rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib --bin lanchat` | 通过 |
| Web 编译及构建 | `rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features web --bin lanchat-web`；同参数执行 `rtk cargo build` | 通过 |
| Headless 实例 | 独立端口 21289、临时数据库与下载目录、关闭发现广播；实际启动、强制退出、重启，再查 health/workspace | 两轮成功；[原始结果](verification/2026-10-09-phase1-headless-smoke.json) |
| Android arm64 编译 | `rtk cargo check --manifest-path src-tauri/Cargo.toml --target aarch64-linux-android --no-default-features --features desktop --lib` | 通过；使用本机 NDK 27.1.12297006 的临时 CC/AR/linker 环境 |

首次端口冲突用例在 Windows 上因 loopback 与 wildcard 地址可共存而未触发预期错误，已调整为占用相同地址并通过。测试下载目录随后显式指定为临时目录。NDK 的重试使用子进程环境，不修改全局设置。Android check 保留 9 个现有未使用变量/函数告警，涉及 commands、managed_image 和 discovery_policy。

当前证据来自本机自动化故障测试、编译检查和隔离进程运行。尚未进行 Android 实机、真实多机断网/切网、持续大文件混合负载和 P95/P99 性能验收；此阶段没有声明吞吐提升倍数。后续性能基准与文件分块重试按照原方案阶段 2 推进。

Android 重试的 NDK 工具来自 `toolchains/llvm/prebuilt/windows-x86_64/bin`：临时设置 `CC_aarch64_linux_android=clang.exe`、`CXX_aarch64_linux_android=clang++.exe`、`AR_aarch64_linux_android=llvm-ar.exe` 为各自绝对路径，C/C++ flags 为 `--target=aarch64-linux-android24`，linker 指向 `aarch64-linux-android24-clang.cmd`。SDK/NDK 路径仅注入该次检查进程。
