# 移动端修复与验证（2026-09-18）

用户已明确批准 `ui-ref/xchat-desktop-prototype.html#mobile-review`，授权正式实现、编译 APK 和 ADB 安装。本次在工作区已有拍照/录音等改动上完成，未创建 Git 提交。

## 正式实现

- 原生 content 容器应用状态栏、刘海、导航栏及输入法 Insets，解决 WebView 的 CSS safe-area 值为 0 时标题被系统栏遮挡的问题。
- 移动端去掉消息旁常驻操作按钮。长按 500 ms 显示原型中的五列深灰菜单、气泡指向三角和选中背景；移动超过 8 px 或滚动取消长按。横屏空间不足时使用完整安全视口。
- 自己的消息包含提醒和撤回，文件包含查看；复制、转发、引用、删除、多选、表情回应接入实际行为，多选支持批量转发或本地删除。
- 表情和圆圈加号采用原型 SVG，24 px、1.75 线宽、44 px 点击区域。加号展开相册/拍照/文件，保留既有语音功能。
- 相机结果回到草稿。相册/文件使用原生 SAF 选择，后台流式复制到应用附件目录，再通过既有 Rust 路径传输，避免普通文件只有浏览器 Blob 而无法发送；修复原生结果重试时丢失事件名称。
- 文件查看通过受 MANAGE_DOCUMENTS 权限保护的 DocumentsProvider 和目录 ACTION_VIEW 打开文件管理器；实现 findDocumentPath，定位实际目录。优先已有默认处理器，否则选择系统文件管理器。仅公开接收、拍摄/录音和导入附件目录，限制越界路径、隐藏未完成文件、只读文件流，不公开数据库和设置。依据：[Android DocumentsProvider](https://developer.android.com/guide/topics/providers/create-document-provider)、[DocumentsContract](https://developer.android.com/reference/android/provider/DocumentsContract)。
- ConnectivityManager / LinkProperties 读取非 VPN 的 Wi-Fi / Ethernet 清单，保留真实网卡名、索引和 IPv4 前缀；Rust 启动前绑定物理网络，随后沿用 5 秒轮询刷新。
- 默认接收目录改为 Context.getFilesDir()/Downloads，兼容 FileProvider。旧默认 /storage/emulated/0/Download/Xchat 映射到新目录，保留自定义目录，不删除历史文件或改写历史消息路径。
- 真机复现普通文件最后一步硬链接被 SELinux 拒绝，Android 改为后台线程执行 renameat2(RENAME_NOREPLACE)，保持原子发布和同名不覆盖，其他平台保持原实现。

## 真机结果

设备：OnePlus 6 / arm64。原包 com.xchat.app 与本机调试签名不同，采用并存包 com.xchat.app.debug；未卸载原包或清除其数据。测试时暂停原包以避免 8888 端口冲突。

- VPN 全程开启：系统 tun0 为 172.19.0.1/30，wlan0 为 192.168.20.106/24。新版 API/Tauri snapshot 返回真实 LAN 地址，网络清单仅选中 wlan0，桌面 peers 正常在线。
- WebView 起点 screenY=80，状态栏位于应用标题之外；底部手势栏独立于应用内容。检查肖像与横屏界面。
- 消息菜单具有五列图标、三角和选中背景；本人的文件菜单包含查看、转发、引用、复制、删除、多选、提醒、撤回、表情回应。移动端 message-quick-actions 的 computed display 均为 none。
- 文件管理器准确打开“XChat · 接收的文件”，显示测试 APK/TXT。首轮缺少 findDocumentPath 导致退回公共下载目录的问题已实测修复。
- 加号 → 拍照 → 相机确认，4.2 MB JPG 回到当前会话待发送草稿。
- 加号 → 文件 → 系统 SAF 选中 16,048,601 字节 APK → 原发起会话草稿 → 点击发送；电脑隔离实例收到相同字节，手机草稿清空，消息已完成且已送达。
- 最终安装后再次验证：本人的 APK 长按菜单含查看/提醒/撤回，纯文本不显示查看；实际点击提醒成功，撤回测试文本后消息消失；多选可勾选两条消息，系统返回事件退出选择。

### VPN 开启时双向校验

电脑隔离实例使用 18891 端口、%TEMP%/xchat-mobile-20260918/lan-peer 数据库和 received-on-pc 目录。未向真实联系人发送测试消息。

接收：经真实 LAN 192.168.20.106:8888/api/upload 发送 16 个 APK 分块和普通文本，返回 completed，ADB 用应用身份读取哈希。反向：手机经 send_conversation_file 和真实电脑地址 192.168.10.178:18891，使用现有 v3 并行传输，电脑返回 completed 并校验哈希。

| 内容 | 字节数 | SHA-256 |
| --- | ---: | --- |
| APK | 16048601 | 4438a3ec5345988217dc24e47d98bd4c031a2074bcc85353169bed8a8d4f6f0d |
| 普通文本 | 31 | 4f5d3994ae64bbdbecc4f5e8a0f588a8c2a0f32e018b55ac0dfa5c5f95c747b0 |
| 不同内容的同名文本 | 44 | 180e65a99250ef8b4f3868b9aa077913749d14b8d680795a6ddd3564df5a669b |

重名文件追加 (1)/(2)/(3)，旧内容保持。原生选择器导入后再发送的 APK 也匹配上述哈希。

## 构建与检查

- rtk proxy npm run build：通过，静态产物已同步。
- rtk proxy npm test：107 通过，包含新增菜单顶部翻转与屏幕/键盘边界定位测试。
- rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib -j2：通过。
- rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features web --bin lanchat-web -j2：通过。
- rtk cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features web --lib discovery_policy::tests -j2：14 通过，包括 Android 清单、前缀和隧道排除测试。
- 上述测试命令筛选 received：1 通过，验证文件发布不覆盖。
- rtk cargo tauri dev -- -- --port 18888 --db-path .../desktop-smoke：构建启动后因已有桌面实例退出，未算作桌面交互验证；前端/原生命令在 Android 真机完成冒烟。
- rtk cargo tauri android build --target aarch64 --debug：Rust 编译通过，Windows 符号链接权限不足导致最后打包失败；复制原生库并对副本 strip-debug 后，Gradle :app:assembleArm64Debug -x :app:rustBuildArm64Debug 成功。
- Gradle 首轮新增代码的一处 Kotlin nullable receiver 错误已修复。既有 Rust unused/dead-code 警告未作无关清理。
- APK：src-tauri/gen/android/app/build/outputs/apk/arm64/debug/app-arm64-debug.apk；ADB install -r 安装成功。
- 最终 APK 为 70,434,327 字节，SHA-256：e3e08f7bd92c439ece9e8a664b8a18ff2882abe349fb2871784a4f125232f57f。
- rtk git diff --check：通过。

脚本、打包 init script 和证据保存在 %TEMP%/xchat-mobile-20260918/，包括 installed.png、menu-visible.png、own-menu.png、attachments.png、camera-draft.png、folder-fixed.png。

## 范围与限制

已验证当前 VPN 配置下真实 LAN 双向传输。VPN 锁定模式、开关切换、切换 Wi-Fi、长期锁屏后台传输未专项验证；第三方文件来源不提供可定位目录时返回明确错误。Android 仅验证 arm64，未验证 32 位 Android 或 iOS。
