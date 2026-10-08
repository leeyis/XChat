# 移动端视频全屏与还原

状态：用户于 2026-10-08 明确回复「评审通过」，并授权实现、提交、推送远程和编译 Android 安装包；另已授权通过 ADB 连接真机测试及更新。

## 需求与范围

用户已确认安装最新 Android 0.1.12 后，接收文件时的滚动问题已消失，音视频可以播放。本次新增功能为对话内视频的全屏播放与还原。

用户进一步明确：仅修改移动端，桌面端视频播放已正常，保持其现有界面与行为。前端新增入口和相关行为必须通过 Android 平台能力隔离，不能仅按视口宽度判断。原型沿用仓库规定的统一 HTML 入口，页面标题与评审说明均标注 Android。

手机专用入口：`ui-ref/xchat-android-video-prototype.html`，无需参数，默认呈现三张手机画面：对话内播放、手机全屏播放、还原回对话。

仓库规定的入口 `ui-ref/xchat-desktop-prototype.html?review=video-fullscreen` 使用相同的原型资源。默认桌面原型增加手机评审链接，防止丢失查询参数时无法找到手机设计。专用入口不加载桌面应用界面，且附有静态设计图作为加载失败时的预览。

评审页在手机框内演示全屏，避免在电脑评审时将整个浏览器变成全屏。正式 Android 实现仍接入原生全屏视图；手机框和框外的评审操作不属于产品界面。

- 视频卡片下方提供「全屏」入口；全屏左上角提供「还原」。
- Android 系统返回键优先退出全屏，回到当前对话。
- 保留同一个视频元素及其播放进度、播放或暂停状态；还原时保留聊天滚动位置。
- 全屏隐藏系统栏，画面等比例完整显示；跟随设备横竖屏方向。
- 进入后台继续沿用已有暂停规则。

## 已定位的原因与接入位置

当前 Wry 生成的 `RustWebChromeClient.onShowCustomView` 直接调用 `callback.onCustomViewHidden()`，因此原生全屏请求被立即取消。生成文件不可作为持久修改位置。

拟在 `MainActivity.onWebViewCreate` 中延后安装一个委托原有客户端的 WebChromeClient，仅接管全屏视图生命周期。当前锁定 Wry 0.54.2，其初始化顺序是先调用 Activity 的 `setWebView`，之后才 `setWebChromeClient`，因此不能在回调内立即替换。项目已有 `androidx.webkit:webkit:1.14.0`，可按特性检测使用 `WebViewCompat.getWebChromeClient` 获取现有客户端，保留权限请求、文件选择、JS 对话框、标题与日志等行为。

Activity 接管全屏视图容器、系统栏和返回键。前端通过全屏 API 放大视频容器，保持媒体元素及 source 不变；全屏 CSS 提供还原按钮。需核对当前播放器的 blur／后台暂停规则，避免将前台全屏切换误判为离开应用。

参考：[Android WebChromeClient](https://developer.android.com/reference/android/webkit/WebChromeClient#onShowCustomView(android.view.View,%20android.webkit.WebChromeClient.CustomViewCallback))、[WebViewCompat](https://developer.android.com/reference/androidx/webkit/WebViewCompat#getWebChromeClient(android.webkit.WebView))。

## 原型验证

使用本地 8 秒 MP4 样例，在 Edge 和 Chrome 中验证最新的手机专用入口：

- 暂停在 3.25 秒，进入全屏和还原后仍为 3.25 秒、暂停状态。
- 播放中进入全屏并还原，时间从 1.258 秒连续推进到 1.305、1.352 秒，未暂停或重播。
- 1280 × 1020 评审页面并排显示三张手机画面。全屏预览尺寸为手机内部的 329 × 644，而非电脑浏览器全屏。
- 390 像素视口内，三张手机预览宽度均为 350 像素，纵向排列，无横向溢出。
- 专用 HTML 和静态 PNG 均返回 HTTP 200；禁用 JavaScript 仍可查看设计图。
- 仓库统一原型的默认页有 Android 评审链接，可跳转到不带参数的手机专用入口。
- 页面没有 JavaScript 错误。

可直接查看的设计图为 `ui-ref/assets/android-video-review.png`。验证记录和其他截图保存在忽略目录 `dist/android/qa/video-fullscreen/`。

## 批准后的验证

需在 Android 包中覆盖全屏入口、还原按钮、系统返回键、原生播放器全屏入口、连续进出、拖动进度、播放和暂停状态、横竖屏、后台暂停、聊天位置和安全区域恢复。还需回归文件选择及媒体权限，确认委托原有 WebChromeClient 没有破坏已有功能。

## 实现与发布验证

- 版本统一升级为 0.1.13。`MessagePlayer` 通过 `nativeVideoFullscreen` 能力仅在 Android 启用按钮；Web 和桌面继续使用原有播放器。
- `video-fullscreen.js` 管理进入、退出、失败重试、重复请求和卸载清理；使用同一个媒体元素，还原系统栏布局后恢复聊天滚动位置。
- `VideoFullscreenController.kt` 委托已有 WebChromeClient，托管全屏视图及系统栏；MainActivity 优先处理全屏返回键，并在失去前台状态时暂停媒体。
- `rtk proxy npm test`：176 项通过。版本同步测试在升级时更新为 0.1.13。
- `rtk proxy npm run build`：通过。
- `:app:compileArm64ReleaseKotlin -x :app:rustBuildArm64Release`：通过。初次编译发现 Android 36 已移除旧 AppCache 回调，删除该不可用委托后重新编译通过。
- 生产界面浏览器验证：暂停 3.25 秒进入和退出后进度不变；聊天 scrollTop 保持 1196；播放中从 1.268 秒连续推进到 1.306 秒，元素未重建、未暂停。
- `rtk cargo tauri dev --no-watch --config dist/android/qa/tauri-fullscreen.json -- -- --port 18889 --db-path dist/android/qa/desktop-data`：已启动隔离实例。桌面原生 WebView 能播放样例视频，未出现 Android 新增入口，原有媒体控件保留。
- `rtk cargo tauri android build --target aarch64 --apk true --aab false --ci`：原生 Release 编译通过（4 分 38 秒）；随后因本机 Windows 符号链接权限限制，Tauri 封装阶段退出。将本次编译的库复制至 JNI 目录后，用 `:app:assembleArm64Release -x :app:rustBuildArm64Release` 完成 Gradle 打包（47 秒），未使用旧库。
- 正式 APK：`dist/android/Xchat_0.1.13_android-arm64.apk`，16,548,986 字节；应用 ID `com.xchat.app`，versionName `0.1.13`，versionCode `1013`，arm64-v8a，minSdk 24 / targetSdk 36。
- SHA-256：`62cc0f89626974162973e363d3179144aa027a97b834599d5f7656903cd890be`。签名证书沿用上一版；签名、16 KiB ZIP 对齐、ZIP 完整性、非调试包、原生库及当前前端资源校验均通过。构建源码提交为 `9dc28da34e0ca7cb105f77fefd4a5cde09b37a3d`。
- `rtk proxy adb -s 30192c63 install -r dist/android/Xchat_0.1.13_android-arm64.apk`：成功覆盖安装到用户连接的 OnePlus 6（Android 11，WebView 92），保留应用数据。
- 用户随后明确反馈「移动端视频播放测试已经通过」，本次真机验收通过。测试结论来自用户实际操作，不能表述为完整自动化真机回归通过。
- 自动化真机调试曾遇到测试数据注入时序问题及页面重载后调试连接中断，日志中有一次原生 SIGSEGV；未确定与全屏功能的关系，未完成横竖屏、系统返回键及权限委托的全套自动化设备验证。终止继续操作手机，采用用户本次真机验收结果，并保留调试记录。浏览器验证与 176 项单元测试结果独立有效。
- 本次只构建 Android arm64 安装包；未构建 iOS 和其他 Android ABI。隔离桌面验证实例已关闭。
