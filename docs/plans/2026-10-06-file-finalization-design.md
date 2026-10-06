# 大文件接收完成前的阶段提示

## 已批准的行为

用户已批准 `ui-ref/xchat-desktop-prototype.html?review=file-finishing`，并要求完成测试后升级至 0.1.9、提交并推送代码、构建新的 Windows 安装包。

接收卡片随真实工作显示“正在接收 → 正在合并文件 → 正在校验文件 → 正在保存文件 → 已完成”。处理阶段使用动画与“请稍候”，不显示网络速率或虚构的处理百分比。数据收齐但后台尚未进入具体阶段时，显示等待处理。接收和处理阶段保持卡片高度一致；完成后恢复普通文件卡片。处理期间不提供取消和打开文件的操作。真正失败才显示错误边框，校验失败提供重新接收入口。

## 实现边界

- 在共享接收核心记录短暂的处理阶段，由 Tauri 与 HTTP 工作区快照提供可选 `processing_phase` 字段；保留现有传输状态、协议和 SQLite 模型。
- 合并、校验、保存的提示对应实际执行边界；处理结束、失败或任务退出后清除阶段，避免旧状态影响重试。
- 复用已有控制事件通知刷新，不把阶段事件合并为消息，保留此前的滚动修复。
- 文件卡片与媒体接收提示复用相同阶段；接收中的临时文件不再误报“本地文件不可用”。

## 实施顺序

1. 为共享核心添加阶段记录与清理，连接实际文件合并、SHA-256 校验、最终保存流程，补充阶段与失败回归。
2. 将可选阶段加入工作区传输快照，保持旧客户端兼容，并保证失败时退出处理中状态。
3. 实现已批准的提示、动画和操作约束，补充前端阶段选择回归；检查原生窗口高度和滚动稳定性。
4. 同步版本为 0.1.9，生成前端资源，运行前端测试、受影响 Rust 测试及 desktop/web 编译检查。
5. 通过隔离端口、数据库和应用实例验证真实接收流程；记录结果后提交并推送。
6. 构建 NSIS 包，核验包内程序、版本与实际加载资源，交付新的安装包。

## 验收重点

并行接收在全部字节到达后仍显示真实处理阶段；成功校验前不显示已完成；失败能退出处理中且重试不继承旧阶段。阶段刷新不会改变消息时间戳、排序或正在阅读的位置。只读观察和隔离测试不修改用户正在运行的客户端与聊天数据。

## 实现与提交前验证

已完成共享核心阶段记录、可选快照字段、前端状态提示，以及 0.1.9 版本同步。阶段记录使用接收锁内的作用域守卫，成功、失败或任务退出后自动清理；失败仍沿用现有传输状态和重试流程。文件记录发布与传输收尾之间可能存在短暂间隔，打开按钮也受活动传输状态约束，防止提前出现并撑高卡片。

| 命令 / 场景 | 结果 |
| --- | --- |
| `rtk npm test` | 147 项通过，包含阶段选择、终态清理、版本一致性和控制事件不改写消息的回归 |
| `rtk npm run build` | 通过；重新生成 `src/` 的生产资源 |
| `rtk cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features web --lib web_server::websocket_protocol_tests -- --test-threads=1` | 21 项通过，包含摘要错误、重试、真实阶段顺序及快照序列化 |
| `rtk cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features web --lib network::conversation_file::tests -- --test-threads=1` | 20 项通过，包含实际分片合并与校验阶段 |
| `rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib` | 通过 |
| `rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features web --bin lanchat-web` | 通过 |
| 隔离的 `rtk cargo tauri dev` 原生 WebView2 | 0.1.9，125% DPI，256 MiB 文件、64 个真实 v4 分片；原生事件与 HTTP 快照均进入合并、校验、保存阶段，成功后清除处理提示 |
| 原生布局与滚动采样 | 接收阶段 524 帧，处理阶段 819 帧；卡片高度、消息顺序、历史阅读位置保持稳定，处理时无取消 / 打开按钮、0 B/s 或本地文件不可用误报 |

首次并行 Rust 筛选运行中，`conflicting_parallel_prepare_does_not_reactivate_failed_transfer` 在删除临时目录时遇到 Windows 文件占用（OS 32）；完整相关模块改为串行运行后通过，无断言失败。原生测试曾发现保存阶段提前出现打开按钮导致卡片增高，已修正并通过上述回归。

原生验证使用临时数据库、下载目录、独立应用标识和端口 18890；验证后已停止该开发实例。当前主机只验证 Windows 与 web 编译，未执行 macOS、Linux 或 Android 平台测试，也未改动用户其他电脑上的数据。

## Windows 安装包验收

提交后的构建命令为 `rtk cargo tauri build --bundles nsis`，目标产物 `K:\cargo\release\bundle\nsis\Xchat_0.1.9_x64-setup.exe`。发布时应从 NSIS 提取程序并核对其与 release 可执行文件一致，再核对包内完整前端资源与本次生成的资源一致；从隔离副本验证原生版本、HTTP 版本及运行时实际加载资源，避免交付旧界面。
