# Xchat 0.1.0 第二轮反馈实施计划

依据：`2026-07-29-xchat-feedback-round-2-design.md`（提交 `ca41d08`）

## 1. 设备元数据与默认名称

- 为 PeerManager 与 SQLite 先补 reply/announcement 交替红灯测试。
- reply 仅刷新在线信息；权威 announcement 更新 MAC/capabilities，内存仅接受首个权威正值。
- 新库优先采用 macOS ComputerName；旧随机名安全迁移；用户保存后标记 custom。
- discovery 声明 `parallel_file_v2`。
- 设置身份卡片显示只读的本机主路由 IP 与权威 MAC，无数据时明确回退。

验证：聚焦 Rust 测试，连续 20 次 workspace 采样稳定。

## 2. React 日常交互

- 表情扩充到不少于 80 个，选择后关闭并恢复光标。
- Tauri 原生拖放与 Web DOM 拖放统一进入附件草稿。
- 活动 transfer 一秒刷新，文件卡显示进度、已传/总量和速度。
- 完成文件按钮增加“打开文件 / 在 Finder 中显示”菜单。

验证：前端逻辑测试、构建和浏览器/Tauri 交互烟测。

## 3. 微信式截图 Overlay

- 将原生交互截图替换为当前显示器静态全屏捕获和无边框 overlay。
- 实现选区创建、移动、八点缩放、尺寸标签和跟随选区的悬浮工具栏。
- 复用现有六种 Canvas 标注，补 redo、颜色/粗细、保存、钉图、完成和取消。
- 所有失败与退出路径恢复主窗口。

验证：绘制/历史/工具栏定位测试及真实 Tauri 截图烟测。

## 4. 文件协议 v2

- prepare manifest 记录身份、文件 SHA-256 和 1/4 分块布局，冲突返回 409。
- 大于 4 MiB 的文件固定四等分并以有界流并发上传；旧 peer 回退 v1。
- 接收端独立写 part，支持乱序、幂等和断点续传，合并后校验 SHA-256 再原子完成。
- chunk、cancel、finalize 使用同一 transfer 锁和确定的竞争规则。
- 传输过程中持续更新真实字节数。

验证：四路乱序、重复、恢复、校验失败、取消竞争、v2→v1，以及大于 20 MiB 双实例传输。

## 5. 集成与提交

- 运行 `npm test`、`npm run build`。
- 运行 Rust 全量测试和 desktop/web 双 feature 编译。
- 使用隔离端口与数据库启动两个实例，验证发现、聊天、拖放、截图和四路传输。
- 运行 `git diff --check`，只提交本轮文件，不混入用户已暂存和未跟踪内容。
