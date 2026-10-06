# 接收文件时消息滚动抖动的追加排查

## 运行端证据

用户确认发送方和接收方均显示 0.1.7；没有操作鼠标时仍抖动，顶部始终显示连接已验证。最初开发机的旧安装与故障现场无关。用户随后在开发机安装同一版本，并提供实际 Windows 接收方地址，才对两个运行端进行只读核验。

两个运行端均提供 `index-Bl90uMfF.js`，SHA-256 为 `2550692f203e5b8ca67ef4d1fd66fcb999e7d6610c5884da104c5f643723d6cc`。它与提交 `d0a200d` 的资源逐字节相同，早于 `fecfa41` 的进度事件过滤修复。开发机已安装 EXE 的完整资源也与之相同，EXE 的 SHA-256 为 `6e8c2c22ec9609693e4d597f300986749e5ae7e95609687c0110302e9a60c2b5`。

因此版本号 0.1.7 不能证明运行端已包含修复。此次确认的是实际运行代码仍旧，未据此推断用户安装操作或安装包流转中发生了什么。被动事件采样期间没有正在进行的传输，不能将该采样表述为现场抖动复现。

`fecfa41` 已修正的路径是：原生 `file_download_progress` 携带消息 ID 和会话 ID，但不包含完整消息字段；旧通用消息合并将缺失时间戳置为 0，短暂重排消息，随后快照刷新又恢复顺序。0.1.8 保留该过滤，避免进度事件重写完整消息。

## 本次修改

1. 自动连接重试保留既有公开状态和错误，完成身份校验后再发布结果。原行为每约 10 秒短暂切换至 `verifying`，使失败提示栏隐藏后重现；在失败 peer 的原生对照中可复现约 58px 的位移。主动刷新继续提供验证中反馈，复用原有身份校验、串行 gate、地址持久化和最终结果流程。
2. 滚动定位容差调整为 1px。125% DPI 下，整数 DOM 高度算出的最大值可能比 WebView2 实际可达位置大 1px；旧实现反复请求不可达位置。回归测试先复现 41 次写入，修正后只保留首次定位，新消息仍跟随底部。
3. `build.rs` 的 desktop 和 web 路径显式跟踪 RustEmbed 使用的 `src/` 目录，避免增量编译继续嵌入旧 HTTP 资源。发布前仍须运行前端构建；此依赖不替代资源生成。
4. 所有版本来源同步为 0.1.8，并重建静态资源及 Windows NSIS 安装包，使此次修复与旧 0.1.7 包可以明确区分。

这些变化恢复既有界面的稳定行为，没有新增布局或交互。连接失败提示栏与 DPI 两个问题有独立复现证据，但不是“始终在线且已验证”现场的直接根因证据。

## 验证

- `rtk npm test`：146/146 通过，包含进度事件和 DPI 滚动回归。
- `rtk cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features web --lib network::peer_connection::tests`：4 项通过。新增聚焦测试覆盖 18 种状态、身份结果与调用方式组合，验证后台状态保留、主动反馈、并发复用，以及成功、身份不匹配和断开后的最终结果。
- `rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib` 和 `rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features web --bin lanchat-web` 均通过。
- `rtk cargo tauri dev` 使用独立端口、数据库和应用标识完成原生冒烟验证。接收 128MiB、32 个真实上传分片；底部与历史采样分别为 1052、999 帧，位置和消息顺序稳定，无 JS 错误。首次启动超过辅助脚本的 100 秒等待窗口，编译继续完成后正常启动；这是测试脚本等待超时，并非应用启动失败。
- 从最终 0.1.8 安装包提取的程序，在 820×650 CSS、125% DPI 下接收 128MiB：底部与历史分别采样 1134、1145 帧，位置分别恒定为 340.7999878px、260.7999878px，滚动写入均为 0，30 个原生进度事件持续到达，32 个分片完成，无 JS 错误。

此前还使用两个隔离的 release 原生客户端，通过真正的 `send_conversation_file`，在 16 通道与 4 通道下各发送 512MiB。双方保持 `ready`，底部和历史位置稳定。该测试用于排除修复后进度流与高通道传输仍会普遍引发抖动，并不替代用户实际机器的复测。

原生 release 测试仅在副本中替换应用标识的 3 个字节以隔离单实例，逐字节确认其余代码与嵌入资源不变；数据库、下载目录、WebView 数据和端口均隔离。用户实际运行进程和聊天数据未修改。

## 安装包核验与限制

`rtk cargo tauri build --bundles nsis` 成功生成 `K:\cargo\release\bundle\nsis\Xchat_0.1.8_x64-setup.exe`。从 NSIS 包直接提取的 `lanchat.exe` 与 release 产物逐字节一致，完整前端资源与 `src/`、`dist/frontend/` 的构建资源一致，旧资源名均不存在。

- 安装包 SHA-256：`11ccbaf1390d09424e2a94097e2aa164f1c26b0cd63b4a5b14efe0cc311149c6`。
- 包内 EXE SHA-256：`fda413de3ecead3a7ba5ae2b21257e38847a99e05ecee2641fb555bca2d4b576`。
- 前端资源：`index-Ycd2_sVa.js`，SHA-256 为 `84cb6cf3f5d08b182bf532657d817ebdcc6ab8ee2581974191946fb45214ff03`。

从该包内程序制作隔离副本并启动后，Tauri 的版本 API 与 HTTP 身份 API 均返回 0.1.8；原生窗口和 HTTP 服务实际加载的 JavaScript 散列均与包内新资源一致。构建提示现有的 bundler type 标记缺失警告，NSIS 构建和启动核验成功；本次未验证自动更新器。

实际接收方升级至 0.1.8 后的表现仍需现场复测。Android、macOS、Linux 原生运行未验证。临时传输文件、数据库、诊断采样及原生测试报告保留在本机 Temp，不提交到仓库。
