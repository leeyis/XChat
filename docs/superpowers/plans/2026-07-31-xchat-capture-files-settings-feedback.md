# Xchat Capture, Files, Settings Feedback Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the confirmed capture, pin, file-input, transfer-bubble, and settings-navigation feedback in the existing React/Tauri application and ship a verified desktop bundle.

**Architecture:** Keep behavior in the existing frontend modules: pure drawing/style and geometry helpers in `capture-drawing.js`, UI state and Tauri pin editing in `CaptureEditor.jsx`, attachment normalization and transfer state in `xchat.js`, and settings metadata/rendering in `App.jsx`. Extend the Rust capture window only where pin edit mode needs a window-size transition; preserve existing serialized payloads and commands.

**Tech Stack:** React 18, dependency-free browser APIs, Vitest, Tauri 2, Rust/Tokio.

## Global Constraints

- Do not add a frontend framework or enumerate all installed system fonts.
- Preserve existing message, WebSocket/HTTP, and SQLite serialization formats.
- Ignore directories and unreadable clipboard entries without rolling back valid files from the same action.
- Keep unrelated dirty-worktree changes intact and avoid broad formatting.
- Verify frontend tests/build, Rust library tests, desktop/web feature checks, and the final DMG path.

---

### Task 1: Capture style data model and tool-specific controls

**Files:**
- Modify: `frontend/src/capture-drawing.js`
- Modify: `frontend/src/CaptureEditor.jsx`
- Modify: `frontend/src/capture-editor.css`
- Test: `frontend/src/capture-drawing.test.js`

**Interfaces:**
- `createTextOperation(input, color, size, style)` returns a text operation carrying `fontFamily`, `fontWeight`, and `fontStyle` with backward-compatible defaults.
- `drawCaptureOperation` and text hit measurement consume the same normalized text-style fields.

- [ ] Write failing tests for text defaults, style persistence/edit restoration, and tool-control visibility semantics.
- [ ] Run the focused Vitest test and confirm failure for missing style fields/normalization.
- [ ] Implement normalized text style helpers, canvas font construction, and tool-specific panel rendering: mosaic size only; text font family/size/color/bold/italic; other tools color/size.
- [ ] Apply the same style to the live input, existing-text editing, redraw, and PNG export.
- [x] Run all frontend tests and refactor only after green.

### Task 2: Stable pin edit layout and destroy-only semantics

**Files:**
- Modify: `frontend/src/CaptureEditor.jsx`
- Modify: `frontend/src/capture-editor.css`
- Modify: `src-tauri/src/capture_editor.rs`
- Test: `frontend/src/capture-drawing.test.js`

**Interfaces:**
- `placePinnedCaptureToolbarBelowImage(image, toolbar, viewport, gap)` always returns a position below the image and an upward window offset when needed.
- Pin context-menu “Show Toolbar” enters the existing pin editing view; exiting restores the normal pin window without destroying it.

- [ ] Add a failing pure geometry test proving toolbar placement never overlaps the image and remains in viewport bounds.
- [ ] Run the focused test and observe the expected failure.
- [ ] Add pin-edit state wiring, below-image transparent toolbar area, fixed 1:1 image canvas, and return path; remove close/hide controls and route the image close button/menu action to idempotent destroy.
- [ ] Add narrow Rust window resize/restore commands or state handling needed by edit mode, guarded to the desktop capture path.
- [ ] Remove geometry-changing capture transitions/transforms.
- [ ] Run frontend tests and Rust tests/checks.

### Task 3: Unified file intake and partial-failure behavior

**Files:**
- Modify: `frontend/src/xchat.js`
- Modify: `frontend/src/App.jsx`
- Test: `frontend/src/xchat.test.js`

**Interfaces:**
- `draft.addFiles` accepts `File`, `DataTransferItem`-derived files, managed/native paths, clipboard file URLs, and mixed valid/invalid entries while preserving valid input order.
- Directory and unreadable-entry notices are emitted once per action and do not discard valid attachments.

- [ ] Add failing tests for browser files, native paths, clipboard file variants, directory filtering, and mixed partial failure.
- [ ] Run the focused tests and confirm the missing non-image/partial-file behavior fails.
- [ ] Normalize all supported sources through one receiver, make Tauri path handling best-effort per item, and broaden paste/drop handling to documents and other files.
- [ ] Run the full frontend suite.

### Task 4: Message-bubble-only transfer progress and cancellation

**Files:**
- Modify: `frontend/src/App.jsx`
- Modify: `frontend/src/xchat.js`
- Modify: `frontend/src/styles.css`
- Test: `frontend/src/xchat.test.js`

**Interfaces:**
- `MessageFile` is the sole progress/cancel surface for a file message; cancellation is idempotent and resolves to cancelled or completed according to the refreshed backend state.

- [ ] Add failing tests for cancelling, cancelled, completed-race, and absence of aggregate transfer dock.
- [ ] Run tests to verify failure.
- [ ] Remove the chat-level transfer dock, keep status/bytes/percent/speed/progress/single cancel in the matching message bubble, and preserve optimistic cancelling until refresh.
- [ ] Run full frontend tests/build.

### Task 5: Settings metadata, icons, and About section

**Files:**
- Modify: `frontend/src/App.jsx`
- Modify: `frontend/src/styles.css`
- Test: `frontend/src/xchat.test.js`

**Interfaces:**
- `labels.settingsSections` is ordered metadata with bilingual title and icon for identity, appearance, notification, download, network, shortcut, and about.
- About content renders `Xchat`, a runtime/build version fallback, and the LAN chat/file-transfer description.

- [ ] Add failing tests for section order, icon mapping, About rendering, and scroll-sync participation.
- [ ] Run focused tests and verify the old label-only navigation fails them.
- [ ] Implement metadata-driven navigation, add/reuse icons, add About section at the bottom, and read runtime version with a built-in fallback without blocking render.
- [ ] Run full frontend tests/build.

### Task 6: Verification and release bundle

**Files:**
- No source changes unless verification exposes a scoped regression.

- [ ] Run `rtk npm test` and `rtk npm run build`.
- [ ] Run desktop and web Rust checks plus `rtk cargo test --manifest-path src-tauri/Cargo.toml --lib`.
- [ ] Remove stale generated `rw.*.dmg` files only from `src-tauri/target/release/bundle/macos` if present, then run `rtk cargo tauri build`.
- [ ] Inspect the resulting app/DMG paths and report any unavailable platform smoke test honestly.
