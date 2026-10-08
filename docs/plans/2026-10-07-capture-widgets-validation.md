# 截图工作台与贴图界面实施验证

日期：2026-10-07。范围为已批准 v0.7 原型中的工作台、贴图、来源、记录与设置交互；编辑器内核、双端适配与 Rust 系统能力由对应实现记录补充。

## 实现

- `CaptureWorkspace.jsx` 挂在主界面，接收截图、工作台、资料库及倒计时事件。WEB 贴图保留在当前页面，主界面可独立开启截图，不需要存在聊天会话。
- 来源支持真实剪贴板图片/文字、本地可解码图片、纯文字和六位 HEX 色卡。文字来源保留文字标注对象，回放后能继续原位编辑。文件来源规范为 PNG；GIF 使用静态帧，不宣称支持动图控制。
- `CapturePin.jsx` 共享贴图显示与右键操作。滚轮缩放，Ctrl/Cmd 加滚轮调透明度，1/2 旋转，3/4 翻转，Shift 加双击缩略，双击/Esc 隐藏，Shift+Esc 销毁，Space 标注。右键提供原始/当前图像复制、保存、变换、分组和阴影。WEB 不显示系统鼠标穿透选项。
- 阴影默认开启，每图独立保存；关闭后所有选中/聚焦状态均无边框、outline、box-shadow 和 filter。左下提示只在悬浮时显示。没有最小化、更多或文字拖动/删除独立按钮。
- 管理支持历史回放、分组过滤、显示/隐藏本组、找回屏外贴图及恢复鼠标交互。隐藏反馈有直接恢复入口。`remember=false` 下重新打开不会自动显示已存贴图，资料库仍保留并可手动恢复。
- `CaptureEditor.jsx` 使用共享编辑器。PC 贴图的源图/标注文档来自原生 pending 与 IndexedDB；原生 pending 的 view 优先。缺少旧记录时先建立记录，再允许操作。
- UI 沿用批准原型的浅色、统一描边图标。工作台没有原型模拟密度、演示桌面、评审说明或竞品文字。截图编辑时工作台不显示。

## 平台契约

- Native PinView 的 x/y 是桌面物理像素。scale=1 表示一个源图像素对应一个屏幕物理像素；PC 显示 CSS 尺寸为原始尺寸 × scale / 当前 scaleFactor。窗口 resize 后重读 scaleFactor。WEB 保持当前页面 CSS 坐标。
- 缩略图限制 160×120 CSS px，CSS 比例为 `min(view.scale / DPR, 160 / rotatedWidth, 120 / rotatedHeight)`。
- 小贴图右键菜单临时扩窗：`capture.pin.update {overlay:true}` 抑制原生位置回写，然后按显示器物理边界扩窗，使用内部偏移保持图像原屏幕位置。关闭时 `overlay:false` 恢复存档几何与阴影。标注临时扩至当前显示器，退出时恢复贴图。
- `capture.group` 切换实际显示分组，原生不改每图 hidden 标记。Native F3 打开工作台，以便恢复隐藏和穿透状态。
- 倒计时使用 `capture-countdown` / `xchat-capture-countdown`，字段 `{remaining,session_id}`。取消发送 `capture.cancel-start {sessionId}`；过期会话的 0 事件不关闭新倒计时。
- view/title 更新使用 `patchCaptureRecord` 单事务更新，避免拖动事件覆盖并发的新标注文档。贴图 view 写串行处理并保留最新的乐观视图，失败回退至最后已提交值。
- Native 连续位置事件由主窗口按 pinId 做 150 ms 尾沿防抖后持久化；贴图窗口只实时显示事件数据。资料库通知仅读取 IndexedDB，首次载入及图像内容更新才读取 Native pending PNG。

## 已执行验证

使用本机 Chrome 与现有 Playwright，1440×1000、900×700 视口，设备像素比 2；独立浏览器上下文，仅操作截图资料与系统图片剪贴板。

| 流程 | 结果 |
| --- | --- |
| 真实文字来源创建、两行内容留存 | 通过，原始文字作为可编辑 operation 存储 |
| 悬浮提示 | 移出 opacity=0，移入 opacity=1；选中不常驻 |
| 阴影关闭 | filter=none、border=0、outline=0、box-shadow=none |
| 移动与视图变换 | 拖动 +90/+55，90° 旋转、水平翻转、滚轮 1→1.1、Ctrl 滚轮透明度 1→0.95 均正确保存 |
| 隐藏与恢复 | Esc 隐藏、反馈直接恢复；工作台保持打开，不被同一次 Esc 误关闭 |
| 分组切换 | 从默认移到设计参考后在默认组消失，切到设计参考恢复显示 |
| 贴图文字再次编辑 | 两行原文可读取，修改后仍为相同 pinId；位置、旋转、翻转、透明度、阴影、分组均保留 |
| 多来源、多图独立 | HEX 输入无效时保留输入并报错；有效色卡与本地 64×32 PNG 创建成功；各图阴影独立 |
| 真剪贴板 | 通过右键复制色卡，系统图片剪贴板实际为 330×220 PNG |
| 页面重载 | 已存贴图恢复；关闭恢复偏好后重载自动显示 0 张，管理面板可恢复 |
| 菜单键盘 | 窗口阴影可用 Space 切换，不误触发 Space 进入标注；菜单方向键可移焦点 |
| 小图/窄屏菜单 | 900×700 下菜单 x109/y186/w234/h440，完整位于视口内；页面无脚本异常 |
| WEB 倒计时事件联调 | 使用 Canvas MediaStream 替代系统来源选择，真实经过 WEB adapter：显示 3 秒、Esc 取消后全部 track=ended、工作台保留且未开启编辑器；不是浏览器系统来源选择授权验收 |
| 键盘与鼠标补齐 | + 缩放 1→1.1、箭头 x/y 每次移动 1；中键恢复 scale=1、opacity=1、thumbnail=false 并保留位置；pointercancel 撤回未完成拖动 |
| RGB/HEX 来源 | 颜色对话框接受 rgb(24,172,113)，系统剪贴板 #f0a 创建 #FF00AA 色卡；两处复用同一解析及渲染路径 |
| Native 事件隔离夹具 | 20 次位置事件产生 1 次 IDB put、1 次资料库通知；Native pending 只在首次读 1 次；最终位置正确。DPR=2 时 320×200 原图显示为 160×100 CSS px。使用接口夹具，不替代真实桌面验收 |

目视截图：`C:/Users/wangy/.codex/tmp/xchat-capture-widgets-implementation/workspace-900-menu.png`。

执行 `rtk npm test`：159 项通过，无失败。执行 `rtk npm run build`：生产构建通过；后续全局验收可能增加测试项，以主任务最终记录为准。

收尾增加 `capture-sources.test.js` 的 2 项解析测试并单独运行通过：规范化 HEX/RGB/百分比通道，拒绝普通说明文字与越界/无效通道。没有为此次收尾重新生成构建产物，由主任务统一构建。

主任务修复 BroadcastChannel 同页回声后，重新运行 Native 事件夹具通过：`reads=1`、`writes=1`、`notifications=1`、`sharedWindowContext=true`、无页面异常。夹具特意同时载入 Vite 带时间参数和不带参数的资料库模块，验证同一窗口的 HMR 模块实例共享 contextId，不产生额外通知。最终证据：`C:/Users/wangy/.codex/tmp/xchat-capture-widgets-implementation/native-events-check.json`。

## 待主任务平台验收

本分支的浏览器结果不能替代 PC 原生窗口验证。真实多窗口、系统阴影、鼠标穿透恢复、原生保存取消、系统抓屏、混合 DPI/负坐标与显示器切换由主任务使用隔离数据目录验收。macOS/Linux 未在此 Windows 浏览器检查中验证。未展示于批准原型的 GIF 帧控制、HTML 贴图、系统打印等能力不在此分支假装为可用选项。
