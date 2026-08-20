# Configurable File Transfer Channels Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Keep the work in the current checkout on `agent/parallel-transfer-channels`; do not create a worktree.

**Goal:** Add a persisted 4/8/16 “maximum parallel channels” setting that limits all new file transfers process-wide, preserves old-device behavior, and safely negotiates higher parallelism only between capable peers.

**Architecture:** Keep the existing v1 sequential and v2 fixed-four protocols unchanged. Introduce an explicit v3 parallel protocol with validated flexible manifests, negotiate it through discovery capabilities, and route every outbound chunk request through a shared generation-based semaphore. A settings change creates a new limiter generation so transfers already in progress keep their original limit while newly started transfers use the saved value.

**Tech Stack:** React/Vite frontend, Tauri 2, Rust, Tokio, Axum, SQLx/SQLite, existing XChat network protocol and test helpers.

**Spec:** `docs/plans/2026-08-18-xchat-network-presence-message-reliability-design.md` Stage E and the approved prototype in `ui-ref/xchat-desktop-prototype.html`.

## Global Constraints

- Preserve serialized v1/v2 behavior and the existing fixed-four v2 endpoints.
- Advertise and use v3 only when both peers explicitly support it.
- Accept only 4, 8, or 16 as saved settings; missing or malformed stored values read as 4.
- Apply a saved value only to transfers started afterward. Existing transfers retain their limiter generation.
- Enforce the selected limit globally across concurrent outbound transfers, not once per file.
- Keep permit waits cancellation-responsive and release permits on success, failure, timeout, or cancellation.
- Do not add a frontend dependency or a database migration.
- Use focused formatting only; do not run repository-wide `cargo fmt`.

---

## Task 1: Persist and model the transfer-channel setting

**Files:**

- Modify: `src-tauri/src/network/transfer.rs`
- Test: `src-tauri/src/network/transfer.rs`

### RED

- [x] Add focused unit tests proving:
  - a missing setting reads as `4`;
  - stored `4`, `8`, and `16` round-trip;
  - invalid requested values are rejected and do not overwrite the previous valid value;
  - malformed legacy/database text falls back to `4`.
- [x] Run the focused tests and observe the expected compile/test failure:

```bash
rtk cargo test --manifest-path src-tauri/Cargo.toml --lib max_parallel_channels
```

### GREEN

- [x] Add `DEFAULT_MAX_PARALLEL_CHANNELS`, the allowed-value validator, and async load/save helpers using the existing generic settings table.
- [x] Use one stable key such as `file_transfer.max_parallel_channels.v1`.
- [x] Keep fallback-on-read separate from reject-on-write so corrupt old data cannot break settings loading.
- [x] Re-run the focused tests.

### Commit

```bash
rtk git add src-tauri/src/network/transfer.rs
rtk git commit -m "feat(transfer): persist parallel channel limit"
```

---

## Task 2: Add the generation-based global concurrency controller

**Files:**

- Modify: `src-tauri/src/network/transfer.rs`
- Test: `src-tauri/src/network/transfer.rs`

### RED

- [x] Add async tests proving:
  - requests for the same configured limit share one semaphore generation;
  - changing the limit returns a distinct generation;
  - an old generation retains its original limit after a change;
  - no generation allows more simultaneous permits than its limit;
  - queued work can be cancelled without leaking a permit.
- [x] Run the focused tests and observe failure.

### GREEN

- [x] Implement `TransferConcurrencyController` with a short synchronous lock around the current generation and an `Arc<tokio::sync::Semaphore>` inside each cloneable generation.
- [x] Add a process-global production controller following the existing cancellation-registry pattern.
- [x] Expose only the narrow operations the uploader needs: select a generation for a validated limit and acquire an owned permit.
- [x] Re-run the focused tests.

### Commit

```bash
rtk git add src-tauri/src/network/transfer.rs
rtk git commit -m "feat(transfer): add global channel limiter"
```

---

## Task 3: Expose the setting through desktop and web APIs

**Files:**

- Modify: `src-tauri/src/workspace.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/web_server.rs`
- Test: `src-tauri/src/commands.rs`
- Test: `src-tauri/src/web_server.rs`

### RED

- [x] Add API-level tests proving settings snapshots return `4` by default, valid updates persist, and invalid updates return an error without mutation.
- [x] Cover both the shared/Tauri update path and the HTTP request path where practical.
- [x] Run focused Rust tests and observe failure.

### GREEN

- [x] Add `max_parallel_channels` to `WorkspaceSettings` and populate it through the shared snapshot path.
- [x] Add optional `max_parallel_channels`/`maxParallelChannels` update arguments to the existing HTTP and Tauri settings endpoints.
- [x] Delegate validation/persistence to Task 1 helpers; avoid duplicating accepted values.
- [x] Keep existing command names, registrations, and permissions unchanged.
- [x] Re-run focused tests.

### Commit

```bash
rtk git add src-tauri/src/workspace.rs src-tauri/src/commands.rs src-tauri/src/web_server.rs
rtk git commit -m "feat(settings): expose parallel channel limit"
```

---

## Task 4: Define v3 negotiation and safe flexible manifests

**Files:**

- Modify: `src-tauri/src/network/discovery.rs`
- Modify: `src-tauri/src/network/conversation_file.rs`
- Modify: `src-tauri/src/web_server.rs`
- Test: `src-tauri/src/network/conversation_file.rs`
- Test: `src-tauri/src/web_server.rs`

### RED

- [ ] Add pure tests proving:
  - peers without v2 remain sequential;
  - v2-only peers remain fixed at four ranges regardless of the local setting;
  - v3 peers select the minimum of local setting and advertised peer maximum;
  - v3 range generation fully and contiguously covers empty, tiny, uneven, and large files;
  - v3 manifest validation rejects gaps, overlaps, duplicate/non-contiguous indices, zero-length non-empty chunks, overflow, and excessive part counts;
  - v2 validation still accepts only the historical fixed-four layout.
- [ ] Add route/handler tests proving v2 and v3 manifests are versioned and recovered independently.
- [ ] Run focused tests and observe failure.

### GREEN

- [ ] Add explicit discovery capabilities for v3 and the peer-supported maximum, with a bounded parser that defaults safely for missing/malformed capability data.
- [ ] Replace the job's `parallel_v2` boolean with an explicit upload protocol/plan carrying the fixed or negotiated channel count.
- [ ] Add flexible v3 range generation with bounded part count and enough small work units to allow fair scheduling; retain `parallel_chunk_ranges` unchanged for v2.
- [ ] Add `/api/uploads/v3/prepare` and `/api/uploads/v3/:transfer_id/:chunk_index` while preserving v2 routes.
- [ ] Share handler internals but validate the path protocol against manifest version and layout before accepting bytes.
- [ ] Ensure initial sends, offline resumes, and retries all recompute/use the same explicit negotiation rules.
- [ ] Re-run focused tests.

### Commit

```bash
rtk git add src-tauri/src/network/discovery.rs src-tauri/src/network/conversation_file.rs src-tauri/src/web_server.rs
rtk git commit -m "feat(protocol): negotiate flexible parallel transfers"
```

---

## Task 5: Route all outbound chunk requests through the global scheduler

**Files:**

- Modify: `src-tauri/src/network/conversation_file.rs`
- Modify: `src-tauri/src/network/transfer.rs`
- Test: `src-tauri/src/network/conversation_file.rs`

### RED

- [ ] Extend the fake HTTP receiver to record active request count, peak count, transfer IDs, and request ordering.
- [ ] Add integration-style sender tests proving:
  - concurrent v3 transfers never exceed a global limit of 4/8/16;
  - at least two transfers make progress before one monopolizes all queued work;
  - v1 and v2 requests consume the same global permit pool;
  - cancelling while queued or in flight exits promptly and leaves later transfers unblocked;
  - changing the setting affects a newly started job but not an already-running job.
- [ ] Run focused tests and observe failure.

### GREEN

- [ ] Capture the saved channel value and its limiter generation when each upload job starts.
- [ ] Acquire one shared permit immediately before every outbound chunk/range HTTP request, and drop it immediately after that request completes.
- [ ] Bound per-file in-flight futures to its negotiated channel count; do not construct an unbounded set of active request futures.
- [ ] Poll cancellation while waiting for a permit and while retry/backoff logic runs.
- [ ] Preserve progress, SHA-256 verification, resume, retry, and receiver-finalization semantics.
- [ ] Re-run focused tests.

### Commit

```bash
rtk git add src-tauri/src/network/conversation_file.rs src-tauri/src/network/transfer.rs
rtk git commit -m "feat(transfer): enforce global fair concurrency"
```

---

## Task 6: Add the approved UI and adapter plumbing

**Files:**

- Modify: `frontend/src/xchat.js`
- Modify: `frontend/src/App.jsx`
- Modify: `frontend/src/xchat.test.js`
- Modify only if generated production assets are repository-owned: `src/`

### RED

- [ ] Add frontend tests proving:
  - absent/invalid settings normalize to `4`;
  - `4`, `8`, and `16` survive normalization;
  - both Tauri and HTTP adapters send the same `maxParallelChannels` update;
  - the setting participates in dirty-state/save/reset behavior.
- [ ] Run the focused frontend suite and observe failure:

```bash
rtk npm test -- --runInBand
```

### GREEN

- [ ] Render the approved control directly below “自动接收文件” in the download-and-transfer section.
- [ ] Use the exact Chinese copy from the prototype and equivalent English copy:
  - label: `最大并行通道`
  - hint: `兼顾兼容性与资源占用。保存后对新开始的传输生效；旧版设备会自动使用 4 个通道。`
  - choices: `4（默认）`, `8`, `16`
- [ ] Add the field to normalized settings, dirty keys, save/reset logic, and both adapters.
- [ ] Build the frontend so the Tauri production asset directory reflects the source if that is the repository's established workflow.
- [ ] Re-run frontend tests and build.

### Commit

```bash
rtk git add frontend/src/xchat.js frontend/src/App.jsx frontend/src/xchat.test.js src
rtk git commit -m "feat(ui): configure transfer channel limit"
```

---

## Task 7: Full verification, visual QA, review, and integration

### Automated verification

- [ ] Run all frontend tests and build:

```bash
rtk npm test
rtk npm run build
```

- [ ] Run all Rust library tests:

```bash
rtk cargo test --manifest-path src-tauri/Cargo.toml --lib
```

- [ ] Check both shared feature sets:

```bash
rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib
rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features web --bin lanchat-web
```

### Runtime verification

- [ ] Launch an isolated Tauri development instance on an alternate port/database and smoke-test load, change, save, reload, and reset of all three options.
- [ ] Visually compare the production control with `ui-ref/xchat-desktop-prototype.html` at desktop widths and capture evidence.
- [ ] Run isolated sender/receiver instances and verify:
  - new↔new negotiates v3 at 4, 8, and 16;
  - new↔v2-only remains fixed at 4;
  - transfer content hashes match after normal, resumed, and cancelled transfers;
  - simultaneous transfers obey the selected process-wide limit.

### Review and integration

- [ ] Inspect `rtk git diff main...HEAD`, confirm no unrelated user changes, and run the completion-verification skill.
- [ ] Perform a focused code review of protocol compatibility, validation boundaries, cancellation, permit lifetime, and settings parity; fix and re-run affected checks.
- [ ] Merge the finished branch into local `main` with a non-interactive merge only after every required check passes.
- [ ] Report the merge commit, verification commands, any unavailable platform target, and the real-device requirement: both devices need the new build to exercise configurable v3; mixed-version pairs intentionally use four channels.
