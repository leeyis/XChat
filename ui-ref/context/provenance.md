# 设计系统溯源

本工作区在新设计系统项目内原位生成，没有创建第二个项目或设计系统 ID。

## 身份

- 来源项目 ID：`2e0eb181-e830-4623-8775-3dc474f49cf9`
- 来源项目名：Web Prototype
- 新设计系统项目 ID：`ed504cc6-b960-4233-80be-1daf664b9eb1`
- 设计系统 ID：`user:web-prototype-design-system`
- 来源项目类型：`prototype`

## 源证据

| 原始文件 | SHA-256 | 提取内容 | 保存位置 |
|---|---|---|---|
| `brand-spec.md` | `33438cfa57d6bd4870d65aec80a1b1ad65fffd66332213d33ba7d115de0bb87d` | 六个核心颜色、字体、四栏布局、设备身份与文件状态规则 | 根目录原文件 |
| `xchat-desktop-prototype.html` | `c176cec9402073545f148983dde756a7e1544a92d3dde1fd75405c662f55bafe` | 76 KB 完整交互原型、组件 CSS、响应规则、状态数据与交互文案 | 根目录原文件、`examples/xchat-desktop-prototype.html` |
| `image.png` | `494742ba03785fa8645bf47ad7872ed83c8af9d659fff8c4fb036d78e3839eb0` | 微信桌面四栏层级、冷灰导航、整行绿色选中态、输入区比例 | 根目录原文件、`assets/xchat-desktop-reference.png` |
| `image-1.png` | `ebefe86c2b74d4c9c5013b26e8da40778b57a3370187a7fb2f2080f3317d9f98` | 完整边框消息输入器、底部工具栏、输入状态 | 根目录原文件、`assets/xchat-composer-reference.png` |

## 证据到规则

- 参考截图确定了低装饰浅灰导航、固定列表宽度、主工作区留白与整行绿色选中语言。
- 品牌规范确定了 OKLch 六令牌、系统无衬线 / 宋体 / 等宽字体分工。
- 源原型确定了 56 / 280 / 弹性主区 / 240 四栏骨架、860px 与 1000px 响应断点。
- 源原型确定了会话、设备、消息、文件、传输、设置、对话框、Toast、拖放与 AI 卡片状态。
- 源数据确定了 MAC 稳定身份、备注优先、UDP 自动发现、手动添加和跨 VLAN / WireGuard 语境。
- 源交互确定了 Enter 发送、Shift + Enter 换行、截图快捷键、文件拖放、主题持久化和已读收据。

## 缺失证据

源文件没有提供独立 Logo、字标、安装图标、托盘图标或字体文件。设计系统因此保留参考截图、内联图标与头像语言，不生成臆造品牌资产。若后续补充真实文件，应放入 `assets/`、`build/` 或 `fonts/`，并同步更新 `DESIGN.md`、预览清单与本页。

