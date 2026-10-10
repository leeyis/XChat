# 聊天语音生产验证

本轮把聊天语音接入独立消息类型、持久附件队列、时长/MIME 元数据和实际麦克风录制。原型已获用户批准；没有修改截图实现。

- Web 共享核心全量测试：176 / 176 通过。
- 前端全量测试：184 / 184 通过，含录音拒绝、迟到授权、停止、取消和 60 秒上限状态测试。
- `rtk npm run build`、desktop lib check、web bin build：通过。
- 隔离双实例使用端口 18921 / 18922 和临时数据库，关闭自动发现。Chrome 使用独立 profile 和合成麦克风设备，执行生产界面的录制、发送、接收、播放。
- 实测 2077 ms、audio/webm、12850 bytes；两端稳定消息 ID、语音元数据、SHA-256 一致。接收端关闭自动文件接收仍能收到语音；播放进度实际前进，零未处理异常。
- 修复转发丢失语音元数据、已有数据库触发器不识别语音、恢复发送协议可能降级及会话摘要显示内部文件名。
- 生产 Tauri 隔离冒烟通过：独立应用标识、端口 18923、临时数据库，WebView2 的录制、send_voice_message 权限、入队和 get_workspace_media_source 访问均通过。物理麦克风、真实两台机器、Android 实机尚未验证；本记录不将合成设备视作硬件验收。

重跑双实例检查需 Python 的 `websockets`、Chrome 以及已构建的 headless binary：

```powershell
rtk proxy python -u -X utf8 docs/verification/2026-10-10-voice-production-e2e.py --binary K:/cargo/debug/lanchat-web.exe
```

临时数据库、进程和浏览器 profile 均属于测试；脚本退出停止自己启动的进程并保留日志。前端测试初次失败来自临时编辑脚本引入 CRLF，恢复项目的 LF 后通过。Tauri CLI 的应用参数需要 `-- -- --port ...`，按本机 CLI 帮助修正后重启隔离验证。
