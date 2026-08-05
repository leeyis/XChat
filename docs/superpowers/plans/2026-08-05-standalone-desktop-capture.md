# Standalone Desktop Capture Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let macOS, Windows, and Linux desktop users open the screenshot editor without any discovered LAN host or active chat, while keeping chat completion conversation-bound and native Copy desktop-only.

**Architecture:** The workspace dispatcher treats desktop capture capability and chat selection as separate concerns: Tauri may start a capture with `conversationId: null`, while Web keeps its existing conversation requirement. Rust accepts an optional conversation ID and stores it in the existing optional capture field. The editor derives action availability from the pending capture: Copy requires native Tauri IPC, while Done requires either a conversation ID or pin-edit mode.

**Tech Stack:** React 19, dependency-free workspace JavaScript, Node test runner, Tauri 2, Rust, Vite 8.

## Global Constraints

- macOS、Windows 和 Linux 桌面端在没有局域网主机、没有历史会话或没有活动会话时，仍可通过截图快捷键进入截图编辑器。
- 独立截图模式支持取消、钉图、复制和保存。
- 只有“完成并加入聊天草稿”依赖活动会话；独立截图模式不得创建无归属附件。
- 不自动创建聊天会话、虚拟主机或本地自聊会话。
- 不改变 Web 截图是否可用的浏览器能力判断。
- 不为 headless Web 增加原生图片剪贴板复制行为。
- 不为 Android 增加截图能力。
- Prefix every shell command with `rtk`; keep Rust formatting local to touched code and preserve unrelated user changes.

---

## File Map

- `frontend/src/xchat.js`: separate platform capability from conversation selection and pass an optional conversation ID to Tauri.
- `frontend/src/xchat.test.js`: exercise the real workspace dispatcher with a mocked Tauri IPC boundary.
- `src-tauri/src/capture_editor.rs`: validate and store an optional capture conversation ID.
- `src-tauri/src/commands.rs`: expose `Option<String>` through the existing Tauri command.
- `frontend/src/capture-drawing.js`: provide a small pure action-availability decision used by the editor.
- `frontend/src/capture-drawing.test.js`: protect standalone, conversation-bound, Web, and pin-edit availability rules.
- `frontend/src/CaptureEditor.jsx`: hide native Copy outside Tauri and disable Done for standalone captures.
- `src/index.html` and `src/assets/`: regenerate Vite output from the updated frontend source.

### Task 1: Start Desktop Capture Without a Conversation

**Files:**
- Modify: `frontend/src/xchat.test.js`
- Modify: `frontend/src/xchat.js:2174-2185`

**Interfaces:**
- Consumes: `createXChatModule()`, `TauriAdapter.startCapture(conversationId)`, backend workspace capability `capture: boolean`.
- Produces: desktop `capture.start` calls `start_capture_editor` with `{ conversationId: string | null }`; Web without a conversation returns `capture_conversation_required`.

- [ ] **Step 1: Write the failing Tauri workspace test**

Import `createXChatModule` from `./xchat.js`, then add this test-only helper and the failing test near the existing Tauri capture tests. The complete workspace fixture is intentional: it mirrors the fields returned by the real snapshot boundary.

```js
async function withTauriCaptureWorkspace(capture, run) {
  const windowDescriptor = Object.getOwnPropertyDescriptor(globalThis, "window");
  const calls = [];
  let workspace;
  try {
    Object.defineProperty(globalThis, "window", {
      configurable: true,
      value: {
        __TAURI__: {
          core: {
            invoke(command, payload) {
              calls.push([command, payload]);
              if (command === "get_workspace_snapshot") {
                return Promise.resolve({
                  self: { id: "self", name: "Me", hostname: "mac", addr: "" },
                  devices: [],
                  conversations: [],
                  files: [],
                  transfers: [],
                  settings: { language: "zh-CN", capture_shortcut: "⌘ ⇧ A" },
                  capabilities: { capture, captureShortcut: capture },
                });
              }
              if (command === "start_capture_editor") {
                return Promise.resolve({ session_id: "capture", conversation_id: null });
              }
              return Promise.resolve([]);
            },
          },
          event: { listen: () => Promise.resolve(() => {}) },
        },
      },
    });

    workspace = createXChatModule();
    assert.equal((await workspace.dispatch({ type: "bootstrap" })).ok, true);
    await run({ workspace, calls });
  } finally {
    await workspace?.dispatch({ type: "shutdown" });
    if (windowDescriptor) {
      Object.defineProperty(globalThis, "window", windowDescriptor);
    } else {
      delete globalThis.window;
    }
  }
}

test("Tauri capture starts without a discovered conversation", async () => {
  await withTauriCaptureWorkspace(true, async ({ workspace, calls }) => {
    const result = await workspace.dispatch({ type: "capture.start" });

    assert.equal(result.ok, true);
    assert.deepEqual(
      calls.find(([command]) => command === "start_capture_editor"),
      ["start_capture_editor", { conversationId: null }],
    );
  });
});
```

- [ ] **Step 2: Run the focused test and confirm the red state**

Run:

```bash
rtk node --test --test-name-pattern="Tauri capture starts without" frontend/src/xchat.test.js
```

Expected: FAIL because `capture.start` returns `capture_unsupported` before invoking `start_capture_editor`.

- [ ] **Step 3: Separate capability and conversation checks**

Replace the current combined `capture.start` condition with:

```js
case "capture.start": {
  const conversation = activeConversation();
  if (!snapshot.capabilities.capture) {
    throw new TransportError(
      uiCopy("当前平台不支持截屏", "The current platform does not support screen capture"),
      "capture_unsupported",
      0,
      false,
    );
  }
  if (!conversation && adapter.runtime !== "tauri") {
    throw new TransportError(
      uiCopy("请先选择一个会话", "Select a conversation first"),
      "capture_conversation_required",
      0,
      false,
    );
  }
  return adapter.startCapture(conversation?.id ?? null);
}
```

Do not alter browser capability detection or create a conversation implicitly.

- [ ] **Step 4: Add the unsupported-platform regression assertion**

Use the same helper for a second exact test:

```js
test("unsupported desktop capture never invokes the backend", async () => {
  await withTauriCaptureWorkspace(false, async ({ workspace, calls }) => {
    const result = await workspace.dispatch({ type: "capture.start" });

    assert.equal(result.ok, false);
    assert.equal(result.error.code, "capture_unsupported");
    assert.equal(
      calls.some(([command]) => command === "start_capture_editor"),
      false,
    );
  });
});
```

- [ ] **Step 5: Run focused and complete frontend tests**

Run:

```bash
rtk node --test --test-name-pattern="Tauri capture starts without|unsupported desktop capture" frontend/src/xchat.test.js
rtk npm test
```

Expected: both focused tests and the complete frontend suite PASS.

- [ ] **Step 6: Commit the standalone workspace route**

```bash
rtk git add frontend/src/xchat.js frontend/src/xchat.test.js
rtk git commit -m "fix: allow standalone desktop capture"
```

### Task 2: Accept an Optional Conversation in the Native Capture Session

**Files:**
- Modify: `src-tauri/src/capture_editor.rs:399-418`
- Modify: `src-tauri/src/capture_editor.rs` test module
- Modify: `src-tauri/src/commands.rs:2031-2036`

**Interfaces:**
- Consumes: Tauri payload `conversationId: string | null`.
- Produces: `capture_editor::start(app: &tauri::AppHandle, conversation_id: Option<String>)` and `start_capture_editor(..., conversation_id: Option<String>)`; pending captures preserve `conversation_id: Option<String>`.

- [ ] **Step 1: Write the failing optional-ID validation test**

Add this focused test in `capture_editor.rs`'s existing test module:

```rust
#[test]
fn standalone_capture_accepts_no_conversation_but_rejects_invalid_ids() {
    assert_eq!(validate_capture_conversation_id(None).unwrap(), None);
    assert_eq!(
        validate_capture_conversation_id(Some("conversation-1".to_string())).unwrap(),
        Some("conversation-1".to_string())
    );
    assert_eq!(
        validate_capture_conversation_id(Some("   ".to_string())).unwrap_err(),
        "无效的会话 ID"
    );
    assert_eq!(
        validate_capture_conversation_id(Some("x".repeat(257))).unwrap_err(),
        "无效的会话 ID"
    );
}
```

- [ ] **Step 2: Run the focused Rust test and confirm the red state**

Run:

```bash
rtk cargo test --manifest-path src-tauri/Cargo.toml --lib standalone_capture_accepts_no_conversation_but_rejects_invalid_ids
```

Expected: FAIL because `validate_capture_conversation_id` is not defined.

- [ ] **Step 3: Add optional conversation validation**

Add immediately before `start`:

```rust
fn validate_capture_conversation_id(
    conversation_id: Option<String>,
) -> Result<Option<String>, String> {
    if conversation_id
        .as_deref()
        .is_some_and(|value| value.trim().is_empty() || value.len() > 256)
    {
        return Err("无效的会话 ID".to_string());
    }
    Ok(conversation_id)
}
```

Change `start` to accept `Option<String>` and begin with:

```rust
let conversation_id = validate_capture_conversation_id(conversation_id)?;
```

Remove the old unconditional string validation. When building `CaptureFile`, assign the already optional value directly:

```rust
conversation_id,
```

- [ ] **Step 4: Update the Tauri command signature**

Change the command wrapper to:

```rust
#[tauri::command]
pub async fn start_capture_editor(
    app: AppHandle,
    conversation_id: Option<String>,
) -> Result<crate::capture_editor::CaptureSessionSummary, String> {
    crate::capture_editor::start(&app, conversation_id).await
}
```

No registration or permission identifier changes are required because the command name is unchanged.

- [ ] **Step 5: Run the focused test and desktop compile check**

Run:

```bash
rtk cargo test --manifest-path src-tauri/Cargo.toml --lib standalone_capture_accepts_no_conversation_but_rejects_invalid_ids
rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib
```

Expected: the focused test passes and the desktop library compiles with the optional Tauri argument.

- [ ] **Step 6: Commit the native optional session**

```bash
rtk git add src-tauri/src/capture_editor.rs src-tauri/src/commands.rs
rtk git commit -m "fix: accept standalone native captures"
```

### Task 3: Gate Editor Actions for Standalone and Web Modes

**Files:**
- Modify: `frontend/src/capture-drawing.test.js`
- Modify: `frontend/src/capture-drawing.js`
- Modify: `frontend/src/CaptureEditor.jsx`
- Regenerate: `src/index.html`
- Regenerate: `src/assets/index-*.js`

**Interfaces:**
- Consumes: pending capture `conversation_id: string | null`, `pinEditing: boolean`, and native IPC presence `Boolean(globalThis.__TAURI__?.core?.invoke)`.
- Produces: `captureEditorActionAvailability({ conversationId, nativeCopy, pinEditing }) -> { canCopy: boolean, canFinish: boolean }`.

- [ ] **Step 1: Write the failing action-availability test**

Import `captureEditorActionAvailability` in `capture-drawing.test.js` and add:

```js
test("capture editor actions distinguish standalone, Web, and pin editing", () => {
  assert.deepEqual(
    captureEditorActionAvailability({
      conversationId: null,
      nativeCopy: true,
      pinEditing: false,
    }),
    { canCopy: true, canFinish: false },
  );
  assert.deepEqual(
    captureEditorActionAvailability({
      conversationId: "conversation-1",
      nativeCopy: true,
      pinEditing: false,
    }),
    { canCopy: true, canFinish: true },
  );
  assert.deepEqual(
    captureEditorActionAvailability({
      conversationId: "conversation-1",
      nativeCopy: false,
      pinEditing: false,
    }),
    { canCopy: false, canFinish: true },
  );
  assert.deepEqual(
    captureEditorActionAvailability({
      conversationId: null,
      nativeCopy: true,
      pinEditing: true,
    }),
    { canCopy: false, canFinish: true },
  );
});
```

- [ ] **Step 2: Run the focused test and confirm the red state**

Run:

```bash
rtk node --test --test-name-pattern="capture editor actions distinguish" frontend/src/capture-drawing.test.js
```

Expected: FAIL because `captureEditorActionAvailability` is not exported.

- [ ] **Step 3: Implement the pure availability decision**

Add to `capture-drawing.js`:

```js
export function captureEditorActionAvailability({
  conversationId,
  nativeCopy,
  pinEditing = false,
} = {}) {
  return {
    canCopy: Boolean(nativeCopy && !pinEditing),
    canFinish: Boolean(pinEditing || conversationId),
  };
}
```

- [ ] **Step 4: Apply the decision in `CaptureOverlay`**

Import the helper in `CaptureEditor.jsx`. After `canExport`, derive:

```js
const actionAvailability = captureEditorActionAvailability({
  conversationId: pending?.conversation_id,
  nativeCopy: Boolean(globalThis.__TAURI__?.core?.invoke),
  pinEditing,
});
```

Make `finish` defensively return an accurate error if invoked without a conversation outside pin-edit mode:

```js
if (!pinEditing && !pending?.conversation_id) {
  setError(english ? "Select a conversation first." : "请先选择一个会话。");
  return;
}
```

Render Copy only when `actionAvailability.canCopy` is true. This removes the unusable Copy action from headless Web while preserving the normal desktop editor position between Pin and Save.

For Done, compute the localized label once:

```js
const finishLabel = actionAvailability.canFinish
  ? english ? "Done" : "完成"
  : english ? "Select a conversation first" : "请先选择一个会话";
```

Use `finishLabel` for `title` and `aria-label`, and set:

```jsx
disabled={!canExport || busy || !actionAvailability.canFinish}
```

Pin-edit mode remains finishable because the helper returns `canFinish: true` there.

- [ ] **Step 5: Run focused and complete frontend tests**

Run:

```bash
rtk node --test --test-name-pattern="capture editor actions distinguish" frontend/src/capture-drawing.test.js
rtk npm test
```

Expected: the focused test and the complete frontend suite PASS.

- [ ] **Step 6: Regenerate the committed frontend bundle**

Run:

```bash
rtk npm run build
```

Expected: Vite succeeds, `src/index.html` references the new content-hashed JavaScript asset, and the obsolete generated JavaScript asset is removed.

- [ ] **Step 7: Run the full cross-target verification set**

Run:

```bash
rtk npm test
rtk cargo test --manifest-path src-tauri/Cargo.toml --lib
rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib
rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features web --bin lanchat-web
rtk git diff --check
```

Expected: all commands exit successfully. The Web compile remains unchanged because no Web clipboard command is added.

- [ ] **Step 8: Commit the editor action gates and generated bundle**

```bash
rtk git add frontend/src/capture-drawing.js frontend/src/capture-drawing.test.js frontend/src/CaptureEditor.jsx src/index.html src/assets
rtk git commit -m "fix: support standalone capture actions"
```

## Final Desktop Smoke Test

After all reviewed task commits are present, build or launch the latest desktop source with an isolated port and database:

```bash
rtk cargo tauri dev -- -- --port 18888 --db-path /tmp/lanchat-agent-standalone-capture
```

With no conversations in the isolated database:

1. Press the configured screenshot shortcut.
2. Confirm the editor opens instead of showing “当前平台不支持截屏”.
3. Select an area and confirm Copy, Save, and Pin are available while Done is disabled with “请先选择一个会话”.
4. Click Copy and paste into an image-capable target to confirm PNG clipboard content.
5. Confirm closing or copying restores the main Xchat window.
