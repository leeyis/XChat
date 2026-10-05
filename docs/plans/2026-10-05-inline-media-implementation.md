# 对话内媒体实现计划

设计依据：[`2026-10-05-inline-media-design.md`](2026-10-05-inline-media-design.md)。用户已明确批准原型并授权实现。

## 执行顺序与责任

1. **后端媒体来源与分段读取**：增加共享响应模块、Web 来源接口和受本地 token 保护的消息媒体入口；登记 Tauri 命令、权限和能力。补齐 Range、HEAD、访问限制及 Android 文件句柄处理。
2. **前端播放器与资源状态**：在现有消息文件分支内实现音频、视频、动图；媒体源直接使用 URL；处理互斥、会话切换、可见区域及错误降级，不新增依赖。
3. **原图片附件暂存**：保留 GIF／WebP／APNG 等图片原字节，验证 MIME 与文件类型，复用现有受管目录和原子落盘；截图 PNG 流程保持独立。
4. **集成与验证**：检查接口、跨端权限、消息状态更新与生产构建一致，运行前端测试／构建及 Rust 两个 feature 的编译和测试，再进行真实服务与界面冒烟。

步骤 1 与 2 分别修改后端和前端，步骤 3 修改 `managed_image.rs`，避免文件所有权冲突。集成检查在三个路径完成后进行。用户追加授权：测试通过以后提交代码并同步远程仓库。只提交本需求改动，保留其他工作区改动，不进行仓库级格式化。

## 验证命令

从仓库根目录运行，所有 shell 命令使用 `rtk`。

```powershell
rtk npm test
rtk npm run build
rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib
rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features web --bin lanchat-web
rtk cargo test --manifest-path src-tauri/Cargo.toml --lib
rtk cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features web --lib -- --test-threads=1
rtk proxy cargo tauri dev --config <temporary-config> -- -- --port 18888 --db-path <disposable-directory>
rtk git diff --check
```

媒体 Range 与权限集成检查使用备用端口、临时数据库和本地原型样例。测试执行时记录实际命令、通过／失败结果以及未能运行的平台。

## 工作记录

- 实施前：生产代码基线为 `f3a53a1`；工作区只含已评审的媒体原型与本地演示样例。
- 已完成：设计文档及具体实施计划。
- 已完成：安全媒体来源、前端播放器、动图原字节暂存，以及命令注册与权限。
- 已完成：复查并修复 Unicode 文件名、StrictMode 位置恢复、历史接收状态及 Android fd 系统打开问题。
- 已通过：最终前端测试 144/144，生产构建；desktop 和 web 编译检查。初始共享工作区默认 Rust 库测试 157 项通过（包含并行新增测试）。
- 已通过：隔离检出的前端测试与构建，最终资源同步后与主工作区一致；Web 全量串行测试 138/138。
- 已通过：隔离代码的默认 Rust 库测试 156/156；两次模式覆盖相同媒体用例，未更改既有测试。
- 已通过：安装的 NDK 27.1、API 24 配置下 `cargo check --no-default-features --features desktop --target aarch64-linux-android --lib`，包括 JNI 文件打开、fd 定位读取及媒体命令；未进行 Android 实机播放检查。
- 已通过：隔离 Windows Tauri／WebView2 开发运行，真实命令权限、本地 token URL、Range、音视频播放与拖动、会话位置恢复、消息刷新保持播放器，以及 GIF／WebP／APNG 跨源暂停帧均通过。运行使用独立应用标识、备用端口和临时数据库，自有进程与端口已收尾。
- 已通过：真实 Tauri 暂存命令保留 GIF 38,357 字节与 APNG 12,328 字节，逐字节比对原文件一致，并成功执行丢弃命令；独立测试 outbox 入口已移除。
- 原生隐藏窗口的 `document.hidden` 保持 false，因此补齐 window blur 暂停音视频／冻结动图与 focus 恢复可见动图。最终 Chromium 冒烟与真实 WebView2 DOM blur/focus 事件验证均通过，focus 不自动恢复声音；没有操作系统级切换焦点，也没有改写 `document.hidden` 模拟解码。
- 首次共享工作区 Web 全量：136 通过、3 失败，均为既有测试清理临时 SQLite 目录时的 Windows 文件占用错误（OS error 32）；媒体专项通过。串行后这 3 项通过，仅并行新增的大文件测试发生同类清理错误，因此改用不包含该外部改动的隔离检出验证实际提交内容。
- 隔离 Web 全量首次 137/138，通过媒体专项，但既有 1 ms 解析超时测试在系统负载下偶发失败；再次完整串行运行 138/138 通过，未修改该测试或解析逻辑。
- 隔离默认库首次串行 155/156，既有 `legacy_resend_queue_ignores_messages_owned_by_the_receipt_pipeline` 在临时目录清理时发生 OS error 32，功能断言和媒体测试通过；按仓库默认并行度完整重跑 156/156 通过。
- 真实 HTTP 冒烟通过：7 种媒体／受管 outbox 的来源、完整读取、GET/HEAD、普通／开放／后缀 Range、416、跨源响应头，以及缺文件／未接收／越界路径／错误 token；18 个并发 Range 字节一致。
- 生产页面 Chromium 冒烟通过：真实 URL／时长、禁止自动播放、播放互斥、消息刷新不重建元素、切会话暂停与位置恢复、后台暂停、GIF／WebP／APNG 原文件动画及暂停帧、错误和待接收操作；900 px／390 px 无横向溢出，页面无 JavaScript 错误。
- 25,259,690 字节、32:11 的自制视频通过未缓冲 80% 位置拖动与继续播放检查；该长视频仅在临时测试目录生成，不纳入仓库。
- 接收／发送补充冒烟首先发现后端 `send/receive` 方向被新 UI 当作 `outgoing/incoming` 使用，导致发送文案显示成接收。增加规范化及回归用例后，真实接收 45% 无播放器、接收完成后切换为暂停的播放器、发送 35% 保留预览／进度／取消、取消调用真实 HTTP 接口且保留原播放器，全部通过。最终媒体页面冒烟再次通过。
- 工作区出现并行修改的 `src-tauri/src/network/conversation_file.rs`，不属于本需求，保留且不纳入本次提交。
- 完成状态：实现、最终前端测试／生产构建、两种 Rust feature 全量测试、真实 HTTP／浏览器／Windows WebView2 冒烟均通过，进入用户已授权的提交与远程同步。Android 仅完成 ARM64 编译检查，未进行实机播放；macOS／Linux 原生目标未运行。
