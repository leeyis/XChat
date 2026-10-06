# 大文件接收收尾性能与会话顶栏整理

## 目标与授权

用户反馈约 500 多 MB 文件在“正在校验文件”阶段停留十几秒，要求深入分析、检索公开方案并优化。同时移除会话右上角重复的“刷新地址”入口，保留设备信息抽屉中的刷新功能；连接验证成功后横幅自动消失。

性能优化已授权。界面调整原型 `ui-ref/xchat-desktop-prototype.html?review=header-refresh` 已获用户明确批准：顶栏只保留更多按钮，抽屉的刷新动作可用，成功后收起连接横幅。生产界面已有成功状态隐藏横幅的逻辑，保留并验证该行为。

## 已确认的实现开销

- v4 接收完成后，`merge_parallel_parts` 用 `tokio::io::copy` 逐分片合并，再调用 `sha256_file` 完整读一遍合并后的文件。收尾阶段需要总量约 3N 的读写（读取 N、写入 N、再读取 N）。
- 合并用的小缓冲异步文件拷贝会反复调度阻塞文件 I/O；`sha256_file` 本身已经放在单个 `spawn_blocking` 中，不能把它误诊为逐小块异步读取。
- 发布配置为 `opt-level = "z"`，当前 `sha2 0.10.9` 在无 SHA CPU 指令时走软件实现。先比较编译优化与不同兼容 SHA-256 实现，再决定改动；不能以关闭校验或改用弱校验来掩盖耗时。
- 同名文件的恢复路径可能额外校验已有目标；正常新文件不会无条件重复校验目标。成功后的网络回执也要单独计时，不能全部算作哈希开销。

本机独立 release 基准：576 MiB、4 个分片、无 SHA 硬件指令。首次当前流程为合并 4094 ms、校验 5653 ms，共 9747 ms；单次读取中合并并计算摘要为 5754 ms。以上是候选方案筛选数据，正式结论应使用重复测量和真实程序路径验证。

## 公开方案与取舍

1. 保持 SHA-256 与现有协议，单次大缓冲合并同时计算摘要，减少一次全量读取与大量异步调度；选择经实测更快的兼容哈希实现。优先方案，保留现有断点续传和分片格式。
2. 分片带独立校验、直接写目标临时文件的相应偏移，将校验与下载重叠。收尾开销更小，但需要协议能力协商、乱序写入与崩溃恢复重新设计，不应在未测量前贸然替换。
3. 仅提高编译优化级别可改善软件哈希，但保留两遍文件处理，收益不足以解决全部收尾开销。

参考资料（官方文档 / 项目源代码）：

- [Tokio 文件 I/O 调优](https://docs.rs/tokio/latest/tokio/fs/)：尽量合并阻塞操作，使用缓冲或将同步文件工作置于单个阻塞任务。
- [Syncthing 同步流程](https://docs.syncthing.net/users/syncing.html)：收到数据块时校验 SHA-256，写入临时文件，完成后发布。
- [腾讯 COS 下载校验](https://www.tencentcloud.com/document/product/436/77356)：异步分段校验减少大对象完整性检查耗时。此资料不能据此断言微信客户端采用同样实现。
- [Chromium 下载文件接口](https://chromium.googlesource.com/chromium/src/+/main/components/download/public/common/base_file.h)：保存部分哈希状态；恢复下载时按已有状态 / 摘要验证已有数据。
- [RustCrypto SHA-2 后端](https://docs.rs/sha2/latest/sha2/)：硬件指令检测和软件回退；新版默认展开软件哈希轮次。
- [ring 的增量摘要接口](https://docs.rs/ring/latest/ring/digest/struct.Context.html)、[Windows CNG](https://learn.microsoft.com/en-us/windows/win32/seccng/creating-a-hash-with-cng)：均可计算同一 SHA-256，需根据性能、依赖及跨平台维护成本比较。

## 实施与验证计划

1. 用同一文件重复测量当前合并 / 哈希、单遍合并、编译与哈希实现候选，记录 SHA-256 一致性。
2. 实现选定的单遍合并与摘要计算；阻塞工作留在工作线程，内存固定上限；取消、异常、摘要不符时关闭文件并清理本次输出，避免干扰重试。
3. 增加实际合并的成功、损坏、长度变化、取消及重试回归；保留原有 v3/v4 与分片恢复测试。
4. 原型获准后移除顶栏重复按钮，验证连接成功不残留横幅、抽屉刷新与滚动稳定性。
5. 跑前端及相关 Rust 测试、desktop/web 编译检查；用真实 release 程序对比 576 MiB 收尾耗时，核验内容一致；测试通过后按既有授权升级小版本、提交推送并生成新安装包。

## 0.1.10 实现与测量

采用单遍合并并计算 SHA-256，使用固定 1 MiB 缓冲和一个 `spawn_blocking` 任务。收尾读写量由约 3N 降为 2N。`sha2` 从 0.10.9 升到 0.11.0，利用其默认展开的软件 SHA-256 实现；保留现有体积优化配置、算法、摘要格式和 v3/v4 协议。比较过 ring、Windows CNG 与提高编译优化级别的候选，本次选择现有库升级，避免引入额外直接依赖或平台专用实现。

每次合并使用独立的临时文件，只有长度和摘要均匹配才发布到既有中间路径。取消、读写失败和长度异常只清理本次输出；损坏文件继续触发 SHA-256 错误和分片清理。新增回归覆盖超过缓冲大小的数据、取消、截断、超长、等长损坏、重试，以及独立 Python hashlib 生成的已知摘要。

同一 Windows 开发机、无 SHA CPU 指令、576 MiB 确定性数据、4 个分片，使用相同 `opt-level = "z"`、LTO 和单 codegen unit。旧流程和新流程交替运行三轮，未与本任务的编译并行：

| 轮次 | 旧合并 | 旧校验 | 旧合计 | 新合并＋校验 |
| --- | ---: | ---: | ---: | ---: |
| 1 | 5032 ms | 5645 ms | 10678 ms | 4580 ms |
| 2 | 4787 ms | 6767 ms | 11555 ms | 4963 ms |
| 3 | 5403 ms | 5972 ms | 11376 ms | 4145 ms |
| 中位总耗时 | — | — | **11376 ms** | **4580 ms** |

中位耗时缩短 59.7%。每次完整摘要均为 `0bd8475dec8f02642cfb4ee256761d8e2c5033a8b2e9069be82f58fb903bf658`。这组数据包含合并与哈希的全部时间；不是把哈希时间从“校验”标签移到“合并”标签后只比较单个状态。数据来自本机缓存与 CPU 条件，不代表其他设备的绝对时间。

实际 0.1.9 安装包提取出的桌面程序也复现了延迟：同一 576 MiB 内容，144 个 4 MiB 分片，合并 7266.5 ms、校验 6632.0 ms，共 13898.5 ms。隔离测试使用不可达的回执对端，因此保存/回执另耗时约 2269.4 ms，单独列出而不混入校验。接收输出已用 Python hashlib 独立校验。

保留后处理提示直到文件真正可用，避免提前显示完成。校验状态现在仅做最后摘要比较，计算工作已经包含在合并时间中。顶栏仅保留更多按钮；抽屉中仍可刷新设备地址，验证成功后的横幅隐藏条件保持不变。

## 验证记录

- `rtk npm test`：147 项通过，包含 0.1.10 版本一致性、传输状态和滚动定位回归。
- `rtk npm run build`：通过；提交的静态资源和 Windows 构建资源均需再次核对。
- `rtk cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features web --lib network::conversation_file::tests -- --test-threads=1`：21 项通过。
- `rtk cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features web --lib web_server:: -- --test-threads=1`：21 项通过，覆盖 v3/v4 接收与完成协议。最初使用 `web_server::tests` 的过滤条件未选中用例，已改用实际模块前缀执行。
- `rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib`：通过。
- `rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features web --bin lanchat-web`：通过。

- `rtk cargo tauri dev`（独立标识、端口 18890 / Vite 1434、临时数据库）：原生 WebView2 回归通过。顶栏只保留更多按钮；抽屉按钮实际调用 Rust，通过隔离 WebSocket 对端完成设备 ID 验证后，连接横幅自动消失。无 JavaScript 异常。
- 测试夹具初版仅模拟 HTTP 身份接口，无法通过实际 WebSocket 消息通道验证；补齐 WebSocket 握手及仅限临时数据库的固定地址记录后通过。开发启动等待器首次超过 100 秒，底层编译继续正常完成，未重复启动进程。

当前验证平台为 Windows x64；macOS、Linux、Android 尚未运行。实际新安装包的大文件接收与资源核验写入安装包旁的 `.verification.json`，包括包内文件、前端资源、运行版本、阶段耗时、完整 SHA-256、滚动稳定性及隔离进程清理结果。
