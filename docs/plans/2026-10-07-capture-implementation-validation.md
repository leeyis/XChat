# XChat 截图与贴图双端实施验收

日期：2026-10-07。依据用户已批准的 v0.7 原型实施。已完成 Web/PC 共用编辑器、双端适配与工作台。Web 真实捕获、Windows 真实屏幕选区与复制/贴图、原生多贴图交互均已验收。Windows 验收覆盖直接启动及后续cargo tauri dev启动；此前间歇性抓屏拒绝访问仍保留为未定位因素，不宣称已修复其根因。

## 生产实现

| 范围 | 结果 |
| --- | --- |
| 全屏选区 | 真正来源图像铺入全视口，截图态无工作台栏；八向调整、移动、Ctrl+A、像素微调与上次选区，标注保留原图坐标 |
| 文字 | 多行、Enter 换行、外部单击完成、单击重编、4 CSS px 边框拖动阈值、Delete 删除与最新内容撤销、字体/颜色/字号保留插入点 |
| 文本定位 | 输入与 Canvas 共用字体、行高和实际基线；边缘输入框避让不改变原锚点；等比显示、裁剪与导出统一使用原图像素 |
| 工具与视觉 | 12 个工具含选区、矩形、椭圆、线、折线、箭头、画笔、荧光笔、文字、马赛克、模糊、橡皮；11 类参数滚轮调节；轻色 20 px 统一图标、同尺寸对钩完成 |
| 多贴图 | 稳定 ID 与可编辑文档；旋转、翻转、缩放、透明度、缩略图、独立阴影、隐藏/恢复/销毁、分组及找回；提示仅悬浮显示 |
| 来源与管理 | 真实文件/剪贴板/文字/色卡、白板、可编辑历史、0/3/5 秒可取消倒计时、尺寸预设、快捷键预设、存储恢复与历史上限；原型已有的鼠标指针开关按平台能力开放 |
| Web | getDisplayMedia 真实取图、同页全屏编辑、实际 PNG 剪贴板与下载、页面内多贴图、IndexedDB 持久化；成功/失败/取消释放媒体流 |
| Windows PC | 光标所在显示器抓屏、真实窗口边界、独立置顶透明窗口、系统阴影/穿透、物理像素 100% 与 DPI 处理、原窗口可见/焦点恢复 |
| 会话安全 | 旧异步输出不清理新截图；pin 更新不消费别图/别的编辑器；视图原子补丁不覆盖新文字文档；来源会话固定，仅加入草稿不自动发送 |

## 可复核验证

- 前端最终 171/171 条回归通过（原有147条、坐标6条、适配16条、来源解析2条）；Vite 生产构建通过。截图/贴图入口315.33 kB，聊天App分块188.98 kB，截图窗口不请求该App分块；CSS为121.34 kB。
- Chromium 2倍设备像素比实测：中文输入态与提交态红色字形边界、像素数完全一致。Enter/Backspace/IME事件保护、外部单击只提交、Delete 后撤销、反复编辑、字体/字号插入点与拖动阈值逐项检查。
- 100%、125%、150%、200% 源密度模拟都保留源锚点；边缘输入控件留在屏幕内，不由避让位置回写原坐标。此项不替代真实系统多显示器 DPI 验收。
- 11种工具滚轮增减、绘制中参数同步通过；马赛克与模糊在真实 Canvas 中改变区域像素，擦除保留底图及邻近标注。
- 实际 getDisplayMedia 从另一标签页获取1440×1000图像，对钩复制同尺寸PNG并关闭编辑器、生成可编辑历史；流均ended，无页面错误。实际5秒倒计时取消不生成截图或历史，流正确释放。
- 指针偏好补齐后再次真实Web验证：当前Chrome未advertise cursor约束，开关禁用并说明能力限制；默认false与历史true两种偏好均继续通过1440×1000真实标签页捕获、PNG复制、关闭编辑器和释放媒体流，不伪造光标。支持约束的环境会核对所选源的能力和实际模式，无法兑现时返回错误。
- WEB 两图独立视图、无影无框、hover-only提示、文本重编辑后稳定ID、真实PNG下载/剪贴板、刷新恢复、分组、窄屏菜单及非法输入保留通过。
- Windows 125% 缩放实测多图独立编辑、菜单扩窗/恢复、无影无框、鼠标穿透恢复、隐藏恢复及3秒倒计时取消通过；重启恢复稳定ID、可编辑文档与阴影设置，320×140图片保持320×140物理窗口（256×112 CSS px）。
- Windows 真实抓屏后续通过：同一二进制直接启动后，1920×1080 原图完整进入1536×864 CSS视口（DPR 1.25），无顶部说明/工作台。选区640×320 CSS px导出800×400原图px；多行中文、框外提交、边框移动48×24 CSS px（原图60×30px）、滚轮箭头4px、30px对钩均通过。实际系统剪贴板解码后与历史导出图像逐RGBA通道一致，差异为0；完成后编辑窗关闭、主窗恢复。
- Windows 真实选区440×240输出独立贴图，原生窗口440×240物理px、CSS 352×192；默认阴影开启，可编辑源文档保留。另两张贴图ID保留，截图输出未替换它们。
- 鼠标指针补齐后的PC实际界面验证通过：开关可用，开启后原生与本地偏好均为true；从“开始截图”按钮取得1920×1080真实画面，400×280选区经对钩复制成功，生成历史并恢复主窗。此次cargo tauri dev启动正常，说明此前启动方式的对照不足以确定原因；仍保留原始失败记录。
- Windows 初次开发启动路径仍有失败记录：xcap 0x80070006、GDI BitBlt OS error 5，错误后窗口恢复。独立实时测试随后通过，但重新经cargo tauri dev启动仍失败；直接运行同一binary则成功。该对照没有改变产品代码、系统权限或桌面会话，不能据此认定Tauri/RTK缺陷或唯一根因。WGC受控实验200ms未收到帧，生产未开启该后端。
- Rust截图编辑器最终14条测试通过、2条真实桌面测试默认忽略；首次相关回归还覆盖了2条快捷键测试。真实抓屏测试已显式通过（2.51秒），检查尺寸、非透明及非单色像素；实时光标测试也显式通过（4.26秒），验证真实屏幕帧加入实际系统指针后的像素变化。性能优化新增PNG透明像素往返、指针裁剪范围回归，56种系统指针与边缘组合另经逐像素对照。最后desktop lib/bin 与 web bin 编译检查通过；19项命令的双入口注册与ACL核对通过。
- 最终性能构建再次真实验证2560×1440屏幕、125%缩放：640×320 CSS选区输出800×400 PNG；Enter换行、框外提交、边框移动40×20 CSS px对应50×25源像素，实际系统剪贴板与历史PNG的RGBA差异为0。随后550×300源图生成独立贴图，原生客户区为550×300物理像素，HTML/body/root背景均透明。
- 独立入口在DEV和生产静态产物中均完成截图、贴图、主界面、提醒界面验证；真实卸载重挂、贴图更新事件和过期来源竞争通过。该浏览器夹具模拟IPC，只用于入口/状态回归，不计作原生启动性能证据。

详细证据：

- [双端适配验证](2026-10-07-capture-adapter-validation.md)
- [工作台与贴图验证](2026-10-07-capture-widgets-validation.md)
- [原生合同与验证](2026-10-07-capture-native-validation.md)
- [截图启动性能与测量边界](2026-10-07-capture-startup-performance.md)
- [0.1.11发布前验证状态](2026-10-08-v0.1.11-release-validation.md)

## 基线范围与仍未覆盖的能力

本次实现不等同于全部参考软件基础能力已经逐项复刻。以下没有用演示数据或无效按钮代替真实能力：

| 项目 | 状态 |
| --- | --- |
| 原生多显示器 | 按光标显示器捕获、负坐标及DPI逻辑已实现；多屏/混合DPI硬件实测尚未完成，跨屏联合选择未做 |
| 系统控件自动识别 | 目前为可枚举的真实窗口边界；没有可访问性控件树检测 |
| 光标合成、WASD系统光标 | Windows真实指针合成已实现并通过实时探针；macOS接入系统参数但未实测；Linux不可用。Web按浏览器真实约束能力控制。WASD系统光标移动仍未实现 |
| 文字旋转与角点缩放 | 批准原型以文字边框拖动及字号滚轮为主，本次未扩展该交互 |
| 输出扩展 | 当前复制、另存为、贴图、聊天草稿完成；打印、快速/自动保存尚未实现 |
| 其他来源 | 当前使用浏览器支持的静态图片解码；HTML/文件路径转换、TIFF/TGA全量支持和GIF帧控制尚未实现 |
| 命令行入口 | 未扩展系统CLI截图/贴图自动化命令 |
| Web原生窗口能力 | Web贴图在当前页面内；系统置顶、跨应用鼠标穿透仅PC支持 |
| 平台验证 | Windows真实抓屏、编辑、复制及贴图实际运行已验收；本轮曾捕获远程桌面不接收输入的成对证据，用户恢复可见后恢复抓屏，不反推此前所有访问异常同因。macOS/Linux未运行，移动端不纳入桌面截图范围 |

这些是基线后续工作，不应在产品或发布说明中宣称已完成。除平台限制外，涉及新的可见交互仍应依照仓库原型评审规则先展示；当前已批准的交互按v0.7落地。

## 验证运行说明

- Windows调试实例使用独立标识 `com.xchat.capture-qa`、端口18888、单独数据库和WebView2目录；不终止或改动用户的已安装实例。
- Ctrl+Shift+A、Shift+F3被既有实例占用，未宣称本轮完整验证全局按键触发；相同按钮/命令路径与快捷键单测通过。
- QA启动初次误连到localhost的另一IPv6服务，使用仅位于临时目录的127.0.0.1 devUrl配置纠正；生产配置未改。
- 受控WGC实验测试未通过；正常自动化回归和生产构建不启用它。实际PC闭环已经使用生产默认后端完成，未以夹具或模拟画面替代。
- 查阅[GitHub类似报告](https://github.com/openai/codex/issues/32637)、[Microsoft远程桌面截图排查](https://learn.microsoft.com/en-us/troubleshoot/power-platform/copilot-studio/actions/computer-use-screenshot-error)及[GetCursorPos调用条件](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getcursorpos)。这些是诊断线索，不是本机根因证明。只读探针报告remoteSession=true、WinSta0/Default且input desktop接收输入；未修改权限、锁屏或会话。
- 本轮真实PC验收元数据位于临时QA目录的native-real-capture-e2e.json、native-real-pin-e2e.json、native-cursor-e2e.json；native-real-toolbar.png仅记录工具条视觉，不将整个屏幕图像加入仓库。
- 性能构建的最终真实回归元数据为同目录native-final-functional-copy.json和native-final-functional-pin.json；完整首帧计时另外记录系统负载及超时，不能与组件微基准混用。

## 本地检查命令与预览

```text
rtk npm test
rtk npm run build
rtk cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib capture_
rtk cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib windows_captures_the_current_monitor -- --ignored --nocapture
rtk cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib windows_composes_the_live_cursor_on_a_captured_frame -- --ignored --nocapture
rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib
rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --bin lanchat
rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features web --bin lanchat-web
rtk git diff --check
```

当前WEB开发预览为 http://127.0.0.1:1420/ ，使用现有Vite服务；截图入口右键/F3可打开贴图管理。原型参考仍位于ui-ref，生产实现位于frontend/src。2026-10-08对照结束后已关闭旧版本，恢复运行最终优化的Windows隔离QA实例，端口18888、标识com.xchat.capture-qa；本轮未替换用户安装版。桌面构建另有项目原有的lib/bin同名PDB输出提示，未为此重命名目标。
