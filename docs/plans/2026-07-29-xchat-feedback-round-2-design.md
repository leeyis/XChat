# Xchat 0.1.0 第二轮测试反馈收敛设计

日期：2026-07-29  
状态：已确认方案 A，等待书面规格复核

## 1. 目标

本轮在不破坏现有聊天、群聊、文件历史、SQLite 数据和旧客户端协议的前提下，完成以下反馈：

1. 彻底消除设备 MAC 地址和可用内存的周期跳动。
2. 默认本地名称使用系统机器名称，并安全迁移旧的自动随机名称。
3. 扩充常用表情，选择后立即关闭表情面板并恢复输入焦点。
4. 把截图改成微信式原屏选区编辑：工具栏悬浮在选区下方。
5. 支持从 Finder/文件管理器拖文件到输入区，先形成附件草稿，点击发送后才传输。
6. 文件卡显示真实进度、百分比和速度。
7. 单个大文件拆成 4 个等长范围并行传输，并兼容旧客户端。
8. 完成文件的“打开”动作提供“打开文件”和“在文件夹中显示”。

## 2. 非目标

- 不增加 OCR、滚动截图、录屏、贴纸商店或云端表情资源。
- 不支持一个截图选区横跨多个显示器；截图覆盖主窗口当前所在显示器。用户可先把主窗口移动到目标显示器。
- 不改变消息序列化格式、现有会话 ID、历史消息 ID 或用户 UUID。
- 不暴露任意本地路径给 HTTP 客户端。
- 不移除旧的顺序分块协议；旧客户端和不声明新能力的客户端继续使用 v1。

## 3. 已确认方案

采用“原屏截图 Overlay + 能力协商的文件协议 v2”：

- 截图使用当前显示器的静态画面作为全屏编辑背景，自研选区、控制点和悬浮工具栏。
- 大文件发送先 prepare，再按 4 个等长范围并行流式上传。
- 接收端把每个分块写入独立受管临时文件，全部到齐后按顺序合并并原子完成。
- 发现协议声明 `parallel_file_v2` 能力；未声明的 peer 自动回退现有 v1。

未采用：

- 仅美化独立截图编辑窗：工具栏无法跟随原屏选区，不符合视觉要求。
- 在同一个 `.downloading` 文件中直接并发 seek/write：跨平台写入与崩溃恢复边界更复杂。
- 引入第三方截图或传输框架：现有 Canvas、Tokio、Axum 和 Tauri 已覆盖需要的能力。

## 4. 设备元数据稳定

### 4.1 根因

同一设备会交替收到两类 UDP 帧：

- 权威 announcement：携带真实 hostname、MAC、capabilities 和动态 available memory。
- reply：内存为 `0`，但仍携带另一网络接口的 MAC。

现有 PeerManager 和 SQLite 会接受所有非空 MAC、所有正内存，因此：

- MAC 在 announcement 与 reply 的接口地址间切换。
- available memory 每两秒随系统采样变化。

### 4.2 合并规则

- reply 只允许更新在线状态、名称、地址和 last_seen。
- hostname、MAC、capabilities 只允许非 reply announcement 更新。
- 内存只接受该 peer 的第一个权威正值；后续正值不覆盖。
- 界面字段改为“发现时可用内存”，明确它是稳定的发现快照，不是假装实时监控。
- PeerManager 和 SQLite 使用相同规则。
- 历史中被 reply 污染的 MAC 允许下一条权威 announcement 修正。

### 4.3 验收序列

```text
reply(ac:de..., 0)
announcement(82:ae..., 2356)
reply(ac:de..., 0)
announcement(82:ae..., 2367)
```

最终必须保持：

```text
MAC = 82:ae...
memory = 2356
```

## 5. 默认机器名称

### 5.1 名称来源

优先级：

1. macOS：`scutil --get ComputerName`。
2. 其他平台或命令失败：`sysinfo::System::host_name()`，去除末尾 `.local`。
3. 无法取得系统名称时：保留现有随机名称生成器作为最后回退。

### 5.2 历史迁移

新增 settings key：`username_source`，值为 `machine` 或 `custom`。

- 新数据库写入机器名，同时写入 `username_source=machine`。
- 历史数据库没有 `username_source` 时：
  - 若名称严格匹配旧生成器的形容词、动物和三位数字组合，则迁移为机器名，并标记 `machine`。
  - 否则视为用户名称并标记 `custom`。
- 用户在设置页保存名称时标记 `custom`，后续启动不得覆盖。

## 6. 表情面板

- 使用系统 Unicode，不增加图片资源和网络依赖。
- 从 21 个扩充到约 90 个，覆盖笑脸、手势、爱心、状态和常用物品。
- 排除国旗、肤色、家庭、职业等容易产生平台差异的复杂 ZWJ 组合。
- 桌面使用 9 列网格，面板限制最大高度并纵向滚动；窄窗口自动收窄。
- 点击表情后：
  1. 按 textarea 当前选区插入。
  2. 关闭面板。
  3. 恢复 textarea 焦点。
  4. 把光标放在插入表情之后。
- Esc、点击面板外和切换会话继续关闭面板。

## 7. 微信式截图 Overlay

### 7.1 捕获与窗口

- macOS 捕获主窗口当前所在显示器的完整静态画面。
- 捕获前隐藏主窗口，捕获完成后打开无边框、置顶、覆盖该显示器的截图窗口。
- 截图窗口显示静态画面和半透明暗色遮罩，不再次读取屏幕内容。
- 用户拖动创建选区；选区有边框、八个控制点和尺寸标签。
- 选区可以移动和缩放；开始第一项标注后锁定选区尺寸，避免标注坐标失真。

### 7.2 工具栏

工具栏是浅色、圆角、带阴影的单行悬浮胶囊：

```text
矩形 椭圆 箭头 画笔 马赛克 文本
│ 撤销 重做
│ 取消 钉图 保存 完成
```

- 默认位于选区下方 8px，水平靠右。
- 下方空间不足时自动翻到选区上方。
- 水平位置限制在当前显示器内。
- 极窄选区不会压缩工具栏，工具栏按显示器边界定位。
- 所有按钮使用内联 SVG、tooltip 和 aria-label，不增加图标依赖。
- 选中工具使用品牌绿和浅绿背景。
- 绘制工具点击后可展开颜色与粗细浮层：
  - 6 个预设颜色。
  - 3 档粗细。
- Esc 取消；Cmd/Ctrl+Z 撤销；Shift+Cmd/Ctrl+Z 重做。
- 文本输入框聚焦时保留系统文字撤销，不抢占快捷键。

### 7.3 编辑与输出

- 矩形、椭圆、箭头、画笔、马赛克、文本继续使用现有 Canvas 操作序列。
- 编辑 Canvas 只覆盖选区，坐标保持相对选区。
- export 时裁剪原始显示器截图，再重放操作序列。
- “完成”保存到受管 outbox、加入当前会话附件草稿并关闭截图窗口，不自动发送。
- “保存”弹出系统保存对话框，写入用户选择的 PNG 路径；成功后不关闭。
- “钉图”使用裁剪并标注后的 PNG，保持单例置顶窗口。
- “取消”删除本轮受管临时截图并关闭。

## 8. 外部文件拖放

### 8.1 Web

- 保留 DOM `dragover/drop` 和 `dataTransfer.files`。
- 只有落点位于 Composer 时才捕获文件。

### 8.2 Tauri

- 使用 `getCurrentWebview().onDragDropEvent` 监听 `over/drop/leave`。
- 将物理坐标按 devicePixelRatio 转换后判断是否位于 Composer。
- `over` 时输入区显示拖放高亮。
- `drop` 时读取原生绝对 paths，转换成现有附件草稿对象。
- `leave`、离开 Composer 或组件卸载时清除高亮并解除 listener。
- 拖放只创建草稿；仍由发送按钮触发发送。
- 目录路径拒绝加入，并给出明确错误；普通文件和图片均允许。

## 9. 文件传输协议 v2

### 9.1 能力协商

- discovery capabilities 增加 `parallel_file_v2`。
- 发送前检查目标 peer：
  - 支持：走 v2。
  - 不支持或能力未知：走现有 v1。
- 群聊按每个接收者分别选择协议，不要求群内全部升级。

### 9.2 Prepare

发送方先调用 v2 prepare，提交：

- conversation ID
- client message ID
- transfer ID
- sender ID
- file name
- file size
- file SHA-256
- 每个分块的 offset 和 length
- chunk total
- 群同步信息（如适用）

发送方在 prepare 前以有界缓冲流式计算 SHA-256；实现只增加小型 `sha2` 摘要依赖，不引入传输框架。

接收端：

- 复用现有会话成员、稳定消息 ID、文件名和下载根目录校验。
- 创建或恢复 message/transfer。
- 只有 conversation、sender、message、文件名、大小、SHA-256 和分块布局全部与既有 manifest 一致时才允许恢复；任一字段冲突返回 409。
- 自动下载关闭时返回 `awaiting_acceptance`，不接收 chunk。
- 返回已存在且长度正确的 chunk index，支持重试跳过。
- 已完整接收时返回 `already_exists`。

### 9.3 四路并行上传

- 文件不大于 4 MiB 时使用一个分块；大于 4 MiB 时拆成 4 个尽量等长、互不重叠的连续字节范围。
- 四个任务各自打开源文件、seek 到确定 offset，并以有界缓冲流式发送确定长度；不得把整个大分块读入内存。
- sender 只发送 prepare 响应中缺失的 chunk。
- 任一任务失败：
  - 停止启动新任务。
  - 取消仍在运行的同批任务。
  - transfer 标记 failed。
  - 接收端已完成的 chunk 保留供 retry。
- 用户主动取消：
  - 停止四个任务。
  - 通知接收端取消。
  - 删除该 transfer 的全部受管 part。

### 9.4 接收与完成

受管目录：

```text
<download_root>/.xchat-receive/<safe-transfer-id>/
  manifest.json
  000000.part
  000001.part
  ...
```

- prepare 原子创建 manifest。
- 每个 chunk 只允许写自己的数字 part 文件。
- chunk 长度必须与 manifest 计算值一致。
- 相同 index 和长度的重复请求按幂等成功处理。
- part 先写临时名，flush 后原子 rename。
- 每个在途 part 维护独立已写字节计数；接收端按总和节流更新 `bytes_transferred`，失败重试覆盖该 part 的计数而不是重复累加。
- 每个 chunk 完成后以已有 part 的实际长度校正 `bytes_transferred`。
- 全部 part 到齐后，在接收锁内：
  1. 按 index 顺序合并到现有 `.downloading` 文件。
  2. 校验最终字节数等于 file_size。
  3. 校验合并文件 SHA-256 等于 manifest；不一致时删除全部 part 和合并临时文件，标记 failed，让 retry 全量重传。
  4. 原子改名到最终文件。
  5. 更新 message/transfer 为 completed。
  6. 删除 part 目录。
- 所有路径必须由数据库消息和受管下载根推导，不接受请求中的 save path。

### 9.5 取消与完成互斥

- chunk 提交、取消和最终合并复用同一个 per-transfer 锁及明确状态。
- chunk 可以先写自己的临时文件；取得锁后若 transfer 已取消，则删除临时文件，不能发布为 part。
- finalize 先取得锁并进入 `finalizing`；取消先取得锁则 finalize 不得开始。
- finalize 已先开始并完成原子改名时，后到的取消返回 `already_completed`，不得删除完成文件。
- 取消先完成时，后到的 chunk 和 finalize 均只能退出并清理受管临时文件。

## 10. 进度与速度

- 继续使用 `TransferRecord.bytes_transferred` 和 `bytes_total`。
- 活动传输存在时，前端每约 1 秒刷新 transfer；没有活动传输时停止高频刷新。
- 前端按相邻快照的 byte delta / monotonic elapsed 计算当前速度。
- 无新数据或传输结束时速度归零。
- 文件消息卡显示：
  - 进度条。
  - 百分比。
  - 已传大小 / 总大小。
  - B/s、KB/s 或 MB/s。
  - 取消按钮。
- 底部传输提示显示活动数量和聚合速度。

## 11. 文件打开菜单

- 完成文件的主按钮改为“打开 ▾”。
- 点击后出现紧贴按钮的浮层：
  - 打开文件。
  - 在 Finder/文件夹中显示。
- 复用现有 `file.open` 和 `file.reveal` dispatch。
- 后端继续通过 `trusted_file_path` 校验数据库消息 ID 和路径。
- 平台不支持 reveal 时隐藏第二项。
- 点击外部、Esc、切换会话时关闭菜单。

## 12. 错误处理与兼容

- v2 prepare 返回不支持或 404 时仅在目标未声明能力的情况下回退 v1；声明 v2 后的协议错误必须显示失败，不能静默重传两份。
- 乱序 chunk 在 v2 合法；v1 继续保持当前 409 行为。
- 分块目录只允许取消、成功、删除消息或明确本地副本清理时删除。
- 应用重启后 retry 可通过 manifest 和现存 part 恢复。
- 截图捕获、窗口创建或编辑器初始化失败时必须恢复主窗口；所有退出路径由同一个清理守卫恢复窗口并处理临时文件。
- 传输失败不移除附件草稿中的失败项。
- Finder 拖入目录、不可读文件或已消失路径时在 Composer 内显示具体错误。
- 截图保存失败不关闭编辑器、不丢失操作序列。

## 13. 测试与验证

### 13.1 Rust

- PeerManager：reply/announcement 交替序列稳定。
- SQLite：同样序列稳定，权威 announcement 可修复历史错误 MAC。
- 默认名称：新库使用注入机器名；历史生成名迁移；自定义名称保留。
- v2 prepare：成员、路径、文件名、大小、SHA-256、分块布局和 transfer ID 边界；manifest 冲突返回 409。
- 四个 chunk 乱序并发到达，最终文件字节与源文件完全一致。
- 重复 chunk 幂等。
- retry 只补缺失 chunk。
- cancel 清理全部 part，不能删除受管目录外路径。
- cancel 与 finalize 竞争时结果只能是完整完成文件或完整清理，不能留下半成品。
- v1 顺序传输和旧测试继续通过。

### 13.2 前端

- 表情插入选区、面板关闭、焦点和光标恢复。
- Tauri drag-drop 坐标只接受 Composer 内路径。
- transfer 快照差分产生正确百分比和速度。
- 打开菜单分发 `file.open` / `file.reveal`。
- screenshot history：undo、redo、新操作清空 redo。
- screenshot toolbar 上下翻转和屏幕边界定位使用纯函数测试。

### 13.3 集成

- 两个隔离实例分别验证 v2→v2 和 v2→v1。
- 发送大于 20 MiB 的确定性测试文件，确认拆成 4 个范围且 4 个请求同时进行。
- 传输中确认发送、接收两端进度和速度持续更新。
- 中途取消并确认临时 part 被清理。
- Finder 拖入普通文件和图片，只进入草稿，点击发送后才传输。
- 截图完成后只进入输入框，钉图、保存和取消分别验证。
- 连续轮询设备详情至少 20 次，MAC 和内存保持一致。

## 14. 验收标准

- 同一 peer 连续 20 次刷新中 MAC/内存完全一致。
- 首次启动的本地名称为系统机器名称；现有自动生成名称完成一次安全迁移。
- 表情数量不少于 80，点击后面板关闭。
- 截图选区可移动/缩放，工具栏稳定贴在选区下方或空间不足时上方。
- 六种标注能力、撤销、重做、保存、钉图和完成可用。
- Finder 文件拖入输入区后出现草稿预览，发送前没有网络传输。
- 大于 4 MiB 的文件在 v2 peer 间拆成 4 个范围，任何时刻不超过 4 个并发请求且可观测到 4 路并发；旧 peer 仍能完成 v1 传输。
- 恢复传输时 manifest 冲突会被拒绝，完成文件 SHA-256 与发送源一致。
- 文件卡可见进度、百分比、已传/总量和速度。
- 完成文件可以打开，也可以在 Finder 中显示。
- 全部前端测试、Rust 测试、desktop/web 编译、Tauri 烟测和 `git diff --check` 通过。
