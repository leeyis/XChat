# XChat Capture Pin, Alerts, and Text Editing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use TDD and verification-before-completion when changing behavior.

**Goal:** Complete the 2026-07-30 capture/text/pin/desktop-alert design while preserving the existing first-text-input and Finder-drop fixes.

**Architecture:** Keep capture interaction state and history in the dependency-free React capture editor, with pure geometry/history helpers in `frontend/src/capture-drawing.js`. Keep pin-window lifecycle, file persistence, clipboard, sizing, and shadow control in the shared Rust capture core. Keep notification filtering and attention lifecycle in `frontend/src/xchat.js` and the existing Tauri commands.

**Tech Stack:** React 19, dependency-free browser JavaScript, Vite, Tauri 2, Rust, Tokio, SQLite-backed workspace.

## Global Constraints

- Text editing is enabled only while the text tool is active.
- Empty edited text cancels the edit; it does not delete an annotation.
- New, edited, and moved text operations each create one undo snapshot and support redo.
- Pinned windows remain single-instance, borderless, always-on-top, and draggable from the image area.
- Pin zoom is bounded to 20%–300%, changes in 1% rounded steps, and resizes the native window from original image dimensions.
- Notifications ignore self-authored and protocol-control messages, deduplicate up to 256 keys, and never block message/file processing.
- Do not touch unrelated user files (`AGENTS.md`, `plan/UI-DESIGN-PC.md`, `.happycode/`, `ui-ref/`) or hand-edit generated assets beyond the build output.

---

### Task 1: Correct text edit cancellation and regression coverage

**Files:**
- Modify: `frontend/src/capture-drawing.test.js`
- Modify: `frontend/src/CaptureEditor.jsx`
- Modify: `frontend/src/capture-drawing.js` only if the focused tests expose a geometry/history defect

**Interfaces:**
- `createTextOperation(input, color, size)` returns a stable text operation or `null`.
- `replaceCaptureOperation(history, id, operation)` records one atomic edit/move snapshot.
- `removeCaptureOperation` remains available for future explicit deletion, but is not called for blank edit cancellation.

- [x] **Step 1: Change the focused test first**

Update the existing blank-edit test to assert that the original operation remains unchanged when edited text is blank. Keep the undo/redo test for the explicit remove helper separate so it still verifies history mechanics.

- [x] **Step 2: Run the focused test and verify the expected failure**

Run: `rtk node --test frontend/src/capture-drawing.test.js`

Expected: the blank-edit behavior fails because the current editor calls `removeCaptureOperation` for an empty edited value.

- [x] **Step 3: Implement the minimal behavior change**

In `commitText`, when `current.original` exists and the trimmed value is empty, clear the input and return without changing history. Preserve the existing behavior for a new blank input and for non-empty edits/moves.

- [x] **Step 4: Run focused and full frontend tests**

Run: `rtk node --test frontend/src/capture-drawing.test.js` and `rtk npm test`.

Expected: both commands pass, including text hit priority, 4px drag threshold, boundary clamping, and atomic undo/redo.

- [x] **Step 5: Review the diff**

Run: `rtk git diff --check` and inspect only the touched capture files for accidental changes.

### Task 2: Restore pinned-window toolbar behavior

**Files:**
- Modify: `frontend/src/CaptureEditor.jsx`
- Modify: `frontend/src/capture-editor.css`
- Modify: `frontend/src/capture-drawing.test.js` if a pure toolbar-state/placement assertion is needed

**Interfaces:**
- `CapturePin` owns `toolbarVisible`, `zoom`, `menu`, `shadow`, and status state.
- The “显示工具条 / Show Toolbar” menu item toggles `toolbarVisible` and exposes `aria-checked`.
- Toolbar actions call the existing `capture.pin.resize`, `capture.pin.copy`, `capture.pin.save`, and `capture.pin.close` dispatch paths.

- [x] **Step 1: Add/adjust a regression assertion before implementation**

Keep a source-level or pure-helper assertion that the menu item is a checked menu item tied to `toolbarVisible`; this prevents the item from silently becoming an unrelated editor action.

- [x] **Step 2: Run the focused assertion and verify it fails against the current implementation**

Run: `rtk npm test`.

Expected: the new assertion fails because the current menu item invokes `onEdit` and the toolbar CSS/markup is absent.

- [x] **Step 3: Implement the toolbar toggle**

Restore the optional toolbar markup and CSS from the existing pin design, restore the `toolbarVisible` state, make the menu item `role="menuitemcheckbox"` with `aria-checked`, and keep the close button visible only when the toolbar is hidden. Keep the existing wheel zoom, menu bounds, native dragging, shadow control, close, destroy, and status feedback paths intact.

- [x] **Step 4: Run frontend tests and build**

Run: `rtk npm test && rtk npm run build`.

Expected: all tests pass and Vite emits the updated `src/assets` bundle.

### Task 3: Strengthen desktop-alert coverage

**Files:**
- Modify: `frontend/src/xchat.test.js`
- Modify: `frontend/src/xchat.js` only if the new file-summary test exposes a mismatch

**Interfaces:**
- `incomingMessageAlert(raw, selfId)` returns a deduplicable `{ key, fromId, title, body }` descriptor for remote text/file messages and `null` for self/control/empty events.

- [x] **Step 1: Add a failing file-alert test**

Assert that a remote file message produces a localized body containing the file name, while self-authored, control, duplicate-key, and empty messages remain filtered.

- [x] **Step 2: Run the focused test and verify the expected failure**

Run: `rtk node --test frontend/src/xchat.test.js`.

Expected: the new file-summary assertion fails if the descriptor does not use `file_name` correctly.

- [x] **Step 3: Make the smallest correction, if required**

Preserve the existing 256-key cache, attention clearing, peer-online notice, and capability behavior. Do not alter message persistence or transfer processing.

- [x] **Step 4: Run the full frontend suite**

Run: `rtk npm test`.

Expected: all frontend tests pass.

### Task 4: Cross-target verification and handoff

**Files:**
- Inspect: `src-tauri/src/capture_editor.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`, `src-tauri/permissions/commands.toml`, `src-tauri/capabilities/capture-pin.json`, `frontend/src/styles.css`
- Regenerate: `src/assets/*` through `rtk npm run build`

- [x] **Step 1: Verify Rust tests and both feature targets**

Run:

```bash
rtk cargo test --manifest-path src-tauri/Cargo.toml --lib
rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib
rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features web --bin lanchat-web
```

Expected: the changed capture/notification code compiles on desktop and web. If the existing sandbox-only file-transfer test fails with `Operation not permitted`, report that exact failure separately rather than changing unrelated transfer code.

- [x] **Step 2: Verify generated output and whitespace**

Run: `rtk npm run build` and `rtk git diff --check`.

- [x] **Step 3: Re-read the design acceptance list**

Confirm text create/edit/move/cancel/undo/redo, pin zoom/menu/toolbar/native controls, notification filtering/dedup/attention clearing, transparent settings selection, and Finder-drop compatibility are all represented in code or tests.
