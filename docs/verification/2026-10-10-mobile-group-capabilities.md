# 移动设备入群能力缓存修复

## 问题与现场证据

群聊创建和添加成员都会检查 `Peer.capabilities` 中的 `group_chat`。此前能力仅保存在内存中，数据库没有保存此字段，`PeerManager::load_from_db` 和工作区的历史设备列表均以空数组恢复能力。应用重启后，尚未重新广播的设备会被误判为不支持群聊。

通过运行中应用的只读工作区接口确认：存在三条名为 OPPO Reno15 Pro 的不同设备记录；两条旧记录为 0.1.6、能力为空，较新的 0.1.12 记录已上报 `group_chat`。截图所选的设备 ID 尚未确认。这些证据不支持“Android 统一不支持群聊”的判断。

## 修改

- 为 `users` 增加有默认空数组的 JSON 文本能力字段，兼容已有数据库。
- 桌面和 Web 的权威发现报文均保存能力；旧式非权威回复保留已有能力，权威空能力列表可以清除旧能力。
- 启动加载和工作区历史设备列表恢复已保存的能力。
- 创建群聊、添加成员原有的协议能力检查保持有效。

已有数据库无法还原过去未保存的能力。更新后需让目标手机上线并完成一次发现，之后离线和重启均可保留能力。选择设备时应核对当前手机对应的记录。

## 验证

新增回归使用临时数据库和离线模拟设备，覆盖“发现 → 旧式回复 → 离线 → 关闭并重新打开数据库 → 创建群聊 → 添加成员”，并确认无群聊能力的设备仍被拦截、成员列表不被错误修改。

通过的命令：

```powershell
rtk cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features web --lib discovered_mobile_peers_can_join_groups_after_restart_without_rediscovery
rtk proxy cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features web --lib db::tests
rtk proxy cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib
rtk proxy cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features web --bin lanchat-web
rtk proxy cargo check --manifest-path src-tauri/Cargo.toml --target aarch64-linux-android --no-default-features --features desktop --lib
rtk git diff --check
```

- 入群回归 1 项通过；数据库测试 19 项通过，包含旧数据库升级和重复初始化。
- Windows desktop、Web 和 Android arm64 编译检查均通过。
- Android 检查使用本机 NDK 27.1.12297006、API 24 的 clang，设置 `ANDROID_HOME`、`NDK_HOME`、`ANDROID_NDK_HOME`、`CC_aarch64_linux_android`、`AR_aarch64_linux_android` 和 `CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER` 后运行上述命令。存在 9 条未使用变量/函数警告，位于本次未修改的文件。
- OPPO 真机入群流程尚未验证。USB 连接的设备是一加 ONEPLUS A6000，并非反馈中的 OPPO；所有新增回归均使用隔离数据库，没有向真实联系人或群聊发送数据。
