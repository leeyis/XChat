# 截图启动性能验证

日期：2026-10-07 至 2026-10-08。用户反馈快捷键和按钮触发截图均有约两秒等待。本次优化共享启动路径，保持已批准的界面、原图分辨率和无损 PNG。

## 已实施

1. Windows/Linux 原图编码使用 PNG Fast + Sub，避免截图启动前对每一行尝试多种滤波器。原实现本身已使用 Fast 压缩，主要成本是 Adaptive 滤波，单纯改为 Fast 不会解决问题。
2. Windows 指针合成仅建立与画面相交的指针小块 DIB，仍由系统 DrawIconEx 处理彩色 alpha 和单色 AND/XOR 指针，保留热点和边缘裁剪。原实现对整幅画面执行两次通道转换。
3. React StrictMode 的效果重放共用当前挂载的 pending 请求，在失效检查后才解码/复制图片；切换来源时旧响应不能写入新画面。贴图初次读取也仅复用当前挂载的请求，更新事件仍重新读取。真正卸载后重新挂载读取新截图，没有跨截图的全局图片缓存。
4. 环境变量 `XCHAT_CAPTURE_TRACE=1` 可输出抓屏、指针合成、PNG 编码、文件读写、窗口创建、pending 传输的阶段耗时，失败路径也保留计时。默认关闭，不记录截图内容、窗口标题或图片路径。
5. 截图和贴图入口直接渲染原 CaptureEditor，其他视图动态加载 App。截图窗口免加载7个聊天/工作台源码模块（合计约289 KiB源码；不能当作生产压缩包大小），保持原路由、样式、语言和透明背景逻辑。
6. 开发构建仅将 lanchat 设置为opt-level=1，将xcap/image/png 0.18.1/fdeflate/simd-adler32/crc32fast设置为opt-level=2。调试符号、断言和溢出检查继续使用原默认值，发布构建配置保持原样。

按钮与原生快捷键均进入 `capture_editor::start`。本轮没有通过提前返回、未完成的截图占位或缩小图像改变计时终点。

## 同图微基准

Windows、1920×1080 实际截图样本、image 0.25.9 / png 0.18.1，使用现有 debug 依赖，连续三次。表中为中位数；这些是组件计时，不能当作完整启动时间或发布版本速度。

| 阶段 | 优化前 | 优化后 | 正确性 |
| --- | ---: | ---: | --- |
| PNG 编码 | 1858.75 ms | 428.51 ms | 解码后 RGBA 逐像素一致 |
| Windows 指针合成 | 788.45 ms | 0.50 ms | 8种系统指针 × 7种正常/边缘/屏外位置，共56例逐像素一致 |
| StrictMode 初始化 | 2次读取 / 2次解码 / 2次整图复制 | 各1次 | 换源、旧解码返回、真正重新挂载检查通过 |

Sub 的样本 PNG 从688672字节增至885898字节（约增加29%），对应 base64 中位数从33.59 ms增至44.80 ms；编码节省明显大于增加的传输准备成本。最终画质、尺寸和像素没有变化。StrictMode 优化针对开发模式，生产构建原先即只加载一次。

原始微基准和复现脚本在本地临时目录 `C:/Users/wangy/AppData/Local/Temp/xchat-capture-perf-1791381468257/`，真实桌面图像未加入仓库。

## 开发构建热路径

进一步独立比较了调用方与依赖的优化等级，每组5次、同一1080p原图，输出均为885898字节且逐像素一致。仅优化调用方、或只优化image/png/fdeflate，均不能充分消除开销。调用方泛型实例和编码/校验依赖需要共同优化；这与[Cargo关于泛型与包级配置的说明](https://doc.rust-lang.org/cargo/reference/profiles.html#overrides-and-generics)一致。

| 组合 | PNG编码中位数 |
| --- | ---: |
| 原默认debug，Fast+Sub | 428.51 ms |
| 五个编码/校验依赖opt2，调用方默认 | 200.83 ms |
| 调用方opt1 + 五个依赖opt2（本次采用） | 78.08 ms |
| 调用方opt2 + 五个依赖opt2 | 15.07 ms |
| 全部依赖及调用方opt2 | 21.33 ms |

采用调用方opt1以兼顾开发调试与编译成本，不为约63毫秒的微基准差值把整个应用提升到opt2。xcap同源、独立非泛型的2MP换色循环另外验证：opt0中位301.49 ms、opt2中位3.38 ms；这一数字不含系统抓屏和GetDIBits。没有扩大到moxcms、全部依赖或发布配置。

完整数据位于 `C:/Users/wangy/AppData/Local/Temp/xchat-codec-profile-1791383632278/results.json`。实际应用收益以完整首帧复测为准。

## 真实应用计时

计时终点为来源画布按实际屏幕尺寸就绪并完成两次 requestAnimationFrame，以浏览器内时间戳计算，另保留自动化工具收到结果的时间。Windows125%缩放；隔离的 Tauri debug 实例、Vite 开发服务。初始1080p编辑器视口为1536×864 CSS px。

| 版本 | 包含指针 | 原生命令返回 | WebView创建 | Canvas首帧 |
| --- | --- | ---: | ---: | ---: |
| 优化前 | 关闭 | 2893 ms | 3046 ms | 3804 ms |
| 优化前 | 开启 | 4571 ms | 4727 ms | 5433 ms |

以上优化前每种模式只有一条成功样本，不作为稳定中位数。后续补采时 Windows 返回 xcap 0x80070006 / GDI 拒绝访问；只读系统探针显示当前远程会话的 Default 桌面不接收输入，GetCursorPos 也返回访问错误。此前同一进程成功过，不能归因为应用启动方式。已请求恢复桌面可见性；未修改桌面权限、锁屏或会话。

用户恢复桌面可见后，探针及应用实际抓屏均恢复。采用Sub但尚未启用包级编译优化时，1080p样本原生命令1348 ms、Canvas2140 ms；分段计时显示开发版抓屏像素转换约0.5秒、PNG编码约0.6秒，由此进一步验证并加入前述开发构建配置。

### 最终构建与满载对照

最终二进制已通过cargo tauri dev完整构建，随后直接启动同一产物。Cargo指纹链确认lanchat调用方opt1、xcap及五个编码/校验依赖opt2实际生效，没有额外RUSTFLAGS覆盖。对照使用预先保留的旧二进制；两个版本都加载同一份最终Vite前端，因此这里只比较原生改动，不把前端拆包收益混入。全程关闭XCHAT_CAPTURE_TRACE，避免同步日志输出影响后一阶段的计时。

采样时已经没有本轮编译或微基准任务，整机CPU仍为99.98%–100%。进程CPU增量采样中QEMU占用最多；未停止或调整用户的虚拟机及其他进程。以下全部成功样本均为2560×1440原图、2048×1152 CSS视口、DPR 1.25。表中列出范围和样本数，不把这些小样本、非空闲环境数据当成稳定P50/P95或发布版速度。

| 版本 | 指针 | 成功首帧样本 | 原生命令返回范围 | Canvas首帧范围 |
| --- | --- | ---: | ---: | ---: |
| 优化前原生 + 同一最终前端 | 关闭 | 2 | 8432–18631 ms | 21334–22125 ms |
| 最终优化构建 | 关闭 | 3 | 1070–2913 ms | 4513–28826 ms |
| 优化前原生 + 同一最终前端 | 开启 | 2 | 9745–13120 ms | 16737–23382 ms |
| 最终优化构建 | 开启 | 1 | 1438 ms | 5649 ms |

异常没有剔除或计作通过：最终版本另一次开启指针时原生命令1389 ms返回，但Canvas等待30秒超时，未记录到该次来源尺寸；按钮计时也发生一次等待WebView调试目标30秒超时。随后检查实际编辑器已显示2560×1440画布，Escape可正常退出。调试目标、rAF与自动化响应存在明显延迟，不能将原生命令返回当成用户已经看到截图界面。

结论：原生处理的重复工作已经减少，同图微基准及满载下的原生命令时间均支持这一点；完整首帧仍存在大幅波动。**正常负载的连续首帧指标尚未验收，不能宣称已实现“稳定几百毫秒”或用本表推断发布版启动速度。** 已请求用户让虚拟机任务空闲；在收到新的环境条件前保留该验证缺口，不改动用户进程。

原始数据及复现脚本位于 `C:/Users/wangy/.codex/tmp/xchat-capture-production/`：`capture-start-performance.json`、`capture-start-comparison-summary.json`、`measure-final-start.js`、`install-capture-paint.js`。原始文件同时保留早期1080p、分段日志开启时的样本和异常，统计时按条件区分。该阶段快捷键因已安装实例占用未完成系统按键验收；调用其相同start路径不替代该项。后续发布补验已通过隔离实例临时注册的全局快捷键完成，见下节。

## 回归与边界

- `rtk npm test`：171/171通过。
- `rtk npm run build`：通过，入口JS315.33 kB、App分块188.90 kB、CSS121.34 kB；截图与贴图不请求App分块。
- `rtk cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib capture_editor::tests -- --nocapture`：14通过、2项需真实桌面的测试默认忽略。新增PNG透明像素往返和指针相交范围边界回归。
- `rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib --bin lanchat`：通过。
- `rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features web --bin lanchat-web`：通过。
- React19真实浏览器验证无页面错误，StrictMode与换源检查结果保存于 `C:/Users/wangy/.codex/tmp/xchat-capture-performance-20261007/strictmode-source-checks.json`。
- DEV及生产静态入口验证：截图/贴图各读取pending一次，主界面及提醒界面按原方式启动，语言及透明背景正确。真实卸载重挂、贴图更新事件及过期响应检查通过；结果为同目录`entry-verification.json`。此项使用模拟IPC，不算原生耗时测试。
- 最终Windows构建从实际按钮进入真实截图，完成800×400选区、多行中文、边框移动与框外提交；系统剪贴板和历史图逐RGBA通道一致。另一真实选区生成550×300物理像素客户区的透明独立贴图。元数据为QA目录的`native-final-functional-copy.json`、`native-final-functional-pin.json`。

macOS继续使用系统原生截图编码，本轮未运行macOS/Linux真实桌面。Web屏幕来源选择由浏览器管理，其选择耗时不属于本轮原生PNG优化收益。

## 2026-10-08 发布补验

用户恢复可见桌面后，Windows截图和实时指针测试显式运行2/2通过。通过Win32 SendInput触发隔离QA实例注册的Ctrl+Alt+Shift+F12，真实1920×1080画布正常出现；Escape退出后恢复原快捷键⌃ 3。没有使用直接调用start命令代替该验证。

当前1080p、125%缩放的优化开发构建，点击启动至实际尺寸画布就绪并完成两次requestAnimationFrame的三个有效样本为1867、1659、1545 ms。CPU采样仍为100%，这些不是正常负载或发布版数据。三个默认300×150空画布样本已明确排除；一次过早重启发生在上一窗口close完成前，调整测试等待后连续通过。原始元数据为QA目录release-visible-desktop-startup.json和release-native-hotkey-check.json。
