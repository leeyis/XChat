# 稳定性阶段一 QA 复查与修复

日期：2026-10-09。复查基线 `3557f78`。范围为阶段一的原子入队、文件执行租约与恢复、网络服务监督、回执优先及服务关闭。实施记录见 [QA 计划](plans/2026-10-09-stability-phase-1-qa.md)，原阶段一结果见 [实施报告](stability-phase-1-implementation.md)。

## 确认的问题

| 问题 | 严重度 | 触发条件与影响 | 修复 |
| --- | --- | --- | --- |
| UDP 单包错误导致全网络重启 | 高 | Windows 收到超过 1024 字节缓冲区的报文后返回 WSAEMSGSIZE；监督器把它当成监听失败，关闭 HTTP 和已有 WebSocket。隔离进程发送 2048 字节报文已复现 health 不可达、generation 1→2 | 丢弃超长、损坏及单包连接错误，真正的监听错误仍交监督器恢复；Unix 检查截断标志，不解析半包 |
| 文件取消在 worker 启动窗口丢失 | 高 | 租约领取后、内存取消句柄注册前，取消已写入 SQLite；新句柄仍为未取消。确定性故障注入证实接收端仍收到文件 | 续租原子返回运行/取消状态；启动、心跳及完成前同步持久化取消到该次执行句柄 |
| 空收件人消息被受理 | 中 | 群里只剩自己时消息仍提交，随后相同 ID 重试报缺失快照，无实际投递目标 | 在入队事务中拒绝空目标，回滚正文；既有非空目标快照保持冻结 |
| 端口 0 假就绪 | 中 | TCP/UDP 各自选不同随机端口，健康状态与发现公告仍使用 0 | 监督器启动前校验并停止，明确报告配置错误；headless 以失败退出码结束；启动等待期间响应停止信号 |
| 自动恢复替换仍兼容的分块布局 | 中 | 重启接管后，新并发设置或对端新增协议支持触发重新协商，替换传输 ID，已有分块无法接续 | 复用手动续传的布局保留规则；原方案仍受支持时继续使用相同 ID/布局，必要时才重新协商 |

## 验证记录

修复前的空目标及取消竞态两项回归均失败；UDP 集成用例也确认已有 WebSocket 被关闭。测试使用独立 SQLite、临时文件和 loopback 接收端。

修复代码已提交：`9ba5e88 fix(stability): 修复阶段一取消恢复与网络异常边界`。

| 检查 | 命令 / 方式 | 结果 |
| --- | --- | --- |
| Web 全库回归 | `rtk cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features web --lib` | 162 项通过 |
| 提交后 Web 定向复验 | 同命令追加 `qa_` | 新增 4 项全部通过，含测试清理边界调整 |
| Desktop 编译 | `rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib --bin lanchat` | 通过 |
| Web 编译 | `rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features web --bin lanchat-web` | 通过 |
| Web 可执行程序构建 | 同参数执行 `rtk cargo build` | 通过 |
| Desktop 全库回归 | `rtk cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib` | 187 项通过，2 项原有交互截图测试 ignored |
| Android arm64 编译 | `rtk cargo check --manifest-path src-tauri/Cargo.toml --target aarch64-linux-android --no-default-features --features desktop --lib`，沿用原阶段一 NDK 子进程环境 | 通过；保留原有 9 个未使用变量/函数警告 |
| Headless 真实进程 | 临时库/下载目录、独立端口 47896、关闭发现广播；两轮各发送 20 个空/畸形/2 KiB/60000 字节报文 | 两轮始终 ready、generation 均保持 1、workspace HTTP 200；强制退出后重启正常，端口释放；`--port 0` 退出码 1。见 [原始结果](verification/2026-10-09-phase1-qa-headless-smoke.json) |

第一轮全库回归出现过 1 个测试断言问题：其他并行测试发布的普通 WebSocket 广播被当成断连。断言改为持续读取普通帧、只把 Close/错误/EOF 判为断连后，全库通过。自动恢复用例还等待消息聚合状态落盘后才关闭临时库，避免收尾中的后台 worker 访问已关闭连接池。

文件 worker 的领取与句柄注册现在按同一顺序串行完成，避免长时间挂起的旧执行者在新租约接管后覆盖新句柄；锁不覆盖上传、文件摘要计算或网络 IO。正常上传仍使用原有并发控制器。

## 范围与限制

本次沿用现有协议、数据模型及界面，没有依赖或数据库结构变更。对原先仍有效的执行租约、旧执行者写入隔离、持久回执分页及慢连接截止时间继续运行既有回归。

验证环境是 Windows x64 与 Android arm64 交叉编译；未运行 macOS/iOS/Linux 目标或 Android 实机。自动化故障测试与本机进程验证不能代替多机切网及长时间混合负载验收；本次不据此推断吞吐提升倍数。群成员移除后历史待投消息的产品语义仍应在后续群同步设计中明确，本次不放宽接收端成员校验。
