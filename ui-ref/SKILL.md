---
name: xchat-open-design
description: 使用 XChat 设计系统生成或审查局域网通讯、设备身份、文件传输、共享文件中心和 AI 诊断界面。
user-invocable: true
---

# XChat Open Design

## What is inside

以下是包内包含内容：

- `DESIGN.md`：产品上下文、视觉基础、布局、组件、动效、语气与反模式。
- `colors_and_type.css`：浅深主题、语义颜色、字体、间距、圆角、阴影和动效令牌。
- `preview/`：颜色、字体、间距、圆角 / 阴影、组件、源资产和应用表面审阅卡片。
- `assets/`、`build/`、`examples/`：真实参考图、源图标与完整交互原型。
- `ui_kits/app/`：聊天、文件中心、AI 诊断和组件速查。

## Source Context

来源上下文：

系统来自 Web Prototype（`2e0eb181-e830-4623-8775-3dc474f49cf9`）。源证据包括品牌规范、76 KB 完整交互原型、桌面四栏参考图和输入器特写；详细映射与校验值见 `context/provenance.md`。

## When to use this skill

适用范围：

当任务涉及 XChat、局域网通讯、设备列表、文件收发、共享文件中心、MAC 身份或 AI 网络诊断时使用本技能。

## How to use

使用方法：

1. 先读 `DESIGN.md`，获取产品语义、组件规则、响应策略和反模式。
2. 原样加载 `colors_and_type.css`，不要重写六个核心颜色。
3. 需要复用产品结构时，查看 `ui_kits/app/`。
4. 需要核对来源时，查看 `context/provenance.md`、`assets/` 与 `examples/xchat-desktop-prototype.html`。

## Generation Workflow

1. 判断表面属于会话、设备、文件、设置还是 AI 诊断。
2. 使用 `56 / 280 / 弹性主区 / 240` 桌面骨架；在窄屏按 `DESIGN.md` 重新编排。
3. 为设备同时提供备注、主机名、当前地址、MAC 和在线 / 发现状态。
4. 为文件传输提供阶段、已传 / 总量、速度或解释、进度和可执行动作。
5. 通过语义令牌实现浅色与深色主题；不要加入新品牌色。
6. 实现真实交互闭环：禁用、聚焦、确认、进度、收据、Toast、错误和空状态。
7. 给主要区域、标题、重复卡片和动作添加稳定的 `data-od-id`。
8. 检查 860px 以下是否无水平滚动，并提供不小于 44px 的响应式触控目标。

## Component Selection

- 会话与设备：使用紧凑列表行，不改成营销卡片。
- 消息：接收使用表面色，发送使用绿色；群聊显示成员名与 MAC。
- 输入器：使用完整边框框体、上方文本区、下方工具栏和右下发送动作。
- 文件：使用状态完整的文件卡片或表格行。
- AI：使用可折叠的思考 / 工具 / 结果卡片与模型单选器。
- 危险操作：使用明确对象和后果的确认对话框。

## Design System Highlights

设计系统要点：

检索关键词：colors、typography、spacing、radius、shadows、icons、layout、interaction。

- 冷白画布、冷灰结构、XChat 绿单点强调；浅深主题共享相同信息层级。
- 56 / 280 / 弹性主区 / 240 桌面骨架；860px 以下重新编排。
- 系统无衬线用于产品操作，宋体仅用于说明型展示标题，等宽字体承载地址与传输数据。
- MAC 是稳定设备身份；文件状态同时给出阶段、进度 / 大小、速度 / 解释和下一步动作。
- 主导航当前项只变绿；会话当前项使用整行绿色。

## Must Avoid

- 大面积绿色、紫色渐变、暖米色默认背景、玻璃拟态。
- 主导航当前项的底色或侧边条。
- 用 IP 作为稳定设备身份。
- 随机头像颜色、模糊文件状态、仅颜色表达错误。
- 过度圆角、卡片套卡片、每个标题配图标。
- 缺少恢复动作的错误文案或虚构指标。

## Completion Check

生成结果必须通过 `DESIGN.md` 第 10 节实施检查，并与 `preview/` 和 `ui_kits/app/` 的表现保持一致。
