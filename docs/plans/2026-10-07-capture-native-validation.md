# XChat 原生截图与贴图实现记录

日期：2026-10-07。范围：Rust 原生窗口、截图、剪贴板、命令入口与授权；Web/React 交互由共享前端实现。状态：Rust 实现与逻辑验证完成；125% DPI 下多贴图实际交互已由主任务验证。当前会话真实抓屏仍被系统 API 拒绝，尚未通过端到端验收，见下方失败证据。

## 已实现的原生行为

- 每张贴图采用独立 UUID，窗口标签为 capture-pin-{uuid}。编辑保留 ID、会话与视图；创建第二张图不替换第一张。来自 main 的导入/恢复不会读取或消费另一个截图编辑器。
- 贴图窗口只能修改自身，main 管理操作必须显式给出 pinId。读取、复制、保存、隐藏、销毁均按窗口范围绑定。
- 旋转、翻转、缩放、透明度、阴影、穿透、隐藏、缩略与分组按图独立。关闭阴影同时保持无系统装饰。原生无装饰窗口不另加边框。
- 原生 x/y 为屏幕物理像素，普通 scale=1 按图片物理像素 1:1 显示；旋转 90/270 度交换宽高。缩略图等比限制在 160×120 逻辑像素。跨 DPI 时重新应用物理窗口尺寸，前端按当前 DPR 转换 CSS。
- 有效负屏坐标保留；恢复时若贴图完全不在可用显示器内，将移回光标显示器或主显示器。实际拔屏/混合 DPI 仍需硬件烟测。
- 组切换只改变窗口是否显示，不覆盖每张图的 hidden 值。F3 打开贴图管理；Shift+F3 切换当前组的隐藏状态，恢复时同时解除鼠标穿透。主窗也能解除任意图的穿透。
- 截图选择光标所在显示器，使用系统全屏窗口覆盖该显示器。抓屏前隐藏 XChat 主窗与工具窗，结束后按原可见、最小化和焦点状态恢复，独立截图不再无条件聚焦主窗。
- Windows/Linux 使用 xcap 枚举真实窗口区域并转换为截图像素坐标；不生成模拟控件候选。
- 原生屏幕抓取、图像剪贴板、文字剪贴板、保存对话框均在阻塞线程执行；延时使用可取消的异步倒计时，完成后才隐藏工具窗并冻结画面。
- 复制和保存可使用共享前端生成的变换后 PNG；保存取消返回 null。主窗的同页图片编辑可直接复制/保存，不关闭其他原生编辑器。贴图发送至聊天只生成草稿，不销毁其他窗口。

## 前后端合同

Tauri 命令参数用 camelCase；现有返回对象顶层保留 snake_case。PinView 字段用 camelCase。

| 命令 | 输入 | 返回或行为 |
| --- | --- | --- |
| start_capture_editor | conversationId?, delay?:0/3/5 | CaptureSessionSummary；省略 delay 读取同步的原生偏好 |
| get_pending_capture | 无，自动绑定调用窗口 | PendingCapture，包括 pin_id?, view?, regions |
| pin_capture | dataUrl, pinId?, view?, conversationId? | 稳定 session_id = pin_id；已有图保留当前 view；新图可传恢复视图 |
| update_pinned_capture | pinId?, view, overlay? | 最终 PinView；overlay=true 临时关闭阴影/穿透并抑制几何回写，overlay=false 恢复正常视图 |
| list_pinned_captures | main 专用 | [{session_id,pin_id,conversation_id,view}] |
| set_capture_pin_group | main 专用，group | 当前组名称 |
| set_capture_preferences | main 专用，delaySeconds?:0/3/5，captureCursor?:bool；省略项保持原值 | {delaySeconds,captureCursor,cursorSupported} |
| cancel_capture_start | main 专用，sessionId? | 仅取消未冻结倒计时；不影响现有 editor |
| resize_pinned_capture | pinId?, scale | 最终 scale |
| set_pinned_capture_shadow | pinId?, enabled | 成功或明确错误 |
| close_pinned_capture | pinId?, destroy | false 隐藏；true 销毁且只删除本图缓存 |
| copy_pinned_capture | pinId?, dataUrl?, scale? | 已传 dataUrl 时视为前端最终 PNG，不再二次缩放 |
| save_pinned_capture | pinId?, dataUrl? | {file_path} 或 null（取消） |
| read_capture_clipboard | 无 | {data_url?,text?}，优先图片、其次纯文本 |
| write_capture_text | text | 复制纯文本/颜色 |
| copy_capture_editor / save_capture_editor | dataUrl | capture-editor 完成相应动作；main 仅输出 PNG |
| finish_capture_editor | dataUrl | 按当前 editor 或 pin 的会话生成聊天草稿 |

PinView：x, y, scale(0.1–8), rotation(0/90/180/270), flipX/flipY(-1 或 1), opacity(0.15–1), shadow(默认 true), hidden, through, thumbnail, group(默认“默认”)。位置信息为原生屏幕物理像素，不是 Web 画布坐标。

事件：capture-countdown 携带 {remaining,session_id}，0 关闭倒计时；capture-pin-updated 携带 CaptureSessionSummary；capture-pin-view-updated 携带 {pin_id,view}；capture-pin-closed 携带 {pin_id,destroy:true}；capture-pin-group-updated 携带 {group}；capture-workspace 携带 {panel:'pins'}。新建、修改、OS 拖动与 DPI 变化均同步视图；临时菜单/编辑扩窗不写回持久视图。

所有新增命令已核对 main.rs / lib.rs 双注册与 commands.toml 权限。capture-pin capability 使用 capture-pin-*，主窗与两个截图窗口分别授予所需能力，不向 pin 开放其他 pin 的选择。

## 验证证据

初轮代码：

- rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib：通过。
- rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --bin lanchat：通过。
- rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features web --bin lanchat-web：通过。
- rtk cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib capture_：8 通过；覆盖窗口/ID 隔离、编辑不消费无关 editor、稳定 ID 与 view、过期修订拒绝、shadow 默认及序列化、参数校验、旋转和缩略尺寸、快捷键转换与注册回退。

最终默认配置：rtk cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib capture_，**11 通过，1 显式桌面测试忽略，152 过滤，0 失败**。增加了分组保留单图 hidden、DPI 2 下 100% 物理尺寸、带 session_id 的倒计时取消、旧 editor/旧修订不可清除后续会话的测试。

rtk git diff --check 通过。静态核对 19 个截图相关命令全部具有 main.rs / lib.rs 双入口注册及 commands.toml 定义。仅对两个截图 Rust 文件运行 rustfmt，commands.rs 只格式化截图命令片段，没有执行仓库级格式化。

异步输出结束使用 session_id + path 原子比较并取走源 editor；窗口销毁与主窗状态恢复也绑定创建时的 session_id，旧窗口回调不能清理或恢复后来的截图。


## Windows 实机抓屏与贴图验证补充

主任务使用独立 identifier、18888 端口与临时数据库运行 PC；没有停止用户安装版。125% DPI 下，以下操作已通过实际 Tauri/WebView2 验证：两张贴图独立编辑、移除阴影、鼠标穿透后由管理器恢复、隐藏后恢复、倒计时取消。重启后两张图 ID、文字文档、shadow=false、100% 时 320×140 原图物理尺寸仍保留（DPR 1.25）。恢复/定位操作的 60/80 位置为主动操作结果。

初轮真实屏幕读取失败，以下是排查历史；后续成功的完整流程见下方“真实抓屏补验”：

- 原 xcap 0.9.0 Windows GDI 抓屏返回“句柄无效 (0x80070006)”。
- 新增 Windows 备用路径使用 GetDC(NULL)、CreateDIBSection 和 BitBlt，在阻塞工作线程内按物理坐标读取真实桌面；DC、位图、DPI context 均按生命周期恢复/释放。只扩展已有 windows-sys 的 Gdi/HiDpi features，没有新增 crate，也不会生成替代画面。
- 新二进制端到端调用仍返回 BitBlt“拒绝访问 (os error 5)”。失败后 main 恢复可见，两张贴图恢复，没有遗留 editor。
- rtk cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib windows_gdi_captures_the_current_monitor -- --ignored --nocapture：已编译，但真实桌面测试失败（0 通过、1 失败、163 过滤），错误同为 BitBlt os error 5。测试随后更名 windows_captures_the_current_monitor，覆盖完整 grab_monitor_png 管线并增加非透明/非单色断言。该测试显式标记为需要可读取的交互式 Windows 桌面，普通逻辑回归不会默认抓屏。
- 只读 WTS 诊断：应用与测试进程同在 session 1，WTSActive=0、SessionFlags=1（已解锁），WinSta0/Default，inputDesktop=Default，WINSTA_READSCREEN 可访问；active console 是 session 4，WTSConnected=1、SessionFlags=0。因此没有证据将当前失败归因为应用会话断开或锁屏。尚未确认具体访问限制来源，也未修改权限、解锁或切换会话。
- 失败提示补充“当前桌面无法截图，请确认 XChat 所在桌面可见且允许截图后重试”，继续保留两条底层 API 错误，避免误报成功。

GDI 备用路径改动后，headless web check 再次通过；WGC 受控验证完成后切回生产默认 features，最终普通截图逻辑回归（--features desktop --lib capture_）为 11 通过、1 显式桌面测试忽略、152 过滤、0 失败。

WGC 只读评估：xcap 0.9.0 的 wgc 是上游正式可选 feature，默认仍为 GDI；它通过已有 windows 依赖扩展 API，不需新 crate。但其 D3D 初始化使用 expect、首帧超时 200ms、无运行时支持探测，新增系统 API 存在版本边界。本仓库 release 配置 panic=abort，因此 catch_unwind 无法消除上游初始化崩溃风险。[上游 PR](https://github.com/nashaofu/xcap/pull/257)、[CreateForMonitor 官方文档](https://learn.microsoft.com/en-us/windows/win32/api/windows.graphics.capture.interop/nf-windows-graphics-capture-interop-igraphicscaptureiteminterop-createformonitor)。经主任务授权，仅使用命令行 features 进行一次受控验证，生产默认没有改动。

受控命令：rtk cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop,xcap/wgc --lib windows_captures_the_current_monitor -- --ignored --nocapture。首次构建在新增测试的像素 slice 表达式发生类型错误，已修正为读取 pixel.0 数组，随后构建通过。实际运行仍为 0 通过、1 失败、163 过滤：WGC 返回 timed out waiting on channel，GDI 备用为 BitBlt 拒绝访问（os error 5）。没有获取到可验证的图像，故不启用该后端、不继续替换抓屏 API，也不把超时解释为明确的权限拒绝。


最后默认配置编译检查全部通过：

- desktop --lib：通过，48 crates，1m25s。
- desktop --bin lanchat：通过，1 crate，14.05s。
- web --bin lanchat-web：通过，1 crate，4.34s。

三条完整命令均为 rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features，加上上述 feature 与入口；未启用 xcap/wgc。上述编译/逻辑测试不替代下述真实抓屏补验。

## 2026-10-07 真实抓屏补验

用户允许搜索GitHub后，查阅了[类似桌面访问报告](https://github.com/openai/codex/issues/32637)和[Microsoft远程桌面截图说明](https://learn.microsoft.com/en-us/troubleshoot/power-platform/copilot-studio/actions/computer-use-screenshot-error)。只读探针报告remoteSession=true、WinSta0/Default、input desktop接收输入且外部GetCursorPos成功；没有改变会话或安全设置。没有证据把本机失败归因为这些报告中的特定原因。

- 生产默认features下显式重跑windows_captures_the_current_monitor：1通过、0失败、163过滤，2.51秒；尺寸、非透明、非单色与PNG解码断言通过。
- 随后cargo tauri dev --no-watch构建并启动同一binary，应用内GetCursorPos和抓屏仍失败；再停止这个QA实例，直接运行K:\cargo\debug\lanchat.exe，沿用18888端口、同一QA数据库、WebView2目录和127.0.0.1的devUrl，鼠标位置与真实抓屏均成功。此间未修改抓屏代码。
- 真实1920×1080屏幕在DPR 1.25下完整铺入1536×864编辑视口，无顶部工作台。640×320 CSS px选区输出800×400原图px；Enter换行、外部点击提交、文字边框48×24 CSS px精确移动、箭头滚轮4px、30px对钩复制通过。
- 实际系统剪贴板图像解码为800×400，与历史输出逐RGBA通道比较，差异为0。PNG编码字符串可能不同，因此未使用base64字符串相等作为像素断言。
- 另一轮真实440×240选区直接贴图，产生新稳定ID与独立窗口；窗口440×240物理px、352×192 CSS px，默认阴影开启，源图/选区文档保留，另两张贴图ID仍存在。
- 完成截图后编辑窗关闭，主窗恢复；原生工具条视觉单独截图核对通过。

因此Windows真实截图到输出闭环已通过。开发启动链路与直接启动的差异仍未定位：本机Tauri CLI 2.9.4、SharedChild 1.1.1及RTK 0.43.0的相关启动代码没有发现主动切换desktop的逻辑；标准流继承及父进程链不同不足以证明根因。不得表述为已经修复Tauri/RTK缺陷。元数据保存在临时QA目录的native-real-capture-e2e.json与native-real-pin-e2e.json。

## 鼠标指针设置补齐

鼠标指针设置补齐：该开关已存在于批准原型。默认关闭；开始截图时原子读取延时与指针偏好。Windows使用真实系统光标句柄，按物理屏幕坐标减去热点后合成到抓到的图像；复制句柄、位图、DC和线程DPI上下文均释放/恢复。[GetIconInfo](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-geticoninfo)、[DrawIconEx](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-drawiconex)。macOS传递系统-C选项；Linux返回cursorSupported=false，显式开启会报错且不部分写入延时。

本次补齐后的最终capture_editor::tests为12通过、2实时探针默认忽略；desktop lib/bin与web bin检查通过。windows_composes_the_live_cursor_on_a_captured_frame另行显式执行为1通过、167过滤（4.26秒），验证真实捕获帧加入实时光标后的像素变化。普通单测同时检查系统箭头像素绘制、背景不变、负坐标显示器及热点裁剪；不会移动系统指针。

最后通过cargo tauri dev重新构建并在隔离PC界面实测：开关可用，启用后原生与本地偏好均为true；“开始截图”按钮取得1920×1080真实图，400×280选区对钩复制、历史保存、编辑窗关闭和主窗恢复均通过。这轮开发启动方式也成功，不能把此前差异定性为CLI缺陷或固定启动限制。后续QA实例退出，未覆盖用户安装版。

## 尚不能宣称完成的基线能力

| 基线 | 当前实现边界 |
| --- | --- |
| C02 | 已选择光标显示器并保留物理负坐标；不提供跨屏联合选区，混合 DPI、负坐标显示器、拔屏找回尚无真实多屏硬件烟测证据 |
| C03 | 真实窗口识别已实现；未接入系统可访问性控件树，因此不宣称任意应用控件级自动识别 |
| C06 | 已提供可取消的 0/3/5 秒倒计时；按钮和全局截图遵循同一偏好，取消不消费原有编辑会话；原生倒计时取消已通过主任务烟测 |
| C07 | 已加入可配置的Windows真实光标合成与macOS系统-C参数；Windows实时探针通过，macOS未实测、Linux明确不可用；白板由共享前端负责 |
| C15 | 原生复制、另存为已实现；快速保存、自动保存、打印未扩展原生合同 |
| C16 | 原生读取图片或纯文本；HTML、文件路径、GIF 帧控制以及更多格式解码不由当前原生命令提供 |
| C18 | 变换已交给共享前端同一导出管线；原生窗口无边框拖拽缩放，由已批准的滚轮/菜单交互控制 |
| C20 | F3 提供可找回隐藏/穿透图的管理入口；不承诺直接恢复最近一张的快捷键行为完全相同 |
| C21 | 持久图片与文档/视图由共享前端 IndexedDB 保存，Rust 维护本次运行的独立窗口状态；不另复制大图持久层 |
| C22 | 原截图快捷键保留注册失败回退，F3/Shift+F3 注册失败会报日志；系统被其他应用占用时不能强占快捷键 |
| C23 | 未添加系统命令行截图/贴图/输出入口 |
| C24 | 未实现 WASD 移动系统光标；绘制滚轮、双击和中键由共享前端处理 |

Windows 为本次编译环境；macOS、Linux 与移动端未在本机交叉编译或实测。移动端仍被原生截图 cfg 排除，headless web 编译不依赖 Tauri 截图模块。

## 参考核对

- [Tauri WebviewWindow 官方 Rust API](https://docs.rs/tauri/latest/tauri/webview/struct.WebviewWindow.html)：窗口定位、尺寸、阴影、穿透与显示器事件；具体实现另核对本机锁定的 Tauri 2.10.2 源码。
- [clipboard-rs 0.3.3 Clipboard API](https://docs.rs/clipboard-rs/0.3.3/clipboard_rs/trait.Clipboard.html)：图片与纯文本剪贴板；PNG 转换核对本机同版本 common.rs。
- xcap 0.9.0 已安装源码 window.rs：真实窗口几何枚举，仅提供窗口区域，不提供控件树。
- [Microsoft WTS 连接状态](https://learn.microsoft.com/en-us/windows/win32/api/wtsapi32/ne-wtsapi32-wts_connectstate_class) 与 [WTSINFOEX_LEVEL1_W 锁定状态](https://learn.microsoft.com/en-us/windows/win32/api/wtsapi32/ns-wtsapi32-wtsinfoex_level1_w)：仅用于解释只读会话探针结果，未新增生产会话控制代码。
- [GetDC](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getdc)、[CreateDIBSection](https://learn.microsoft.com/en-us/windows/win32/api/wingdi/nf-wingdi-createdibsection) 与 [BitBlt](https://learn.microsoft.com/en-us/windows/win32/api/wingdi/nf-wingdi-bitblt)：Windows 备用抓屏路径 API 和资源生命周期。
