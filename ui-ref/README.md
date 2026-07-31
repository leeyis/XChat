# XChat Open Design 设计系统包

这是从 Web Prototype 源项目提炼出的完整设计系统工作区。它覆盖 XChat 的局域网通讯、设备身份、文件传输、共享文件中心和 AI 网络诊断表面，并保留原始实现与参考图像作为可追溯证据。

## Product Overview

XChat 是面向局域网与跨 VLAN / WireGuard 场景的桌面通讯工具。用户在一个四栏工作台中发现设备、单聊或群聊、收发文件、管理共享文件，并通过 AI 助手诊断端口和防火墙问题。系统以 MAC 作为稳定设备身份，完整呈现传输阶段与恢复动作，用克制的中性界面承载高密度信息。

产品类型（product）：桌面应用与通讯工作区（desktop app / workspace）。它支持（supports）设备发现、会话、文件收发和网络诊断，并提供（provides）可追溯身份、完整状态和可恢复动作。

主要表面：

- 会话与群聊：消息、已读收据、截图、附件和设备详情。
- 设备：在线 / 离线、备注、主机名、IP、MAC 和发现方式。
- 文件中心：按来源、类型和搜索筛选，预览、定位与删除。
- AI 诊断：思考过程、工具调用、错误结果、模型选择与重新扫描。

## Review First

1. [`preview/index.html`](preview/index.html)：设计系统审阅入口。
2. [`preview/applied-surfaces.html`](preview/applied-surfaces.html)：最接近真实产品的应用表面。
3. [`preview/components.html`](preview/components.html)：消息、设备、传输、输入器与 AI 卡片。
4. [`ui_kits/app/index.html`](ui_kits/app/index.html)：应用套件入口。
5. [`DESIGN.md`](DESIGN.md)：完整规则与实施检查表。

## Package Contents

```text
.
├── DESIGN.md
├── README.md
├── SKILL.md
├── brand-spec.md
├── colors_and_type.css
├── assets/
│   ├── README.md
│   ├── xchat-composer-reference.png
│   └── xchat-desktop-reference.png
├── build/
│   └── xchat-icons.svg
├── context/
│   ├── provenance.md
│   └── source-context.md
├── examples/
│   └── xchat-desktop-prototype.html
├── preview/
│   ├── index.html
│   ├── manifest.json
│   ├── preview.css
│   ├── colors.html
│   ├── colors-primary.html
│   ├── typography.html
│   ├── typography-specimens.html
│   ├── spacing.html
│   ├── spacing-tokens.html
│   ├── radius-shadows.html
│   ├── components.html
│   ├── components-buttons.html
│   ├── brand-assets.html
│   └── applied-surfaces.html
└── ui_kits/app/
    ├── README.md
    ├── index.html
    ├── shared.css
    ├── chat-workspace.html
    ├── file-center.html
    ├── ai-diagnostics.html
    └── components.html
```

根目录保留的 `xchat-desktop-prototype.html`、`image.png` 与 `image-1.png` 是源项目复制件；`examples/` 和 `assets/` 中的同源文件是设计系统包的稳定消费路径。

Preserved source-backed artifacts 包括 `assets/xchat-desktop-reference.png` 与 `assets/xchat-composer-reference.png` 品牌参考图、`build/xchat-icons.svg` runtime 图标和 `examples/xchat-desktop-prototype.html` 完整 component 实现；源证据没有 font 文件，因此不创建空的 `fonts/`。

## Source Context

- 来源项目：Web Prototype（`2e0eb181-e830-4623-8775-3dc474f49cf9`）。
- 设计系统 ID：`user:web-prototype-design-system`。
- 证据索引与 SHA-256：[`context/provenance.md`](context/provenance.md)。
- 原始项目交接：[`context/source-context.md`](context/source-context.md)。
- 真实参考图：[`assets/`](assets/)。
- 完整高信号实现：[`examples/xchat-desktop-prototype.html`](examples/xchat-desktop-prototype.html)。

## Preview Manifest

| 审阅主题 | 主文件 | 兼容入口 |
|---|---|---|
| 颜色与主题 | `preview/colors.html` | `preview/colors-primary.html` |
| 字体与信息层级 | `preview/typography.html` | `preview/typography-specimens.html` |
| 间距与密度 | `preview/spacing.html` | `preview/spacing-tokens.html` |
| 圆角与阴影 | `preview/radius-shadows.html` | — |
| 核心组件 | `preview/components.html` | `preview/components-buttons.html` |
| 源资产 | `preview/brand-assets.html` | — |
| 应用表面 | `preview/applied-surfaces.html` | — |

[`preview/manifest.json`](preview/manifest.json) 是机器可读清单；[`preview/index.html`](preview/index.html) 是统一审阅入口。兼容入口保留与主卡片相同内容，供设计系统检查器按约定文件名发现。

## Reuse Workflow

- 新界面先加载 [`colors_and_type.css`](colors_and_type.css)，只通过语义令牌引用颜色、字体、间距、圆角和阴影。
- 组件形态、状态和响应行为以 [`DESIGN.md`](DESIGN.md) 为准。
- 直接复用 UI Kit 的布局与组件时，从 [`ui_kits/app/shared.css`](ui_kits/app/shared.css) 和对应页面开始，不从预览卡片复制产品代码。
- 需要对照源证据时，查看 [`context/provenance.md`](context/provenance.md)、[`assets/`](assets/) 与 [`examples/`](examples/)。
- 使用 AI 生成 XChat 界面时，加载 [`SKILL.md`](SKILL.md) 作为执行约束。

推荐顺序：

1. 从 `DESIGN.md` 确定目标表面与不可变规则。
2. 加载令牌样式和 `ui_kits/app/shared.css`。
3. 从最接近工作流的 UI Kit 页面复用结构。
4. 用预览卡片核对视觉基础、组件状态与源资产。
5. 验证浅色、深色、1000px 与 860px 以下布局。

## Design Notes

- MAC 地址是稳定设备身份；IP 只描述当前连接。
- 主导航当前项只变绿，不加底色或侧边条。
- 选中会话使用整行绿色。
- 消息输入器必须是完整边框容器，工具栏固定在底部。
- 文件传输必须同时呈现阶段、进度 / 大小、速度 / 说明和下一步动作。
- 绿色只用于当前、主动作、发送消息与活动进度。
- 当前证据没有独立 Logo、应用图标、托盘图标或字体文件，不得臆造。

## Theme

默认浅色。将 `data-theme="dark"` 加到 `html` 或组件根节点可启用深色主题。浅深主题共用相同组件结构与绿色强调。
