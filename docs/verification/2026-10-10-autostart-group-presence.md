# 开机自启动与建群在线状态验证

2026-10-10，Windows。用户已批准 `ui-ref/xchat-desktop-prototype.html` 后实施。

## 结果

- 设置增加“启动 / 开机自启动”，使用原有“保存设置”操作；未注册时默认关闭。状态直接读取本机系统，读取失败不会阻断聊天，也不会伪装成已关闭。网页及移动端不可修改系统自启动。
- 自启动命令在 Windows、macOS、Linux 桌面目标注册，系统操作在阻塞任务中执行；不写入聊天数据库。Windows 启动路径正确加引号，Linux Exec 字段处理空格及保留字符。
- 新建群聊显示在线／离线文字及状态点，在线设备优先，同状态保持原顺序；离线设备仍可选择，设备标识及勾选状态不变。

## 自动检查

以下命令通过：

```powershell
rtk proxy node --test frontend/src/autostart.test.js frontend/src/xchat.test.js frontend/src/workspace-sync.test.js frontend/src/desktop-connection.test.js frontend/src/styles.test.js
rtk npm run build
rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib
rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features web --bin lanchat-web
rtk proxy rustfmt --check --edition 2021 src-tauri/src/autostart.rs
rtk git diff --check
```

前端共 102 项通过。新增集成测试覆盖默认状态、开启／关闭、增量快照、系统读写失败、写入结果确认、外部状态变化及不支持的平台。

额外临时脚本渲染正式 React 组件，在中文和英文下验证群成员在线优先、同状态稳定排序、离线可选、输入数组不变，以及自启动开启／关闭／读取失败／平台不可用状态。

## Windows 系统与桌面界面

使用正式 `src-tauri/src/autostart.rs` 编译临时探针，验证真实 HKCU 启动项：开启、读取、含空格的程序路径、任务管理器禁用状态、再次启用、关闭及重复关闭，全部通过。测试项使用独立名称，验证后清理。

首次 `rtk cargo tauri dev` 因已有开发实例占用 `K:\cargo\debug\lanchat.exe` 而链接失败。保留已有实例，在临时源码副本中将测试二进制改名，使用独立应用标识、启动项名称和临时数据库；桌面构建和启动成功。临时副本仅调整测试构建配置和启动项名称，正式业务实现保持一致。

初次使用的 `18893` 端口另有原型预览服务，因此保留该服务，改用已确认空闲的 `18994`。为避开其他构建占用的 Cargo 锁，直接运行已构建的同一测试程序，确认 HTTP 服务返回 `200` 并提供 Xchat 前端。

通过隔离 WebView 的调试接口操作真实界面，验证：初始未勾选、勾选尚未保存不改变系统、保存成功后系统已启用、离开并重新进入设置仍显示启用、取消勾选尚未保存不改变系统、保存关闭后系统已禁用，以及两种成功提示。界面截图检查通过。并行实例导致全局截图快捷键已被占用，不影响本次流程。

测试启动项和临时运行实例已清理；没有修改用户的 `Xchat` 启动项。macOS 与 Linux 未做实机验证，未执行电脑注销或重启。
