# OnePlus 6 地址发现真机复查（2026-09-18）

## 已确认的原因

通过已连接的 ADB 设备 `30192c63` 读取手机网络和应用日志，并使用 Windows 上的 Cargo 调试实例复现。手机运行 `com.xchat.app.debug`，设备 UUID 为 `f237e04e-aa9d-476c-b5d0-73068cf75ec6`。

- 手机真实 Wi-Fi 地址为 `192.168.20.106/24`，HTTP 身份接口和带目标 UUID 的 WebSocket 握手均成功；桌面历史地址 `192.168.10.120:8888` 返回连接拒绝。
- 从电脑 `192.168.10.178:18892` 定向发送发现报文到手机真实地址，收到同一手机 UUID 的 UDP 回复，来源却为 `192.168.10.120:临时端口`。旧 12 字段报文没有真实 IP，桌面因此把经过改写的 UDP 来源当作聊天服务地址。已证实来源改写，尚未定位具体改写节点，不能仅凭 VPN 开启就断定是 VPN 实现的问题。
- 手机保留 `192.168.10.178:18891 → 8838bc91-e41f-45a0-a6eb-8b0dbe725898` 的旧调试实例绑定。旧身份过滤只按 IP 建索引，错误拒绝了同一电脑上正式客户端 `192.168.10.178:8888` 的不同 UUID；手机日志明确记录了该拒绝。
- 选中的 OnePlus 6 历史记录 UUID 与真机一致，本次不是同名设备混淆或重新安装导致的 UUID 变化。

上一轮本地注入测试没有经过这条来源改写的真实网络路径，因此漏掉了第一个问题。

## 修复

`src-tauri/src/network/discovery.rs`：

- 追加可选第 13 字段，携带最多 4 个去重的 IPv4 字面量，使用原有公告服务端口；兼容旧 6/7/12 字段，旧客户端可忽略尾部字段。
- 仅从启用的网络接口生成地址；按接口发送广播和回包时优先携带当前出口地址，避免多网卡时被数量上限截掉。
- 丢弃 URL、主机名、额外端口、回环、0/8、组播及保留广播地址。所有提示仅进入候选集，沿用 WebSocket 目标 UUID 和响应 UUID 校验，成功后才更换活动地址。
- 固定地址保留完整 IP 和端口，按公告的服务端口区分同一 IP 上的实例，不使用 UDP 临时来源端口。
- 关闭自动发现时，只有来源 IP 和已绑定 UUID 正向匹配才可使用固定地址例外。允许已绑定设备的内外端口映射差异，以及多个映射服务公告同一内部端口。
- 桌面、Android 和 Web 监听路径采用相同的过滤及候选处理方法。

`src-tauri/src/network/peer_connection.rs`：

- 保留最近固定地址和当前地址的验证机会，同时优先处理其余新候选，避免大量旧固定记录挤掉新公告的可达 IP。

本次未修改生产 UI，也没有把 ADB 读到的 IP 硬编码进产品或写入用户的固定设备配置。

## 真机结果

Cargo 隔离实例使用端口 `18891` 和目录 `%TEMP%/xchat-cargo-lan-z4rzhg_l`。保留手机既有的诊断实例 UUID，隔离数据库只预置手机旧地址 `.10.120`，其固定设备列表为空。没有手工添加手机 `.20.106`。

1. 更新前：手机在线，但连接状态 `missing`，活动地址 `.10.120:8888`，WebSocket 错误 10061。
2. 更新手机调试包后：实例自动得到 `.20.106:8888`，状态 `ready`，`previous_address` 为 `.10.120:8888`，`error=null`。
3. 新手机回复仍从 `.10.120` 到达，但第 13 字段为 `192.168.20.106`，确认恢复依赖新协议而非网络环境刚好改变。
4. 主动刷新接口返回 `ready` 和 `.20.106:8888`；设备列表、连接快照和 SQLite 中的活动地址一致。
5. 手机端同时将正式桌面 `192.168.10.178:8888` 和诊断实例 `192.168.10.178:18891` 判为 `ready`，旧固定记录没有删除，证明端口误拦截已修复。

安装前从手机备份 APK 并比较签名证书；使用 `adb install -r` 更新既有 `com.xchat.app.debug`，没有卸载应用或清除数据。手机应用 UUID 保持不变。电脑仍需安装本次重新打包的 Windows 客户端，旧客户端不会解析新地址字段。

## 检查与构建

- `rtk cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features web --lib -j2 -- --test-threads=1`：133 通过。
- `rtk cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib network:: -j2 -- --test-threads=1`：83 通过。
- `rtk cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib web_server::websocket_protocol_tests:: -j2 -- --test-threads=1`：18 通过。
- 桌面完整测试的串行运行：148 通过、2 失败，均发生在测试结束删除 SQLite 临时目录时（Windows 错误 32）；上述 WebSocket 测试组单独复跑全部通过。此前并行运行还遇到同类清理竞争及一个依赖毫秒定时的超时测试波动。本次未改写这些无关测试的清理实现。
- `rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib -j2`：通过。
- `rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features web --bin lanchat-web -j2`：通过。
- `rtk cargo build --manifest-path src-tauri/Cargo.toml --no-default-features --features web --bin lanchat-web -j2`：通过，并通过 `rtk cargo run ... --port 18891 --db-path ...` 运行该共享网络核心完成实机验证。
- `rtk cargo tauri android build --target aarch64 --debug --apk true --aab false --ci`：Android arm64 Rust 编译通过；Tauri 打包因 Windows 无符号链接权限失败。复制本次原生库、仅去除副本调试符号，再用既有调试包 init script 执行 Gradle `:app:assembleArm64Debug -x :app:rustBuildArm64Debug` 成功。APK 内原生库与复制的本次编译产物 SHA-256 一致。
- Android 实机只验证 arm64，未验证其他 Android ABI、iOS 或 macOS；未专项验证 VPN 锁定模式、切换 Wi-Fi 或长期后台休眠。

实机证据保存在 `%TEMP%/xchat-cargo-lan-z4rzhg_l/nat-verification.json` 与 `phone-update.json`；启动日志为同目录 `cargo-debug-after.log`。

## 交付

`rtk cargo tauri build --bundles nsis --features desktop --ci` 成功，Release 编译耗时 7 分 20 秒。安装包通过 7-Zip 完整性检查，内嵌程序与本次 Release 可执行文件 SHA-256 一致，PE 架构为 x64。构建仍有既有的 PDB 文件名冲突、`.app` 标识符和缺少 updater bundle marker 警告；NSIS 手动安装包正常生成。

- Windows：[Xchat_0.1.6_x64-setup_20260918-180928.exe](../../dist/Xchat-0.1.6-lan-address-fix-20260918-180928/Xchat_0.1.6_x64-setup_20260918-180928.exe)，4,414,775 字节。
- Windows SHA-256：`358c5d95724c04296716b6c76eae8e9ae745b5d5eda5b6cd1c1be297a1db3d6e`。
- 已安装的 Android 调试包也保存在同目录，文件名 `Xchat_0.1.6_android-arm64-debug_20260918-180928.apk`；SHA-256 为 `ad15aab826902d5e1786549a22b7889c290e7b64d63dad17fee8b0030c437797`。
- 同目录包含 `BUILD-INFO.json`、`SHA256SUMS.txt` 和实机 JSON 证据。

后续多轮发现期间地址持续保持 `.20.106:8888` 且状态 `ready`，没有回退到被改写的来源。验证完成后已关闭本次创建的 Cargo 隔离实例；用户原有桌面客户端继续运行。未安装 Windows 新包，由用户手工安装测试。
