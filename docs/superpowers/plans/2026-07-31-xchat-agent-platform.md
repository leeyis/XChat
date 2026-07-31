# XChat AI Agent / MCP / Skill 平台实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. This is a later milestone and must not block the FeiQ collaboration or remote-assist releases.

**Goal:** 后续为 XChat 增加本地模型连接、用户自建 Agent、MCP/Skill 配置，以及“仅本机可用 / 向局域网暴露”的 Agent 发现与对话能力。

**Architecture:** Agent 是设备上的逻辑身份，不替代 MAC 设备身份；Agent 配置和秘密留在本机，发现帧只公开安全元数据。普通消息、Agent 对话、MCP 工具调用和 Skill 执行分层，远程用户只能通过公开 capability 进入 Agent 对话，不能改变配置或注入任意脚本。

**Tech Stack:** React 19.2.8、Rust/Tokio、SQLite/sqlx、现有 WebSocket/TCP discovery、模型 HTTP API、受控 MCP transport、Skill 包管理与本机沙箱（实现时单独选定受支持 runtime）。

## Global Constraints

- 第一阶段只预留 `agent.chat.v1` capability、`kind = 'agent'` 会话位和事件命名空间，不实现 Agent runtime。
- API key/token 只存本机安全存储或受保护配置，永不进入 discovery/message/log。
- `private` Agent 不广播；`lan` Agent 只广播 ID、名称、版本、公开描述和能力摘要。
- MCP/Skill 执行必须经过本机授权与沙箱；远端不能调用任意文件、网络或进程。
- Agent 逻辑不塞入普通 `message.sendText` 分支；使用独立事件与会话状态。

### Task 1: 模型连接和秘密边界

**Files:**
- Create: `src-tauri/src/agent_config.rs`
- Modify: `src-tauri/src/db.rs`
- Modify: `src-tauri/src/commands.rs`, `src-tauri/src/web_server.rs`
- Modify: `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`
- Modify: `src-tauri/permissions/commands.toml` and relevant capability JSON
- Modify: `frontend/src/App.jsx`, `frontend/src/xchat.js`
- Test: Rust config tests, `frontend/src/xchat.test.js`

**Interfaces:**
- `ModelConnection { id, name, provider, base_url, model, protocol, secret_ref, enabled }`。
- Commands/routes: `list_model_connections`, `save_model_connection`, `delete_model_connection`, `test_model_connection`。
- Adapter actions: `agent.modelConnection.save`, `agent.modelConnection.test`。

- [ ] 写测试证明序列化输出不含 secret 明文；日志和错误只显示连接名称与状态码。
- [ ] 建立 `model_connections` 表，凭据只存 `secret_ref`；实现 URL/模型名/协议校验和连通性测试。
- [ ] 设置页增加模型连接管理入口，但不把它混入普通通知/网络设置字段。
- [ ] 运行 Rust tests、desktop/web checks 和前端 build。
- [ ] 提交 `feat: add local model connection storage`。

### Task 2: Agent 配置与本机运行边界

**Files:**
- Create: `src-tauri/src/agents.rs`
- Modify: `src-tauri/src/db.rs`, `src-tauri/src/workspace.rs`
- Modify: `src-tauri/src/commands.rs`, `src-tauri/src/web_server.rs`
- Modify: `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`
- Modify: `src-tauri/permissions/commands.toml` and relevant capability JSON
- Modify: `frontend/src/App.jsx`, `frontend/src/xchat.js`, `frontend/src/styles.css`
- Test: Rust agent config/state tests, `frontend/src/xchat.test.js`

**Interfaces:**
- `AgentRecord { id, name, description, system_prompt, model_connection_id, visibility, version, enabled }`。
- Commands/routes: `list_agents`, `create_agent`, `update_agent`, `delete_agent`, `set_agent_visibility`。
- Adapter actions: `agent.create`, `agent.update`, `agent.delete`, `agent.setVisibility`。

- [ ] 写状态测试：private Agent 不出现在 discovery snapshot；删除模型连接会阻止依赖 Agent 启动。
- [ ] 添加 `agents` 表和本机配置校验；Agent ID 与 peer/MAC ID 分离。
- [ ] 设置页提供“仅自己可用 / 局域网可发现”选择，保存时明确说明公开哪些元数据。
- [ ] 在 workspace snapshot 预留 `agents` 数组和 `agent.changed` 事件；没有 Agent 能力时不渲染主导航入口。
- [ ] 提交 `feat: add local agent configuration model`。

### Task 3: MCP 与 Skill 绑定模型

**Files:**
- Create: `src-tauri/src/agent_tools.rs`
- Modify: `src-tauri/src/db.rs`, `src-tauri/src/agents.rs`
- Modify: `src-tauri/src/commands.rs`, `src-tauri/src/web_server.rs`
- Modify: `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`
- Modify: `src-tauri/permissions/commands.toml` and relevant capability JSON
- Modify: `frontend/src/App.jsx`, `frontend/src/xchat.js`, `frontend/src/styles.css`
- Test: Rust authorization tests, frontend reducer tests

**Interfaces:**
- `McpBinding { agent_id, name, transport, endpoint, allowed_tools, enabled, authorization_state }`。
- `SkillBinding { agent_id, skill_id, version, source, enabled, authorization_state }`。
- Actions: `agent.mcp.bind`, `agent.mcp.authorize`, `agent.skill.bind`, `agent.skill.enable`。

- [ ] 写测试证明未授权 MCP/Skill 不能执行，远端不能改变本机绑定。
- [ ] 添加 `agent_mcp_bindings` 和 `agent_skill_bindings` 表；endpoint、文件路径和工具列表都做 allowlist 校验。
- [ ] 设置页用独立 Agent 详情展示绑定，不把 MCP/Skill 混入共享文件或普通网络设置。
- [ ] 先实现配置和授权状态，不把具体 MCP/Skill runtime 直接塞进 UI。
- [ ] 提交 `feat: add agent MCP and Skill bindings`。

### Task 4: 局域网 Agent 发现与对话

**Files:**
- Modify: `src-tauri/src/network/discovery.rs`, `src-tauri/src/network/protocol.rs`, `src-tauri/src/network/messaging.rs`
- Modify: `src-tauri/src/peers.rs`, `src-tauri/src/db.rs`, `src-tauri/src/workspace.rs`
- Modify: `frontend/src/xchat.js`, `frontend/src/App.jsx`
- Test: Rust discovery tests, frontend event normalization tests, two-instance Web smoke

**Interfaces:**
- Discovery payload: `agent_id`, `owner_device_id`, `name`, `version`, `description`, `public_capabilities` only。
- Conversation kind: `agent` with `agent_id` and owner peer ID。
- Events: `agent.changed`, `agent.session.changed`, `agent.message.changed`。

- [ ] 写 discovery 测试：private Agent 完全不广播；lan Agent 不包含 base_url、secret_ref、MCP endpoint 或 Skill 内容。
- [ ] 在 Peer 的能力命名空间中挂载 Agent 元数据，但保持 MAC 作为设备稳定身份。
- [ ] 为 Agent 会话增加独立消息路由和本地/远端可见性判断，不让普通 direct fallback 把 Agent 当设备。
- [ ] 前端在有公开 Agent 时增加来源分组和 Agent 对话入口；无 capability 时保持现有聊天布局不变。
- [ ] 双实例验证发现、私有隐藏、公开对话、远端停止和断线清理。
- [ ] 提交 `feat: discover and chat with LAN agents`。

### Task 5: Agent runtime 安全门槛

**Files:**
- Create: `src-tauri/src/agent_runtime.rs`
- Modify: `src-tauri/src/agents.rs`, `src-tauri/src/agent_tools.rs`
- Modify: `src-tauri/src/commands.rs`, `src-tauri/src/web_server.rs`
- Modify: `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`
- Modify: `src-tauri/permissions/commands.toml` and relevant capability JSON
- Test: Rust sandbox/timeout/permission tests

- [ ] 写测试证明单次 Agent 请求有超时、最大输出、工具调用次数和取消路径；取消后不残留子进程或临时文件。
- [ ] 实现模型调用 adapter，配置内容和工具输出分离；模型异常不把秘密写入 UI 或日志。
- [ ] 实现 MCP/Skill allowlist、用户授权、执行超时和资源上限；拒绝任意路径、任意命令和未授权网络。
- [ ] 为 `agent.message.changed` 提供流式思考/工具/结果三类状态，复用 UI kit 的 AI 卡片语言，不污染普通消息气泡。
- [ ] 运行 Rust tests、desktop/web compile 和隔离本地 Agent smoke。
- [ ] 提交 `feat: add sandboxed agent runtime`。
