# 飞秋高价值能力复刻路线图

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this roadmap task-by-task. The linked sub-plans are independently testable and should be executed in the listed order.

**Goal:** 在不重写 XChat 现有消息/文件核心的前提下，优先交付飞秋式局域网聊天协作、共享文件和带双向语音的远程协助，并为后续 AI Agent/MCP/Skill 平台保留安全扩展位。

**Architecture:** 继续使用 React `XChatModule` 作为前端唯一运行时 seam，Tauri/Web adapter 作为两种真实接入；群协作和共享文件复用现有 SQLite、WebSocket/TCP、传输状态机和可信路径边界。远程协助采用控制面与媒体面分离：现有控制面负责邀请、同意、静音、停止和远控授权，WebRTC 负责局域网内的桌面视频和双向音频。AI Agent 不进入第一期，只预留 capability、会话类型和秘密配置边界。

**Tech Stack:** React 19.2.8、Vite 8.1.5、dependency-free CSS/DOM、Tauri 2、Rust/Tokio、Axum WebSocket/HTTP、SQLite/sqlx、现有分块传输核心、WebRTC（屏幕/麦克风媒体）。

## Global Constraints

- 不实现飞秋 IPMSG/2425 协议兼容层；这是功能复刻项目。
- 第一期开启：单聊/群聊加固、共享文件目录/下载、远程桌面共享、双向语音和远控二次授权。
- 飞秋空间日志、云账号、公网中继、AI 诊断、端到端加密体系不在范围内。
- 远程协助不落盘视频/音频；会话邀请、同意、静音、停止和控制授权必须显式且幂等。
- 模型密钥、MCP 凭据、Skill 内容永不进入局域网发现广播、普通消息或日志。
- 严格遵守 `ui-ref/DESIGN.md`：`56 / 280 / flexible / 240` 四栏、绿色只表达当前/主动作/进度、MAC 是稳定设备身份、文件状态包含阶段/进度/速度/动作。
- 所有新增 Tauri command 必须同步接入 `main.rs`、`lib.rs`、`permissions/commands.toml` 和相关 capability JSON。
- 共享 Rust 行为必须同时验证 `desktop` 与 `web` feature；不做仓库级 rustfmt 清理。
- 保留当前工作区已有改动；计划执行时只提交对应任务文件，不把 `AGENTS.md`、`plan/UI-DESIGN-PC.md`、`.happycode/`、`ui-ref/` 等用户内容混入产品提交。

## 优先级与交付门槛

| 里程碑 | 内容 | 目标交付 | 依赖 |
|---|---|---|---|
| M0 | 合同与能力矩阵 | 事件/能力/错误码固定，双 adapter 不漂移 | 无 |
| M1 | 核心协作 | 群公告、群文件共享、共享文件目录、请求下载 | M0 |
| M2 | 远程协助 | 桌面共享、双向语音、静音、停止、远控二次授权 | M0；媒体能力 spike 通过 |
| M3 | 桌面增强 | 黑名单、勿扰、隐身、Windows 共享浏览、备份恢复 | M1 |
| M4 | Agent 平台 | 模型连接、本地 Agent、MCP/Skill、局域网暴露/私有 Agent | M0、M1；独立发布 |

M1 和 M2 可分别发布；M2 如果某平台没有可用屏幕/麦克风权限，只能在该平台显示明确 capability 禁用状态，不得伪造“已共享”。

这里的“一期”指 M0→M2 的首个交付波次：远程协助不是后置到 M3/M4，而是与核心聊天、群协作和共享文件一起完成首期验收；M3/M4 属于后续增强与独立发布。

## 子计划

- [核心协作与共享文件](2026-07-31-feiq-core-collaboration.md)：M0 + M1。
- [远程协助与双向语音](2026-07-31-feiq-remote-assist.md)：M0 + M2。
- [AI Agent/MCP/Skill 平台](2026-07-31-xchat-agent-platform.md)：M4，后续独立排期。

## 推荐执行顺序

1. 先执行核心协作子计划的 M0/M1，保持现有聊天和文件回归全绿。
2. 并行做远程媒体能力 spike；只有桌面/Web 目标环境能实际获取屏幕和麦克风时，才进入 M2 控制面与媒体面实现。
3. M2 完成后再做 M3 桌面便利能力，避免在远程协助未稳定前继续扩展入口。
4. M4 只先做数据模型和秘密边界设计，再独立实现 Agent 运行时；不要把 Agent 逻辑塞入普通消息发送分支。
