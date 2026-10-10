# Windows WebView2 后台调度调查

日期：2026-10-10。范围：只读依赖源码、公开文档和既有测试报告；未改生产参数，未启动应用或运行性能测试。使用 Context7 查找 Tauri 文档后，以本地锁定版本源码复核。

结论：Windows 没有 Tauri/Wry 稳定公开的“一键取消全部后台节流”接口。三个 Chromium 开关适合作为后续诊断变量，不能据此承诺最小化后持续 30 FPS。当前先测原生 HEVC 压缩路径的默认运行时性能，不引入未经实测的生产 flags。

## 既有证据及边界

- [shared IPC 基线](2026-10-10-remote-shared-ipc-baseline.json)：约 4.68 FPS；一次发送端采样 capture 9.724 ms、bridge 1314.449 ms、canvas 17 ms。最小化后 bridge 5703.097 ms，并报共享帧传递超时。
- 测试时共享方被运动窗口遮盖；这些数据支持调查遮挡/后台调度，但不能单凭 bridge 指标断言时间全部耗在 Chromium 节流。bridge 还包含原生任务排队、跨进程传递与 JS 回调等待。
- 报告 user agent 仅给出 Edge/Chrome 154.0.0.0，不能由此确定 WebView2 完整构建号或它与 Chromium 上游的精确提交对应关系。
- 后续 focus-emulation 诊断约 7.48 FPS，由主任务报告；本调查未独立复跑，不作为交付性能证据。独立 viewer 生命周期通过，也不等于后台发送性能达标。

## 锁定版本与公开 API

`Cargo.lock`：Tauri 2.10.2、tauri-runtime-wry 2.10.0、Wry 0.54.2、tauri-utils 2.8.2。

| 配置/API | 已核实行为 | 使用限制 |
| --- | --- | --- |
| Tauri `WebviewWindowBuilder::background_throttling(Disabled)` | Tauri 明确 Windows 不支持；Wry 仅在 macOS 14+/iOS 17+把 WKWebView `inactiveSchedulingPolicy` 设为 `None` | 构建期配置；不能修复 Windows，旧 macOS 不生效 |
| `additional_browser_args` | Wry 创建 WebView2 environment 时设置 `AdditionalBrowserArguments` | 不是运行中可切换的策略；显式值替换 Wry 默认参数 |
| WebView2 `IsVisible` / `Resume` | 可动态改变程序可见状态、恢复显式 suspension | 不构成所有后台调度/绘制的豁免。Wry 当前 `WM_SIZE` 分支在最小化时只跳过 bounds 更新，并未在此设置 `IsVisible=false`，不能假定重复设 true 就能解决 |
| `MemoryUsageTargetLevel::Normal` | 稳定的内存使用目标，与 Low 相对 | 不等于取消后台节流，内存充裕也不能排除调度问题 |
| `ICoreWebView2ExperimentalSettings9::Preferred*TimerWakeIntervalInMilliseconds` | 可即时调整脚本 timer 的首选唤醒间隔，0 表示首选 0 ms | 仍是 prerelease experimental；仅针对 `setTimeout`/`setInterval`，运行时可受资源约束，其他后台策略独立生效 |

对应源码：[Tauri 构建器](https://github.com/tauri-apps/tauri/blob/tauri-v2.10.2/crates/tauri/src/webview/webview_window.rs#L1187-L1205)、[Wry WKWebView 实现](https://github.com/tauri-apps/wry/blob/wry-v0.54.2/src/wkwebview/mod.rs#L458-L480)、[Wry environment 参数](https://github.com/tauri-apps/wry/blob/wry-v0.54.2/src/webview2/mod.rs#L284-L328)、[Wry 最小化处理](https://github.com/tauri-apps/wry/blob/wry-v0.54.2/src/webview2/mod.rs#L1203-L1239)、[Wry 内存目标](https://github.com/tauri-apps/wry/blob/wry-v0.54.2/src/webview2/mod.rs#L1726-L1734)。动态接口参见 [Controller](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2controller)、[ExperimentalSettings9](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2experimentalsettings9?view=webview2-1.0.4181-prerelease)。

## 三个启动参数到底覆盖什么

| 参数 | 源码/官方文档给出的作用 | 尚不能保证 |
| --- | --- | --- |
| `--disable-background-timer-throttling` | 禁用后台页面的 timer task throttling | 不是所有 IPC、共享缓冲事件、canvas 或合成器的无限速开关 |
| `--disable-backgrounding-occluded-windows` | Chromium `UpdateWebContentsVisibility` 将 `OCCLUDED` 转为 `VISIBLE` | 分支不转换 `HIDDEN`；不能单独保证最小化状态 |
| `--disable-renderer-backgrounding` | 当前 Chromium 上游 `ComputeAppliedPriority` 强制应用的 renderer priority 为 visible；桌面还清除 priority override，作用不只旧说明所写的 OS priority | 不是 WebContents 每一项可见性/绘制策略的承诺；上游当前源码不能替代本机 WebView2 实测 |

来源：[Microsoft 对 timer flag 的说明](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/webview-features-flags)、[Chromium 遮挡状态处理](https://chromium.googlesource.com/chromium/src/+/main/content/browser/web_contents/web_contents_impl.cc)、[Chromium renderer priority 实现](https://raw.githubusercontent.com/chromium/chromium/main/content/browser/renderer_host/render_process_host_impl.cc)。后两项是调查当日的上游 main，未声称与本机 Edge 的完整构建相同。

Microsoft 明确不建议把 browser flags 用于生产依赖：它们可能随版本改变或删除。这是兼容性限制，因此本次没有把这些参数写入生产配置。[官方说明](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/webview-features-flags)

## 最小可落地的后续步骤

1. 先以默认 runtime 测原生压缩路径，分别记录主窗可见、完全被遮挡、最小化、恢复四种状态的接收 FPS、画面年龄、超时与丢帧。原生编码减少大帧搬运和 canvas 工作，但若每帧发送仍由 JS 主线程触发，就仍需验证后台调度。
2. 若仍有差异，再做三个 flags 的隔离 A/B。必须在第一个 WebView2 browser process 启动前设定，并完整重启使用同一 user-data-folder 的 WebViews；只重开 viewer 不足以证明新参数生效。不同 environment options 共享 browser process 可能返回 `ERROR_INVALID_STATE`。[WebView2 environment 规则](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/webview2-idl?view=webview2-1.0.3967.48)
3. 诊断可在启动器中给进程级 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` 追加参数，保留已有 CDP 等参数，不写全局环境变量。该环境变量会追加至 environment options；提权运行存在忽略环境覆盖的限制。若改用 Tauri builder，则所有同 profile 窗口需统一参数，并保留 Wry 默认 `--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection`，以及需要的 autoplay/proxy 参数；显式值会绕过 Wry 默认参数组装。
4. 如果仍冻结，应拆分测量原生请求入队、UI 线程执行、PostSharedBuffer 返回、JS 收到事件、发送完成五段。稳定方案是把持续捕获、编码及关键发送调度保留在原生层；不能仅延长 5 秒超时来掩盖停止送帧。

本次归档未执行新 E2E、benchmark、构建或应用进程操作。
