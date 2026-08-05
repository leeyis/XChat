# XChat 截图编辑器复制功能设计

日期：2026-08-05

## 1. 背景

截图编辑器当前可以对选区添加矩形、椭圆、箭头、画笔、马赛克和文本标注，并可
将最终 PNG 钉图、另存或作为聊天附件完成。钉图窗口已经通过 Rust 后端和
`clipboard-rs` 支持复制图片，但普通截图编辑器工具栏没有复制入口。

## 2. 目标

- 在普通截图编辑器工具栏的“钉图”和“保存”按钮之间增加“复制”按钮。
- 复制当前选区及所有已提交或正在编辑的标注，剪贴板内容为 PNG 图片。
- 复制成功后关闭截图编辑器、清理本次截图临时文件并恢复聊天主窗口。
- 复制失败时保留编辑器和截图状态，在界面中显示可重试的错误。
- 支持当前桌面截图平台：macOS、Windows 和 Linux。

## 3. 非目标

- 不复制图片文件路径或文本形式的 data URL。
- 不改变“完成”按钮将截图加入聊天附件草稿的行为。
- 不改变钉图窗口右键菜单已有的复制功能。
- 不在钉图编辑模式的共享工具栏中增加新的复制行为。
- 不为 Android 或 headless Web 增加截图编辑器能力。

## 4. 方案

采用原生后端复制。前端继续使用截图编辑器现有的 `exportPng()`，将当前选区、原图
和标注合成为 PNG data URL，然后通过新的 workspace action 和 Tauri command 交给
Rust。Rust 复用现有 PNG 校验与 `clipboard-rs` 图片写入能力，把解码后的图片放入
系统剪贴板。

不采用 Web Clipboard API，因为 Tauri 在不同系统 WebView 中对图片剪贴板的权限和
实现存在差异。不借用钉图命令，因为那会创建或替换钉图状态和窗口，产生与复制无关
的副作用。

## 5. 界面与交互

- 新按钮位于“钉图”和“保存”之间，使用通用的复制图标。
- 中文 `title` 和 `aria-label` 为“复制”，英文为 “Copy”。
- 未形成有效选区或正在执行导出动作时禁用按钮，与钉图、保存和完成保持一致。
- 点击时先提交尚未失焦的有效文本输入，再导出 PNG。
- 复制期间沿用工具栏现有 busy 状态，避免重复提交。
- 成功后不额外显示提示，因为窗口立即关闭；失败时使用现有错误提示区域。

## 6. 数据流与生命周期

```text
复制按钮
  -> CaptureOverlay.exportPng()
  -> workspace.dispatch({ type: "capture.copy", dataUrl })
  -> desktop adapter: copy_capture_editor(dataUrl)
  -> Rust 校验活动编辑会话并解码 PNG
  -> clipboard-rs 写入系统图片剪贴板
  -> 清理 editor 状态和临时截图
  -> 关闭 capture-editor，恢复 main 窗口
```

后端只在系统剪贴板写入成功后清理会话。若解码、会话校验或剪贴板写入失败，命令
返回错误且不关闭窗口，用户可再次点击复制或选择其他完成方式。

## 7. 后端与权限

- 在 `capture_editor.rs` 增加普通编辑器截图复制入口，并将原生图片写入细节与现有
  钉图复制共享，避免两套平台行为漂移。
- 在 `commands.rs` 暴露新的 Tauri command。
- 同步更新 `main.rs`、`lib.rs` 两处命令注册。
- 在 `permissions/commands.toml` 新增最小权限，并只授予
  `capabilities/capture-editor.json`。
- 新 command 仅属于 desktop feature；Web capability 和 HTTP/WebSocket 协议不变。

## 8. 错误处理

- 无活动截图编辑会话时返回明确错误，不修改剪贴板。
- 非 PNG、超过现有限额或尺寸无效时沿用当前 PNG 校验错误。
- 剪贴板不可用或图片写入失败时返回具体错误并保留编辑器。
- 成功写入后即使窗口关闭操作异常，也确保截图状态和临时文件按现有关闭流程收敛，
  避免下次截图读取旧会话。

## 9. 测试与验收

自动验证：

- 前端 action 路由测试确认 `capture.copy` 调用 desktop adapter 并传递完整 data URL。
- Rust 聚焦测试覆盖 PNG 数据准备和活动会话校验中可独立测试的逻辑。
- 运行前端现有测试与构建、Rust library tests、desktop compile check、web compile check
  和 `git diff --check`。

手工验收：

1. 截取区域并添加至少一种标注。
2. 确认“复制”位于“钉图”和“保存”之间，提示文字正确。
3. 点击“复制”，确认编辑器关闭且聊天主窗口恢复。
4. 在聊天输入区或其他支持图片粘贴的应用中粘贴，确认内容为裁剪和标注后的图片。
5. 模拟剪贴板不可用时确认编辑器保留并显示错误。
6. 回归钉图复制、保存和完成截图功能。
