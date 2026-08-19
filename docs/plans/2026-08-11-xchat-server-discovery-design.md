# Xchat-server 可选跨网段发现中心设计

## 背景

Xchat 当前通过 UDP 广播、链路本地组播和自定义节点单播发现设备。该机制可以在同一二层网络中实现零配置发现，但不能可靠跨越 VLAN 或路由边界。现场网络同时存在 `192.168.10.x` 与 `192.168.20.x`：TCP 服务能够互访，但 UDP 发现报文无法完成跨网段往返。

本设计增加一个可选的 `Xchat-server`。它只承担跨网段设备登记、在线状态和工作区准入，聊天消息和文件仍由客户端直接传输。未配置服务器时，Xchat 保持当前纯局域网工作方式。

## 已确认决策

- 每家公司或局域网独立部署一套，单租户。
- 首期使用一个工作区密钥，不实现账号登录。
- 服务器只承担发现、在线状态和工作区认证，不中继消息或文件。
- 与客户端保存在同一仓库，构建为独立 Rust binary 和 Docker image。
- 在线目录只保存在内存，服务器重启后由客户端重新注册。
- 注册中心是可选增强；关闭、未配置或不可用时，同网段功能不受影响。
- Docker 同时支持 HTTP/WS 内网模式和 HTTPS/WSS 代理模式。
- Windows 与 macOS 共用同一套前端和 Rust 实现。

## 目标

- 让可路由但不共享广播域的设备自动互相发现。
- 保留同网段零配置发现和现有点对点数据路径。
- 注册中心发生故障时不阻塞客户端启动或同网段通信。
- 为未来账号、设备凭证和权限体系保留清晰的认证 seam。
- 使用现有 Axum、Tokio、WebSocket 和 `PeerManager`，避免建立第二套客户端状态模型。
- 提供可直接部署的非 root Docker image、健康检查和 TLS 代理示例。

## 非目标

- 账号登录、用户管理、管理后台或多租户。
- 消息中继、离线消息、聊天记录或文件的服务器存储。
- PostgreSQL、SQLite 或其他服务器持久化数据库。
- 高可用注册中心集群。
- 改变现有聊天、群聊、回执、撤回和文件传输协议。
- 在首期替换或重新设计当前 UDP wire protocol。

## 已比较方案

### A. 网络侧转发广播或组播

由路由器、UDP helper 或组播代理连接两个网段。客户端改动最少，但依赖特定网络设备和运维权限，扩大广播范围，并且迁移到其他企业网络时需要重新配置。它可以作为现场网络优化，不作为 Xchat 的产品级发现机制。

### B. 静态自定义节点

复用现有 `custom_peers`，让客户端定向联系 `192.168.20.139:8888`。实现量最小，但只会发现被配置的节点，不能自动获得其他在线设备；当前现场的 UDP 单播也未得到回应。该能力继续保留用于兼容和应急，不承担跨网段目录职责。

### C. 混合发现（采用）

同网段继续使用 UDP，跨网段客户端主动连接一个轻量 WebSocket 注册中心。注册中心返回在线设备目录，客户端获得地址后仍直接建立现有 TCP/WebSocket 连接。该方案不依赖跨 VLAN UDP，能够复用已经确认可达的 TCP 路径。

## 总体架构

```text
192.168.10.x 客户端 ──┐
                      ├── WS/WSS ── Xchat-server
192.168.20.x 客户端 ──┘                 │
                                       └── 仅维护在线目录

客户端 A ─────────── 现有 TCP/WebSocket ─────────── 客户端 B
             消息、群聊协议和文件均不经过服务器
```

客户端内部由一个深 `DiscoveryEngine` module 合并全部发现来源：

```text
LAN UDP adapter ─────┐
Registry WS adapter ─┼── DiscoveryEngine ── PeerManager / SQLite / UI events
SQLite cache ────────┘
```

调用方只需要启动发现、更新配置和读取状态，不需要知道 UDP、WebSocket、租约、地址优先级或重连细节。远端注册中心属于自有网络依赖；生产使用 WebSocket adapter，测试使用内存 adapter。

建议的外部 interface 为：

```rust
let discovery = DiscoveryEngine::start(context, config).await?;
discovery.configure(config).await?;
let status = discovery.status().await;
discovery.shutdown().await;
```

具体 adapter、事件合并和持久化操作保持在 module 内部，不暴露给桌面、Web 或移动入口。

## 仓库与构建结构

- 新增独立 binary：`xchat-server`。
- 新增 `server` Cargo feature，避免要求桌面平台依赖才能构建容器。
- 客户端与服务器共享版本化注册协议类型和验证逻辑。
- 保留现有 `lanchat-web`，不将其改造成注册中心，也不改变其兼容用途。
- Docker 只构建 `xchat-server` binary。

首期不拆分仓库。只有当客户端和服务器需要独立发布节奏或独立团队维护时，才考虑提取共享协议 crate 并拆仓。

## 注册中心 interface

### 端点

- `GET /healthz`：无需认证的容器健康检查，只返回服务版本和注册协议版本，例如 `{"status":"ok","version":"0.1.0","registry_protocol_version":1}`。
- `GET /discovery/v1/ws`：注册、心跳、快照和增量事件 WebSocket。
- `GET /discovery/v1/ws?probe=1`：完成相同认证，但不加入在线目录，供设置页“测试连接”使用。

不提供公开设备列表 HTTP 端点，避免绕过持续连接、认证和租约规则。

### 连接顺序

1. 客户端建立 WS 或 WSS 连接。
2. 服务器生成一次性随机挑战，30 秒后过期。
3. 客户端使用工作区密钥签名注册 payload。
4. 服务器验证挑战、签名、字段范围和协议版本。
5. 服务器使用连接来源 IP 与客户端声明的监听端口生成主要 endpoint。
6. 服务器原子登记当前客户端并返回在线快照，快照排除客户端自身。
7. 服务器向其他已认证连接推送 `peer_upsert`。
8. 客户端按照服务器下发的间隔发送心跳。
9. 连接正常关闭或租约过期后，服务器推送 `peer_offline`。

### 消息类型

服务器到客户端：

- `challenge`：注册协议版本、一次性 nonce 和过期时间。
- `ready`：心跳间隔、租约时长和当前在线快照。
- `peer_upsert`：新设备上线或在线元数据更新。
- `peer_offline`：设备连接关闭或租约过期。
- `error`：稳定错误码和可显示说明。

客户端到服务器：

- `authenticate`：原始注册 payload 与 HMAC proof。
- `heartbeat`：刷新当前连接的租约。
- `presence`：已认证连接更新名称、主机名或 capabilities。

注册 payload 包含：

```text
peer_id
name
listen_port
hostname（可选）
mac_address（可选）
peer_protocol_version
capabilities
```

注册中心 wire protocol 独立使用 `registry_protocol_version=1`；payload 中的 `peer_protocol_version` 表示客户端现有点对点协议能力，两者不能混用。

HMAC 针对固定上下文 `xchat-discovery-v1`、服务器 nonce 和未经重新编码的 payload 字节计算，避免依赖 JSON 字段排序。nonce 每条连接只能使用一次，因此截获的 proof 不能在新连接中重放。`probe=1` 验证成功后返回 `probe_ok` 并关闭连接，不返回快照也不登记设备。

## 在线目录和租约

- 目录以 `peer_id` 为键，只存在于进程内存。
- 默认心跳间隔为 5 秒，租约为 20 秒；具体值由服务器在 `ready` 中下发。
- 同一 `peer_id` 建立新连接时，新连接取代旧连接，并记录设备 ID 冲突日志。
- 正常关闭立即移除；异常断线在租约过期后移除。
- 容器重启后目录为空，客户端按照重连策略重新登记。
- 单条 WebSocket 消息最大 8 KiB。
- `peer_id` 和名称最大 128 字节，hostname 最大 255 字节，MAC 字段最大 64 字节。
- capabilities 最多 32 项，每项最大 64 字节。
- 在线目录最多 4096 台设备，超限返回 `capacity_exceeded`，不能无限增长。

注册中心不主动连接客户端，不探测消息端口，也不持久化设备目录。

## 地址与来源可信度

普通模式直接使用 TCP 连接的来源 IP。客户端只能声明监听端口，不能任意指定对其他客户端公开的主要 IP。

TLS 代理模式允许从受信任代理读取转发来源地址，但必须同时满足：

- `XCHAT_TRUST_PROXY=true`；
- `xchat-server` 端口不发布到宿主机，只能从 Docker 内部网络访问；
- 反向代理覆盖客户端提供的转发头，不能透传未校验值。

这样可以避免外部客户端伪造 `X-Forwarded-For`。如果网络存在会隐藏真实客户端地址的额外 NAT，则需要网络侧保留来源地址；首期不接受客户端自报任意 IP 作为绕过方案。

## 工作区认证

工作区密钥是高熵随机 secret，只用于控制谁能加入企业目录。

- 服务器优先从 Docker Secret 文件读取密钥。
- 服务器缺少密钥时启动失败，不能静默运行成匿名目录。
- 客户端使用 HMAC-SHA256 challenge-response，密钥本身不通过网络发送。
- nonce 一次性使用并有短过期时间。
- 服务器将单个来源的认证失败限制为每分钟 10 次，超限返回 `rate_limited`。
- 密钥、proof 和完整认证 payload 不写入日志。

明文 WS 模式可以证明客户端持有密钥并防止简单重放，但不会隐藏设备元数据。需要保密性的环境必须使用 WSS。

工作区密钥不能证明单台设备的独立身份。持有同一密钥的客户端理论上可以声明其他 `peer_id`；未来账号体系应使用用户 access token 和逐设备凭证替代共享密钥 adapter。

## 客户端发现合并

每个 `peer_id` 可以同时拥有 `lan`、`registry` 和 `cache` observation。每种在线来源维护独立的新鲜度，派生设备状态和可用地址时遵循：

```text
近期 LAN 地址 > 有效 registry 地址 > SQLite 历史地址
```

- 以 `peer_id` 去重，UI 只显示一个设备。
- LAN observation 有效时优先走同网段地址。
- LAN observation 过期但 registry 租约仍有效时切换到跨网段地址。
- 所有在线 observation 都失效后才标记离线。
- 多来源重复发现只触发一次上线或重连事件，避免重复执行挂起消息补发。
- 注册中心重连后的完整快照替换旧 registry observations，不覆盖仍有效的 LAN observations。
- 禁用注册中心时停止 Registry adapter，并使 registry observations 失效；历史联系人继续以离线状态保留。

现有 `custom_peers` 继续作为独立的定向节点功能，不与注册中心地址混用。

## 客户端设置界面

复用“设置 → 网络”，增加：

- `跨网段发现` 开关，默认关闭。
- `注册中心地址` 输入框。
- `工作区密钥` 密码输入框。
- `测试连接` 操作。
- 实时状态：未配置、连接中、已连接、认证失败、连接失败。

行为规则：

- 关闭时保留已保存地址和密钥，但不建立服务器连接。
- 开启时地址和密钥必填。
- 地址支持 `ws://`、`wss://`、域名和 `IP:端口`；未写 scheme 的 `IP:端口` 解释为 `ws://`。
- 保存成功后立即重配 adapter，无需重启客户端。
- 测试连接使用尚未保存的表单值和 `probe=1`，不得产生上线/离线事件。
- 使用明文 WS 时显示“连接未加密”的弱警示，而不是阻止连接。
- 已保存的密钥不从 Rust 后端回传给前端，只返回 `has_workspace_key`。

客户端 SQLite 只保存：

```text
registry_enabled
registry_url
```

工作区密钥保存在 macOS Keychain 或 Windows Credential Manager。安全存储失败时保存操作失败并显示原因，不能退回 SQLite 明文保存。

## 客户端重连和错误处理

- 网络错误使用 1、2、4、8、15 秒上限的带抖动退避，确保服务器恢复后 30 秒内至少重试一次。
- 认证失败停止高频重试；配置变化或用户手动测试后再立即尝试。
- 错误码固定为 `auth_failed`、`protocol_unsupported`、`invalid_payload`、`capacity_exceeded` 和 `rate_limited`；协议不兼容时显示升级提示，不持续刷日志。
- 注册中心断开只使 registry source 降级，不停止 UDP adapter，不阻塞客户端界面。
- 服务器恢复后获取新快照并恢复跨网段在线状态。
- UI 状态必须区分“注册中心不可用”和“本地 Xchat 后端不可用”。

## Docker 部署

发布一个 `xchat-server` image，使用多阶段构建，最终镜像仅包含服务器二进制和必要运行库，并以非 root 用户运行。

### 普通内网模式

直接将容器端口映射到宿主机：

```text
客户端 ── ws://server:8888 ── xchat-server:8080
```

该模式适合受信任网络，设备元数据不加密。

### TLS 模式

通过单独的 Compose 示例增加 Caddy：

```text
客户端 ── wss://xchat.company.lan ── TLS proxy ── xchat-server:8080
```

TLS 模式不向宿主机发布 `xchat-server` 的明文端口，证书由代理挂载和终止。客户端必须信任企业证书链。Nginx 等其他代理只需遵守相同的转发头和端口隔离约束，不另行维护第二套示例。

### 运行配置

```text
XCHAT_LISTEN_ADDR=0.0.0.0:8080
XCHAT_WORKSPACE_KEY_FILE=/run/secrets/xchat_workspace_key
XCHAT_TRUST_PROXY=false
RUST_LOG=info
```

普通模式和 TLS 模式使用同一个 image；差异只存在于部署配置。容器提供健康检查并配置 `restart: unless-stopped`。首期没有服务器数据卷。

## 可观测性

- `/healthz` 返回运行状态、构建版本和注册协议版本，不返回设备或密钥信息。
- 日志记录连接、认证失败原因类别、注册、租约过期和设备 ID 冲突。
- 日志不得包含工作区密钥、HMAC proof 或完整认证 payload。
- 在线数量可以作为结构化日志字段输出，首期不增加监控系统或管理页面。

## 兼容与发布顺序

1. 先发布 `xchat-server` image。
2. 再发布支持注册中心的客户端。
3. 客户端升级后注册中心默认关闭，行为与当前版本一致。
4. 管理员部署容器并生成工作区密钥。
5. Windows 和 macOS 客户端填写相同地址与密钥后启用。

旧客户端继续通过 UDP 工作，不理解也不需要连接 `Xchat-server`。新客户端连接旧版本或错误服务时显示协议不兼容，不影响本地发现。

## 测试策略

### 协议和状态测试

- HMAC 正确、错误密钥、过期 nonce、重复 nonce 和被修改 payload。
- 消息大小、字段长度、监听端口、capabilities 数量和协议版本校验。
- 注册、心跳、正常退出、租约过期和相同 `peer_id` 重连。
- 使用可控时钟验证租约，不依赖真实等待。
- 使用内存 Registry adapter 验证来源合并、地址优先级和单次重连事件。

### 集成测试

- 启动真实临时端口服务器，让两个 WebSocket 客户端完成认证并互收事件。
- `probe=1` 认证成功但不进入快照。
- 服务器重启后客户端重连并重新形成快照。
- 代理模式仅在信任代理且明文端口不公开时采用转发地址。
- 注册中心不可达时 UDP adapter 保持运行。

### Docker 和客户端烟测

- 构建镜像并验证非 root 用户、健康检查和正常停止。
- 验证普通 WS 部署与带测试证书的 WSS 代理部署。
- Windows 与 macOS 分别验证设置保存、安全存储、状态提示和自动重连。
- 在两个实际路由网段验证发现、文字、群聊、撤回和文件直传。

## 验收标准

1. 新安装和升级后的客户端默认不连接注册中心，现有同网段行为不变。
2. 两个不同网段客户端连接同一注册中心后，10 秒内互相显示。
3. 跨网段文字、群聊协议和文件继续点对点传输，不经过服务器。
4. 停止服务器后，同网段发现与通信继续正常。
5. 服务器重启后，在线目录在 30 秒内自动恢复。
6. 错误工作区密钥无法注册，客户端显示明确错误且不高频重试。
7. 工作区密钥不出现在 SQLite、日志、状态快照或服务器响应中。
8. 同一设备经 UDP 与注册中心同时发现时，列表中只有一个条目。
9. Windows 与 macOS 的设置、连接状态和降级行为一致。

## 未来账号体系演进

未来增加账号管理时，保持注册中心的发现 interface 不变，只替换认证 adapter：

- PostgreSQL 持久化企业用户、设备绑定、会话和权限。
- 用户登录换取短期 access token，设备获得独立可撤销凭证。
- 注册连接使用 access token 与设备凭证，不再共享工作区密钥。
- 在线目录仍保持内存态；账号数据与 presence 生命周期分开。

只有在客户端之间的直接 TCP 连接不再成立，或产品明确需要离线跨设备同步时，才单独设计消息中继与服务器历史记录。
