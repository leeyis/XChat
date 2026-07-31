# XChat 应用界面套件

`ui_kits/app/` 将设计系统落实为可操作的产品表面。所有页面加载根目录的 `colors_and_type.css`，并通过 `shared.css` 共用四栏布局、导航、列表、消息、文件、AI、模态框和反馈状态。

## Structure

应用套件结构：

- [`index.html`](index.html)：套件入口与范围说明。
- [`chat-workspace.html`](chat-workspace.html)：会话、消息、输入、传输摘要、设备身份与主题切换。
- [`file-center.html`](file-center.html)：来源、类型、搜索、文件预览、路径动作与删除确认。
- [`ai-diagnostics.html`](ai-diagnostics.html)：AI 思考、工具调用、错误结果、模型选择与重新诊断。
- [`shared.css`](shared.css)：应用结构和组件实现。
- `components/` 角色在当前纯 HTML 套件中由单一 [`components.html`](components.html) 审阅文件聚合，不创建 React 子目录。

## Components

组件文件：

- [`components.html`](components.html)：按钮、状态、设备身份、输入器、AI 工具卡片与核心颜色速查。
- [`shared.css`](shared.css)：导航、列表、消息、输入器、传输、信息面板、文件表格、AI 卡片、模态框和 Toast。
- [`../../build/xchat-icons.svg`](../../build/xchat-icons.svg)：从源原型整理的线性图标符号。
- [`../../colors_and_type.css`](../../colors_and_type.css)：所有产品页面直接或通过共享样式加载的设计令牌。

角色映射：`App` 是四栏应用壳，`Sidebar` 是主导航与来源列表，`ChatArea` 是主工作区，`MessageBubble` 是方向性消息气泡，`InputBar` / `Composer` 是完整边框输入器。

## Usage

复用工作流（usage workflow）：

1. 加载（load / import）`../../colors_and_type.css` 与 `shared.css`。
2. 从最接近目标表面的页面复制（copy）结构并组合（compose）工作流，不从 `preview/` 复制产品代码。
3. 保留 `data-od-id`、中文可访问标签和语义状态。
4. 设备身份继续以 MAC 为稳定主键；文件状态继续包含阶段、进度 / 大小、速度 / 说明和动作。
5. 构建（build / create）新页面时，同时验证浅色、深色、1000px 和 860px 以下布局。

## Design Notes

设计说明：

- 聊天：切换会话、展开身份面板、主题切换、发送消息、添加文件、Toast。
- 文件：来源 / 类型 / 搜索联合筛选、在线预览、打开目录、删除确认。
- AI：折叠工具卡片、切换模型、重新诊断、发送诊断请求。
- 主导航当前项只改变图标颜色；会话当前项使用整行绿色。
- 系统无衬线承载操作，等宽字体承载 MAC、IP、容量、速度与时间。
- 所有页面在 860px 以下隐藏列表并扩大紧凑控件的可点击包围盒。
- Source-based layout、colors、typography 与 tokens 均来自根目录规范和真实原型证据。

## Source

高信号完整源原型位于 `../../examples/xchat-desktop-prototype.html`；该文件比 UI Kit 页面覆盖更多边缘流程，适合核对行为而不是直接作为新页面模板。

参考图位于 `../../assets/xchat-desktop-reference.png` 与 `../../assets/xchat-composer-reference.png`；证据映射和缺失资产边界见 `../../context/provenance.md`。
