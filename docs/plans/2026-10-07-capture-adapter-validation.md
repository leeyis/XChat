# 截图双端适配验证（2026-10-07）

对应用户已批准的截图原型 v0.7。改动范围：frontend/src/xchat.js、frontend/src/App.jsx、frontend/src/capture-adapters.test.js、capture-library.js 的原子补丁接口、现有 xchat.test.js 的新增 delay 参数断言。

## 实现

- Web 抓屏由 getDisplayMedia 获取真实来源，在当前页面打开 CaptureSurface；不再把 PNG 塞入 localStorage，也不再打开截图 popup。成功、失败、取消均释放所有媒体轨道，重复启动合并同一次请求。
- 抓屏无需当前会话。添加聊天草稿始终使用截图的来源会话；导入、历史回放没有会话时不会误投当前聊天。原生主窗导入使用 stage_image_attachment，不消费其他原生编辑窗口。
- 相同 capture-ready 事件统一经适配订阅处理，本地 CustomEvent 和跨窗口 BroadcastChannel 均支持，现有草稿 ID / 路径去重继续生效。
- 贴图使用稳定 ID 和 IndexedDB。创建原生窗口前保存完整可编辑文档，创建失败回滚；关闭原生窗口前写入隐藏/销毁状态。多图复制、保存、旋转、翻转、透明度、窗口阴影、分组、位置、缩放与恢复均按 pinId 操作。
- Native 首次贴图由 Rust 按真实显示器定位，恢复使用已有 view。临时 overlay 模式不保存临时窗口几何；尚未启动的原生贴图记录可以先更新存档再恢复。
- App 挂载 CaptureWorkspace，截图按钮右键打开工作台。编辑期间避免再次触发页面截图快捷键。Android 不挂载桌面原生截图工作台。

## 自动验证

- rtk proxy node --check frontend/src/xchat.js：通过。
- rtk proxy node --test frontend/src/capture-adapters.test.js：9 / 9 通过。
- rtk npm test：162 / 162 通过。
- rtk npm run build：通过（Vite 生产构建）。
- 首次全套运行出现一处旧测试仅预期 conversationId 的断言失败；新增 delay: 0 后更新对应断言，重跑通过。未忽略失败。

适配测试覆盖：同页无会话截图与原始尺寸、解码失败释放媒体轨道、不漂移的草稿会话归属、原生 stable pin ID / overlay 参数、主窗导入不消费独立编辑器、ready 事件订阅清理。

## 真实浏览器烟测

独立 Chrome headless context，在本地 HTTP origin 执行生产 xchat.js / capture-library.js / capture-renderer.js；使用真实 IndexedDB、Canvas、Clipboard API 和下载事件。

- 创建两个 40 × 20 PNG 贴图，两者记录与视图相互隔离。
- 第一张旋转 90°、缩放 200%、水平翻转、透明度 50%、关闭阴影；第二张保持默认原图、100% 与阴影。
- 第一张经 capture.pin.copy 输出至真实剪贴板，解码为 40 × 80 PNG。
- 第二张隐藏后恢复成功。
- capture.pin.save 触发实际 PNG 下载事件。
- 刷新页面后两张记录仍存在，可编辑 document 与 shadow=false 保留。
- 模拟 Native 建窗失败，命令执行前可以读到已存入的 document，失败后新建记录回滚删除，错误被明确返回。
- 销毁两张测试贴图后资料库记录数为 0。

## 会话竞争与延时取消补充

- Web 输出、取消按 sourceSessionId 清理，仅结束原来的捕获，不清掉更新的截图会话。
- 新增 patchCaptureRecord 原子 readwrite 事务：并发修改文档与窗口视图不会覆盖彼此；不存在的记录不会被补丁复活。
- 实际浏览器验证：并发修改 document / x / opacity 后字段全部保留；原生视图异步命令返回时新 document 仍保留。
- 恢复已存在的 Native pin 只更新显示状态，不向 pin_capture 重放旧 PNG，避免覆盖正在编辑的内容。
- capture.preferences 同步 delaySeconds 到原生，全局与按钮截图共用延时设置；capture.cancel-start 按 sessionId 取消倒计时。Web 取消后不编码、不打开编辑器、不保留媒体流，也不弹错误提示。

## 真实屏幕共享完整闭环

独立 Chrome headless 浏览器，生产 Vite 页面 http://127.0.0.1:1420/，另一张标签页标题为 XChat Capture QA Source；Chrome 自动选择该真实标签页作为 getDisplayMedia 来源（没有伪造 getDisplayMedia 返回图像）。

- 开始截图获取真实 1440 × 1000 PNG，video track 结束；编辑画布覆盖全视口，编辑层没有工作台 header。
- 切回 XChat 页并 Ctrl+A 后点击完成对钩，实际剪贴板 PNG 解码为 1440 × 1000。
- 完成后 Surface 数量为 0、cap-error 为 null、截图历史为 1 且尺寸一致、pageerror 为空。
- UI 选择 5 秒延时，第二次真实共享后点击倒计时取消：同 session_id 事件归零、Surface/倒计时均关闭、所有轨道 ended，历史仍为 1，无错误提示。
- 未发送任何聊天消息。

证据位于 C:/Users/wangy/.codex/tmp/xchat-capture-adapter-20261007/：smoke.json、race-smoke.json、real-display-media.json、real-countdown-cancel.json、restore-race-smoke.json。

原生桌面 UI、DPI、全局快捷键与多个系统窗口仍由主任务联调；浏览器权限按实际安全来源能力处理。
