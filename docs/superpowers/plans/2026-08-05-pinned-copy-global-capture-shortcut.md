# Pinned Copy and Global Capture Shortcut Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Copy to the pinned-image editing toolbar and make the configured desktop capture shortcut work while Xchat is unfocused or hidden in the tray.

**Architecture:** Keep pinned editing UI decisions in the pure capture action helper and React editor, while reusing the existing pinned-capture persistence and clipboard commands. Register the desktop shortcut in Rust with Tauri's global-shortcut plugin, update registration transactionally when the setting changes, and retain the DOM key listener only for the Web runtime.

**Tech Stack:** React 19, dependency-free Node tests, Rust, Tauri 2, SQLite, `tauri-plugin-global-shortcut`.

## Global Constraints

- Desktop shortcut registration must work with the main window focused, unfocused, minimized, or hidden to the tray.
- Updating to an unavailable shortcut must keep the previously registered shortcut and must not persist the unavailable value.
- Web capture keeps its focused-page shortcut behavior.
- Pinned editing Copy updates the pinned image, copies the edited original-size pixels, exits editing, and leaves the pin alive.
- Run every shell command through `rtk` and avoid repository-wide formatting.

---

### Task 1: Pinned editing Copy

**Files:**
- Modify: `frontend/src/capture-drawing.test.js`
- Modify: `frontend/src/capture-drawing.js`
- Modify: `frontend/src/CaptureEditor.jsx`

**Interfaces:**
- Consumes: `captureEditorActionAvailability({ conversationId, nativeCopy, pinEditing })` and workspace actions `capture.pin`, `capture.pin.copy`.
- Produces: `canCopy: true` for native pinned editing and a Copy handler that updates, copies, then exits editing.

- [ ] Write a failing assertion expecting native pinned editing to expose Copy.
- [ ] Run `rtk node --test --test-name-pattern="capture editor actions distinguish" frontend/src/capture-drawing.test.js` and confirm the pin-edit assertion fails.
- [ ] Change the availability helper and pin-edit Copy handler with the smallest behavior-preserving edit.
- [ ] Re-run the focused test and `rtk npm test`.

### Task 2: Transactional desktop global shortcut registration

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/main.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `frontend/src/App.jsx`

**Interfaces:**
- Consumes: the persisted `capture_shortcut`, `capture_editor::start(app, None)`, and Tauri global-shortcut manager.
- Produces: `register_capture_shortcut`, transactional replacement logic, startup registration, and immediate re-registration from `update_workspace_preference`.

- [ ] Add failing Rust tests for shortcut normalization/replacement rollback using an injected registration boundary.
- [ ] Run the focused Rust tests and confirm they fail because the registration helper is absent.
- [ ] Add the global-shortcut dependency/plugin, implement registration state and replacement, then wire startup and preference updates.
- [ ] Restrict the React `keydown` capture listener to the Web runtime and update focused-only shortcut copy.
- [ ] Run Rust library tests, desktop compile check, and Web compile check.

### Task 3: Build and smoke verification

**Files:**
- Regenerate: `src/index.html`
- Regenerate: `src/assets/`

**Interfaces:**
- Consumes: completed frontend and Rust behavior.
- Produces: packaged frontend assets and verification evidence.

- [ ] Run `rtk npm run build`.
- [ ] Run `rtk npm test` and `rtk cargo test --manifest-path src-tauri/Cargo.toml --lib`.
- [ ] Launch `rtk cargo tauri dev -- --port 18888 --db-path /tmp/lanchat-agent-global-shortcut` and smoke test foreground, background, and tray shortcut activation plus pinned-toolbar Copy.
- [ ] Inspect `rtk git diff --check` and the final scoped diff.
