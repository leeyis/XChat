# 接收文件时消息列表抖动修复

用户在安装 0.1.7 后反馈：接收大文件时，消息列表在两个位置之间反复上下跳动。排查基线为 `d0a200d`，修复恢复既有界面的稳定阅读行为，没有新增布局或交互。

## 原因与修改

现代接收路径每个完整分块广播 `file_download_progress`。事件包含稳定消息 ID、会话 ID 和文件名，但不包含完整消息的时间、正文、发送者、文件路径和大小。

前端通用消息分支仅凭会话 ID 与文件名，就将这个局部控制事件规范化并合并到消息缓存。规范化的默认字段覆盖原文件消息，尤其将时间改为 0，导致消息排序移动。约 100 ms 后，历史刷新又恢复原消息；传输期间这一过程不断重复，滚动锚点随之反复补偿。

在通用消息合并之前，复用现有控制事件类型集合，同时识别外层 Tauri 事件名与内层载荷类型。进度和状态事件继续触发权威刷新，但不再替换聊天消息。完整的 `msg_type: file` 创建／完成事件仍然立即合并。

生产修改仅涉及 `frontend/src/xchat.js` 的事件分支及对应构建资源。后端事件、文件协议、数据库、滚动控制器及 CSS 无需修改。

## 回归与验证

- 新增一个聚焦回归用例，使用真实接收进度载荷、延迟历史响应，并覆盖 `new-message` 封装、无内层类型的 Tauri 下载／上传事件及完整完成事件。
- 原代码红测复现：文件消息的类型、正文、发送者、路径、大小和排序被进度事件覆盖。修复后，该文件 9 项测试通过，全部前端测试 145/145 通过。
- 真实生产构建 Chromium 对照：每种阅读位置发送 36 次真实格式的进度事件，历史响应延迟 100 ms。修复前，底部位置在 1377／1485 间切换，历史阅读位置在 1239／1347 间切换；修复后分别保持 1479、1239，没有程序滚动写入，文件卡和进度条始终存在，页面错误为 0。
- 生产构建与 `git diff --check` 通过。
- Windows Tauri／WebView2 真实接收路径通过：以 33 个真实分片接收 128 MiB 加 44 字节的 WAV；底部和上移 240 px 阅读各上传 12 个 4 MiB 分片，分别采样 590、507 帧，滚动位置保持 2058.399902、1818.400024，程序滚动写入为 0。24 次真实 `new-message/file_download_progress` 事件期间，消息顺序、文件卡、进度条和页面高度稳定；剩余分片接收完成后显示暂停的原生音频播放器，页面错误为 0。没有改写可见性或操作系统焦点。

运行命令（仓库根目录）：

```powershell
rtk proxy node --test frontend/src/message-refresh.test.js
rtk npm test
rtk npm run build
rtk proxy node C:/Users/wangy/AppData/Local/Temp/xchat-scroll-repro.cjs --assert-stable
rtk proxy cargo tauri dev --config <temporary-config> -- -- --port 18889 --db-path <temporary-db-directory>
rtk git diff --check
```

浏览器对照使用隔离 HTTP／WebSocket 响应和实际生产资源，不读取用户数据库。原生运行使用独立应用标识、备用端口和临时数据库。此次没有修改 Rust；Android、macOS／Linux 原生运行未验证。

验证证据保存在本次本机临时目录：浏览器基线 `xchat-scroll-browser-mD6dst/results.json`、修复后 `xchat-scroll-browser-epQLj3/results.json`，以及原生 `xchat-scroll-native-_2edvt01/native-scroll-results.json`。这些临时配置、数据库、长音频和运行记录不纳入提交。
