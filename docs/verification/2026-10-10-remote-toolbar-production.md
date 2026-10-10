# 远控工作台与浮动工具栏生产验收

基线为已批准的 `60240ba` 原型。共享方、协助方的正式 React 界面已按该原型实现，包括标题与权限区、双向通话条、沟通侧栏、显示/画质操作、暂停提示，以及两种角色的浮动工具栏。

## 已实现的操作

- 拖动六点手柄调整位置；方向键移动 8px，Shift 加速至 24px，Home 复位。面板随可用区域变化约束位置。
- 隐藏后保留状态浮标，浮标可独立拖动；展开恢复完整面板的位置。同一会话离开再打开保留状态，新会话默认展开。
- 共享方浮标保留收回控制、结束；协助方浮标保留结束。收起不暂停画面、不释放授权、不挂断语音。
- 原生共享条使用相同组件，缩小为可见置顶浮标，主工作区与原生浮标同步折叠。窗口本身不调用 hide/minimize，保持原有授权可见性约束。
- 主动允许对方控制必须经本机确认，提交时校验会话 revision；旧确认框不能恢复已撤销的控制。暂停、撤权、结束继续复用共享 Rust 状态机。
- 主导航可切回聊天，进行中的协助通过浮动入口恢复；Esc 可离开工作区，全屏时先由浏览器退出全屏。

## 验证结果

| 验证 | 结果 |
| --- | --- |
| 仅包含本次改动的提交快照 `rtk npm test` | 193 项通过 |
| 工作树与独立提交快照 `rtk npm run build` | 均通过，57 个模块 |
| `cargo test --no-default-features --features desktop --lib remote:: -- --test-threads=1` | 9 项通过，208 项过滤 |
| `cargo test --no-default-features --features web --lib remote:: -- --test-threads=1` | 9 项通过，181 项过滤 |
| `cargo check --no-default-features --features web --bin lanchat-web` | 通过 |
| 隔离 `rtk cargo tauri dev` | 编译与启动通过，独立标识、端口 18923、临时数据库 |
| 独立 Chrome 像素/交互验收 | 24 组对照、9 项交互，外加 2 个视觉汇总断言，零未捕获异常 |
| 原生 WebView2 与 Chrome 实际联调 | 14 项通过，三窗口零未捕获异常 |

Cargo 命令均从仓库根执行并带 `rtk`、`--manifest-path src-tauri/Cargo.toml`。桌面构建仍有既有 lib/bin 同名 PDB 提示，无新增编译错误。

像素对照覆盖共享方/协助方、展开/浮标、浅/深主题、1280/860/390px。使用同一 Chrome、相同容器尺寸，屏幕内容背景和裁剪起点统一后比较实际生产组件与批准 HTML：全部元素的边界、字体、间距、颜色、圆角和 SVG 样式一致。24 组图片尺寸一致；以单像素 RGB 通道差值大于 8 计，差异像素比例最高 **0.1788%**。不能把这一结果表述为所有图片逐字节相同，也不能将工具栏局部比较等同于真实远程桌面整屏比较。完整工作台另存截图并人工对照标题、通话条及整体布局。

原生联调使用真实 xcap → JPEG → WebRTC 屏幕链路，成功获得 9 次原生帧和 7 帧接收解码，画面像素仅在内存中使用。实测 125% 缩放下折叠、方向键移动、位置恢复、窗口越界回正、快速切换、主窗口同步、收回旧授权、主动授权、重新申请、暂停/恢复及结束清理；浮标折叠期间帧继续增长且麦克风、授权保持。证据截图仅包含控制窗。

## QA 修复与边界

- 修复生产全局 `hidden` 样式缺失造成两套工具同时显示的风险。
- 按原型修正窄窗口按钮高度及共享状态换行；恢复原型标题区按钮数量与间距。
- 修复原生居中计算产生小数坐标而被 Tauri `i32` 位置参数拒绝的问题，测试夹具现在拒绝非整数坐标。
- 原生尺寸调整串行合并，忽略已销毁实例的排队操作；旧轮询不能覆盖新会话版本。
- 验证脚本曾有缩进、缺省错误字段读取问题；独立提交快照首次遗漏既有测试所需的 Rust/UI 文件。这些均已修复后通过。原生边界断言按 CSS 整数尺寸的一个像素取整误差处理物理缩放，避免把测量取整当成越界。

本次没有重新验证 Android/macOS/Linux、跨物理显示器拖动、真实触摸、物理双机网络和真实麦克风/扬声器。原生键鼠授权用 no-op keepalive 检查，未向日常应用注入键鼠。鼠标拖动由独立 Chrome 的真实 Pointer Events 覆盖；原生窗口的尺寸、坐标、键盘移动和边界在 WebView2 实测。

截图、编辑、贴图、快捷键的源码及资源未改。自启动/群在线状态等其他工作流保持原样，不混入本次 Git 提交；提交用的生成资源从 HEAD 加本次源码单独构建。

## 可复跑证据

- [视觉和交互脚本](2026-10-10-remote-toolbar-production.py)、[结果](2026-10-10-remote-toolbar-production.json)，配合生产组件专用 [QA fixture](2026-10-10-remote-toolbar-fixture.jsx)。原型预览服务根目录为 `ui-ref`、端口 18893。
- [原生联调脚本](2026-10-10-remote-toolbar-native.py)、[结果](2026-10-10-remote-toolbar-native.json)、[控制窗截图](2026-10-10-remote-toolbar-native.png)。脚本接收隔离 Tauri 启动状态 `--state` 和 Web 二进制 `--binary`。
- [共享方工具栏](2026-10-10-remote-toolbar-production-host.png)、[协助方工具栏](2026-10-10-remote-toolbar-production-viewer.png)。工作台截图采用 QA 会话，生产图中屏幕区域为空白占位，不是网络传输失败。

原生几何转换依据 [Tauri Window API](https://v2.tauri.app/reference/javascript/api/namespacewindow/) 的 LogicalSize / PhysicalPosition、monitor workArea 与缩放语义。
