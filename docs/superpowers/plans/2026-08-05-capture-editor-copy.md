# Capture Editor Copy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a “复制” / “Copy” toolbar action that copies the edited screenshot as a PNG image to the native clipboard and closes the normal capture editor on success.

**Architecture:** The capture overlay continues to own image composition through `exportPng()`. A new workspace action forwards that PNG data URL through `TauriAdapter` to a narrowly permitted Tauri command; `capture_editor.rs` validates and decodes it, writes it with the existing `clipboard-rs` backend, then consumes the editor session and restores the main window. The pinned-capture copy path shares the final native clipboard writer but keeps its current behavior.

**Tech Stack:** React 19, dependency-free workspace JavaScript, Node test runner, Tauri 2, Rust, `clipboard-rs` 0.3.3, Vite 8.

## Global Constraints

- The button appears only in the normal capture editor, between Pin and Save.
- Copy the selected and annotated PNG image, never a file path or data URL text.
- Close and clean up only after clipboard writing succeeds; preserve the editor and show an error when it fails.
- Keep the pinned-image context-menu copy behavior unchanged.
- Support the existing macOS, Windows, and Linux desktop capture targets without adding dependencies.
- Do not add Android or headless Web capture behavior.
- Register every new Tauri command in `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`, `src-tauri/permissions/commands.toml`, and the narrow capture-editor capability.
- Prefix every shell command with `rtk` and keep Rust formatting local to touched code.

---

## File Map

- `frontend/src/xchat.js`: desktop adapter method and workspace action routing.
- `frontend/src/xchat.test.js`: adapter contract test for the new Tauri invocation.
- `frontend/src/CaptureEditor.jsx`: copy icon, copy handler, toolbar button, and stable initial toolbar width.
- `src-tauri/src/capture_editor.rs`: PNG-to-native-clipboard implementation, editor-state success/failure transition, and focused Rust test.
- `src-tauri/src/commands.rs`: Tauri command wrapper.
- `src-tauri/src/main.rs`: desktop command registration.
- `src-tauri/src/lib.rs`: library/mobile entry-point command registration.
- `src-tauri/permissions/commands.toml`: command permission declaration.
- `src-tauri/capabilities/capture-editor.json`: permission grant scoped to the capture editor window.
- `src/index.html` and `src/assets/`: Vite output regenerated from frontend source; the current `src/assets/index-DjICC-Z7.js` is replaced by Vite's new content-hashed JavaScript asset.

### Task 1: Add the Frontend IPC Contract

**Files:**
- Modify: `frontend/src/xchat.test.js:430-520`
- Modify: `frontend/src/xchat.js:951-989`
- Modify: `frontend/src/xchat.js:2182-2195`

**Interfaces:**
- Consumes: `TauriAdapter.invoke(command: string, payload?: object): Promise<unknown>`.
- Produces: `TauriAdapter.copyCapture(dataUrl: string): Promise<unknown>` and workspace action `{ type: "capture.copy", dataUrl: string }`.

- [ ] **Step 1: Write the failing adapter contract test**

Add beside the existing Tauri clipboard tests:

```js
test("Tauri capture copy forwards the edited PNG to the native command", async () => {
  const calls = [];
  const adapter = new TauriAdapter({
    core: {
      invoke(command, payload) {
        calls.push([command, payload]);
        return Promise.resolve();
      },
    },
  });
  const dataUrl = "data:image/png;base64,capture";

  await adapter.copyCapture(dataUrl);

  assert.deepEqual(calls, [["copy_capture_editor", { dataUrl }]]);
});
```

- [ ] **Step 2: Run the focused test and confirm the red state**

Run:

```bash
rtk node --test --test-name-pattern="Tauri capture copy" frontend/src/xchat.test.js
```

Expected: FAIL because `adapter.copyCapture` is not defined.

- [ ] **Step 3: Add the adapter method and workspace route**

Add next to `finishCapture`, `pinCapture`, and `saveCapture`:

```js
copyCapture(dataUrl) {
  return this.invoke("copy_capture_editor", { dataUrl });
}
```

Add between `capture.pin` and `capture.save` in the workspace dispatcher:

```js
case "capture.copy":
  return adapter.copyCapture(action.dataUrl);
```

- [ ] **Step 4: Run the focused and complete frontend tests**

Run:

```bash
rtk node --test --test-name-pattern="Tauri capture copy" frontend/src/xchat.test.js
rtk npm test
```

Expected: the focused test and the complete frontend suite PASS.

- [ ] **Step 5: Commit the IPC contract**

```bash
rtk git add frontend/src/xchat.js frontend/src/xchat.test.js
rtk git commit -m "feat: route capture copy to desktop backend"
```

### Task 2: Implement Native Clipboard Copy and Editor Cleanup

**Files:**
- Modify: `src-tauri/src/capture_editor.rs:341-363`
- Modify: `src-tauri/src/capture_editor.rs:645-749`
- Modify: `src-tauri/src/capture_editor.rs:1029-1090`
- Modify: `src-tauri/src/commands.rs:2045-2077`
- Modify: `src-tauri/src/main.rs:121-131`
- Modify: `src-tauri/src/lib.rs:105-115`
- Modify: `src-tauri/permissions/commands.toml:284-311`
- Modify: `src-tauri/capabilities/capture-editor.json:7-15`

**Interfaces:**
- Consumes: validated PNG `data_url: String`, active `CaptureState.editor`, Tauri `AppHandle`, and `clipboard-rs::RustImageData`.
- Produces: `capture_editor::copy_editor(app: &tauri::AppHandle, data_url: String) -> Result<(), String>` and Tauri command `copy_capture_editor(app: AppHandle, data_url: String) -> Result<(), String>`.

- [ ] **Step 1: Write the failing editor-state transition test**

Add to `capture_editor.rs`'s existing test module. Reuse the local `capture` constructor pattern already used by the pin replacement test:

```rust
#[test]
fn editor_copy_consumes_state_only_after_the_action_succeeds() {
    let capture = CaptureFile {
        session_id: "editor".to_string(),
        conversation_id: Some("conversation".to_string()),
        path: PathBuf::from("editor.png"),
        file_name: "capture.png".to_string(),
        file_size: 24,
        width: 1,
        height: 1,
    };
    let mut state = CaptureState {
        editor: Some(capture),
        pin: None,
    };

    let error = consume_editor_after(&mut state, || Err("剪贴板失败".to_string()))
        .err()
        .unwrap();
    assert_eq!(error, "剪贴板失败");
    assert!(state.editor.is_some());

    let copied = consume_editor_after(&mut state, || Ok(())).unwrap();
    assert_eq!(copied.session_id, "editor");
    assert!(state.editor.is_none());
}
```

- [ ] **Step 2: Run the focused Rust test and confirm the red state**

Run:

```bash
rtk cargo test --manifest-path src-tauri/Cargo.toml --lib editor_copy_consumes_state_only_after_the_action_succeeds
```

Expected: FAIL because `consume_editor_after` is not defined.

- [ ] **Step 3: Add the state transition and shared clipboard writer**

Add the state helper near `clear_editor`:

```rust
fn consume_editor_after(
    state: &mut CaptureState,
    action: impl FnOnce() -> Result<(), String>,
) -> Result<CaptureFile, String> {
    let capture = state
        .editor
        .clone()
        .ok_or_else(|| "没有待处理的截图".to_string())?;
    action()?;
    state.editor = None;
    Ok(capture)
}
```

Extract the common final clipboard write from `copy_pin`:

```rust
#[cfg(not(target_os = "android"))]
fn set_clipboard_image(
    image: clipboard_rs::RustImageData,
    failure_context: &str,
) -> Result<(), String> {
    use clipboard_rs::{Clipboard, ClipboardContext};

    ClipboardContext::new()
        .map_err(|error| format!("剪贴板不可用: {error}"))?
        .set_image(image)
        .map_err(|error| format!("{failure_context}: {error}"))
}
```

Keep `copy_pin`'s loading and scaling behavior unchanged, replacing only its final `ClipboardContext` call with:

```rust
set_clipboard_image(image, "复制钉图失败")
```

Also narrow `copy_pin`'s local import to
`use clipboard_rs::{FilterType, RustImageData};` after moving `Clipboard` and
`ClipboardContext` into the shared writer.

- [ ] **Step 4: Implement `copy_editor` with success-only cleanup**

Decode and validate before touching state. Resolve the capture editor window, create `RustImageData::from_bytes(&bytes)`, then use `consume_editor_after` while performing both the clipboard write and `window.hide()`. This ordering keeps the editor visible and its state intact whenever either operation fails:

```rust
pub fn copy_editor(app: &tauri::AppHandle, data_url: String) -> Result<(), String> {
    #[cfg(not(target_os = "android"))]
    {
        use clipboard_rs::common::RustImage;
        use clipboard_rs::RustImageData;

        let (bytes, _, _) = png_from_data_url(&data_url)?;
        let image = RustImageData::from_bytes(&bytes)
            .map_err(|error| format!("读取截图失败: {error}"))?;
        let window = app
            .get_webview_window("capture-editor")
            .ok_or_else(|| "截图编辑器窗口不可用".to_string())?;
        let capture = {
            let mut state = lock_state()?;
            consume_editor_after(&mut state, || {
                set_clipboard_image(image, "复制截图失败")?;
                window
                    .hide()
                    .map_err(|error| format!("关闭截图编辑器失败: {error}"))
            })?
        };

        remove_file(&capture.path);
        let _ = window.close();
        restore_main_window(app);
        Ok(())
    }
    #[cfg(target_os = "android")]
    {
        let _ = (app, data_url);
        Err("当前平台不支持复制截图".to_string())
    }
}
```

- [ ] **Step 5: Expose and authorize the command everywhere Tauri requires**

Add to `commands.rs` beside `save_capture_editor`:

```rust
#[tauri::command]
pub fn copy_capture_editor(app: AppHandle, data_url: String) -> Result<(), String> {
    crate::capture_editor::copy_editor(&app, data_url)
}
```

Add `commands::copy_capture_editor` in `lib.rs` and
`lanchat::commands::copy_capture_editor` in `main.rs`. Add this permission to
`commands.toml`:

```toml
[[permission]]
identifier = "allow-copy-capture-editor"
description = "Allows copying the edited screenshot to the system clipboard"
commands.allow = ["copy_capture_editor"]
```

Grant only `"allow-copy-capture-editor"` in `capture-editor.json`, adjacent to the existing save permission.

- [ ] **Step 6: Run the focused test and desktop compile check**

Run:

```bash
rtk cargo test --manifest-path src-tauri/Cargo.toml --lib editor_copy_consumes_state_only_after_the_action_succeeds
rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib
```

Expected: both commands PASS, including Tauri permission generation and command registration.

- [ ] **Step 7: Commit the native copy path**

```bash
rtk git add src-tauri/src/capture_editor.rs src-tauri/src/commands.rs src-tauri/src/main.rs src-tauri/src/lib.rs src-tauri/permissions/commands.toml src-tauri/capabilities/capture-editor.json
rtk git commit -m "feat: copy edited captures to native clipboard"
```

### Task 3: Add the Toolbar Button and Regenerate Frontend Assets

**Files:**
- Modify: `frontend/src/CaptureEditor.jsx:88-166`
- Modify: `frontend/src/CaptureEditor.jsx:417-430`
- Modify: `frontend/src/CaptureEditor.jsx:629-665`
- Modify: `frontend/src/CaptureEditor.jsx:1361-1378`
- Modify: `src/index.html`
- Delete: `src/assets/index-DjICC-Z7.js`
- Create: the content-hashed JavaScript asset emitted by `rtk npm run build`

**Interfaces:**
- Consumes: workspace action `{ type: "capture.copy", dataUrl: string }` from Task 1 and backend success/failure lifecycle from Task 2.
- Produces: a normal-editor-only Copy button with `title` and `aria-label` set to `复制` or `Copy`.

- [ ] **Step 1: Add the copy icon and stable toolbar dimensions**

Add an overlapping-pages icon to `CaptureIcon`:

```jsx
{name === "copy" && (
  <>
    <rect {...common} x="9" y="9" width="11" height="11" rx="1" />
    <path {...common} d="M15 9V4H4v11h5" />
  </>
)}
```

Increase the initial `toolbarSize.width` from `520` to `560`; the existing `ResizeObserver` remains the source of truth after first render.

- [ ] **Step 2: Add the copy handler**

Place this handler between `pin` and `save`:

```jsx
const copy = async () => {
  if (pinEditing) return;
  setBusy(true);
  setError("");
  setStatus("");
  const result = await workspace.dispatch({
    type: "capture.copy",
    dataUrl: exportPng(),
  });
  setBusy(false);
  if (!result.ok) {
    setError(result.error.message);
    return;
  }
  closeWindow();
};
```

- [ ] **Step 3: Render Copy between Pin and Save only in normal editor mode**

Insert immediately after the Pin button:

```jsx
{!pinEditing && (
  <button
    type="button"
    onClick={copy}
    disabled={!canExport || busy}
    title={english ? "Copy" : "复制"}
    aria-label={english ? "Copy" : "复制"}
  >
    <CaptureIcon name="copy" />
  </button>
)}
```

No new component-test dependency is added. The meaningful data boundary is covered by Task 1, the failure-preserving state transition by Task 2, and the rendered position and interaction by the desktop smoke test below.

- [ ] **Step 4: Run frontend tests and regenerate committed assets**

Run:

```bash
rtk npm test
rtk npm run build
```

Expected: all tests PASS; Vite replaces the old JavaScript hash in `src/assets/` and updates `src/index.html`. Do not hand-edit generated assets.

- [ ] **Step 5: Check the generated diff and commit the UI**

Run:

```bash
rtk git diff --check
rtk git status --short
```

Confirm the source change, one old generated JavaScript deletion, one new generated JavaScript file, and the `src/index.html` reference update. Then commit:

```bash
rtk git add frontend/src/CaptureEditor.jsx src/index.html src/assets
rtk git commit -m "feat: add copy button to capture toolbar"
```

### Task 4: Cross-Target Verification and Desktop Smoke Test

**Files:**
- Verify only; make no unrelated edits.

**Interfaces:**
- Consumes: the complete `capture.copy` frontend-to-native path.
- Produces: evidence that frontend, desktop, and headless Web builds remain healthy and that macOS receives the edited PNG clipboard image.

- [ ] **Step 1: Run all automated verification**

```bash
rtk npm test
rtk cargo test --manifest-path src-tauri/Cargo.toml --lib
rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib
rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features web --bin lanchat-web
rtk git diff --check
```

Expected: every command exits successfully.

- [ ] **Step 2: Launch an isolated desktop development run**

```bash
rtk cargo tauri dev -- --port 18888 --db-path /tmp/lanchat-agent-capture-copy
```

Keep the process running while performing the smoke test, and terminate it cleanly afterward.

- [ ] **Step 3: Exercise the user-visible flow**

1. Open a conversation and start a screenshot.
2. Select an area and add a visible annotation.
3. Confirm Copy is between Pin and Save and reports “复制” in Chinese UI.
4. Click Copy and confirm the capture editor disappears and the main chat window returns.
5. Paste into a native image-capable target and confirm the pasted bitmap contains the crop and annotation.
6. Reopen capture and confirm Pin, Save, and Done still behave as before.

- [ ] **Step 4: Confirm final repository state**

```bash
rtk git status --short --branch
rtk git log -4 --oneline --decorate
```

Expected: no uncommitted implementation files remain; the branch contains the design commit and three focused implementation commits.
