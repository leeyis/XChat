# XChat 反馈修复第三轮设计

日期：2026-08-01

## 目标

一次完成以下五项桌面端反馈：

1. Windows 显示好友上线或消息通知时不再闪出 cmd/PowerShell 黑窗。
2. 左侧单聊会话头像显示在线状态圆点：在线绿色、离线灰色。
3. macOS 从 Finder 粘贴文件到输入框后可直接发送，不再二次打开文件选择器。
4. 群聊支持通过 `@` 选择成员，并且只有被提及的成员收到重点系统提醒。
5. Windows 托盘提醒继续闪烁，但不再出现黑色方块。

本轮不支持新旧客户端混用；前端、后端和局域网协议作为同一版本一起升级。

## 方案选择

采用复用现有模块的窄改动：

- Windows 通知继续使用现有 PowerShell 气泡通知，只为子进程增加无控制台启动标志。
- 在线状态继续读取现有 `conversation.peer.is_offline`，不增加接口。
- macOS 粘贴继续使用已经注册并授权的 `read_clipboard_files` 命令，恢复 React 重写时丢失的前端接线。
- 群聊消息增加明确的 `mention_ids`，复用逐消息、逐收件人的 `message_receipts` 保存提及状态。
- Windows 托盘在普通 Logo 与带红点 Logo 之间切换，不再加载空白图标。

不采用纯前端姓名解析，因为无法可靠支持重名成员、离线重发和定向提醒；也不引入富文本编辑器或单独的 mention 框架。

## Windows 通知与托盘提醒

### 通知黑窗

`show_notification` 的 Windows 分支保留当前 PowerShell 通知脚本，并通过 `std::os::windows::process::CommandExt::creation_flags` 设置 `CREATE_NO_WINDOW`。不使用 `-WindowStyle Hidden`，因为该参数要等 PowerShell 启动后才解析，仍可能短暂创建控制台。

### 托盘闪烁

删除托盘闪烁对 `icon_empty.png` 的依赖。新增一个与普通托盘图标同为 `32×32` RGBA 的提醒图标：保留现有 Logo，在右上角增加红色状态点。现有 500 ms 切换周期、单线程防重入状态、停止后恢复普通 Logo 的流程保持不变。

收到需要提醒的消息时仍调用 Windows 系统通知和现有 attention API。消息已读或主窗口重新获得焦点后停止闪烁。

## 会话头像在线状态

只在左侧单聊会话的头像右下角显示状态圆点：

- `conversation.peer.is_offline === false`：绿色实心圆点。
- `conversation.peer.is_offline === true`：灰色实心圆点。
- 群聊头像不显示圆点，因为群没有单一在线状态。

状态沿用现有 `peer-online` 事件即时刷新和工作区每 5 秒刷新，不增加轮询或后端字段。会话按钮增加可读的在线/离线文本，避免状态只靠颜色表达。

## macOS 粘贴文件

输入框粘贴事件按以下顺序处理：

1. 如果剪贴板已经提供 `text/uri-list` 或本机绝对路径，复用 `draft.addPaths`。
2. Tauri 桌面端只有浏览器 `File` 而没有路径时，立即调用现有 `read_clipboard_files`，取得 Finder 剪贴板中的真实文件路径，再复用现有路径校验和草稿附件流程。
3. Web 环境或纯图片剪贴板继续使用现有 Blob/图片暂存流程。

读取原生路径必须发生在粘贴时，而不是点击发送时，避免用户在两步之间复制其他内容后发送错误文件。发送草稿附件时若显式附件没有可用路径，返回文件不可用错误，不得回退到文件选择器。只有用户主动点击“添加文件”按钮时才打开选择器。

该方案不复制普通文件、不把大文件转为 Base64，也不扩展当前仅用于 PNG 的截图暂存命令。

## 群聊 @ 交互

### 成员选择

仅在群聊输入框中输入 `@` 时显示成员选择浮层：

- 排除当前用户。
- 可按成员显示名或当前 IP 地址过滤。
- 主行显示成员名，副行显示当前 IP；没有地址时显示“地址未知”。
- 支持方向键移动、Enter 或鼠标选择、Esc 关闭。

IP 只用于搜索和辨认；选中目标后始终记录稳定的成员 ID。选择后向文本插入 `@显示名 `。发送前只保留正文中仍存在对应提及文字的目标，避免删除文字后仍错误通知该成员。

### 发送与存储

前端发送文本消息时同时提交去重后的 `mention_ids`。后端仅允许群消息携带提及目标，并验证每个目标都是当前群成员且不是发送者；任何目标非法时整条消息不发送。

`ProtocolMessage::GroupMessage` 增加 `mention_ids`。`message_receipts` 增加 `mentioned INTEGER NOT NULL DEFAULT 0`，复用已有收件人行记录哪些成员被提及。发送端保存该标志，离线重发时从回执表恢复 `mention_ids`；接收端再次验证后保存本机的提及状态。工作区消息和实时事件都返回 `mention_ids`。

### 通知语义

- 群消息仍发送、显示并计入所有群成员的未读数。
- 群消息只有在 `mention_ids` 包含本机 ID 且应用不活跃时，才触发系统通知和托盘闪烁。
- 未被提及的群成员静默接收消息。
- 单聊消息继续使用现有通知行为。

## 异常处理

- @ 目标校验失败：不发送消息，保留输入内容与附件草稿，并显示错误。
- 粘贴到文件夹或不可读路径：保留其他有效附件，并合并显示一次错误。
- 粘贴的文件在发送前被移动或删除：发送失败并保留草稿，不打开选择器。
- Windows PowerShell 启动失败或托盘提醒图标加载失败：不影响消息接收；保留错误日志并恢复普通托盘图标。
- 同一消息重复到达：沿用稳定 `client_message_id` 去重，提及标志不得因重复保存而丢失。

## 代码范围

前端：

- `frontend/src/App.jsx`：头像状态、粘贴分流、@ 选择浮层与发送参数。
- `frontend/src/xchat.js`：剪贴板路径适配、mention 数据归一化、两个传输适配器、定向通知。
- `frontend/src/styles.css`：头像状态圆点和 @ 浮层。
- `frontend/src/xchat.test.js`、`frontend/src/styles.test.js`：聚焦回归测试。

Rust：

- `src-tauri/src/commands.rs`：Windows 无窗口 PowerShell、托盘提醒图标、Tauri 消息参数。
- `src-tauri/src/network/protocol.rs`：群消息 `mention_ids`。
- `src-tauri/src/db.rs`：`mentioned` 字段及读写。
- `src-tauri/src/workspace.rs`：校验、保存、发送和离线重发。
- `src-tauri/src/web_server.rs`：HTTP 参数、群消息接收和实时事件。
- `src-tauri/icons/`：新增带红点的 `32×32` 托盘提醒图标。

现有 `read_clipboard_files` 命令已完成注册、权限和 capability 配置，本轮不新增命令或依赖。

## 验证

### 自动验证

- 前端单元测试：名称/IP 过滤、键盘选择、删除提及后移除目标、Tauri 粘贴路径优先、显式附件不打开选择器、单聊通知不变、群聊仅本机被 @ 时提醒。
- 样式测试：单聊头像存在绿色/灰色状态类，群聊头像无状态类，状态存在非颜色文本。
- Rust 协议测试：`mention_ids` 往返、非法成员拒绝、重复 ID 去重。
- Rust 数据库/工作区测试：提及标志保存、重复保存、离线重发恢复目标、接收端事件保留目标。
- 托盘资源检查：普通与提醒图标均为 `32×32` RGBA，提醒图标不是全透明帧。

运行：

```bash
rtk npm test
rtk npm run build
rtk cargo test --manifest-path src-tauri/Cargo.toml --lib
rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib
rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features web --bin lanchat-web
rtk cargo check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc --no-default-features --features desktop --lib
```

### 桌面冒烟

- macOS：Finder 复制图片、文档和其他普通文件，粘贴后直接发送，确认不出现选择器。
- 三个群成员：发送普通群消息时其他成员静默接收；分别 @ 一个成员，确认只有该成员在后台收到系统通知和托盘提醒。
- 好友上下线：确认单聊头像圆点在绿色与灰色之间更新。
- Windows：触发好友上线通知和消息通知，确认无 PowerShell/cmd 黑窗；隐藏到托盘后收到被 @ 或单聊消息，确认普通 Logo 与红点 Logo 交替且没有黑色方块，聚焦窗口后恢复普通 Logo。

## 完成标准

五项反馈均按上述行为完成；前端、Rust、desktop/web 和 Windows 交叉编译验证通过。无法在当前 macOS 环境自动观察的 Windows 窗口与托盘行为，必须在真实 Windows 环境完成最终冒烟后才视为平台验证完成。
