# Remote connection feedback follow-up

## Acceptance target

- Sharer has no recursive local screen preview; keep a compact floating control bar.
- Controller uses an independent remote window with fullscreen and host/connection information; conversation stays in the main window.
- Voice calls connect in both directions between macOS and Windows, with correct microphone permissions and session cleanup.
- Performance baseline supplied by user: at least the illustrated 32 FPS, approximately 55 ms network / 60 ms frame latency, hardware encoding/decoding, H.265, DXGI and encrypted UDP P2P. The illustrated 405.5 Kbps is a sample workload, not evidence of this application's performance.
- Report actual measurements and remaining gaps. Do not claim sample metrics, polling frequency or reduced resolution as achieved performance.

## Latest RDP verification and release scope

The user confirmed an active RDP connection. Both the test process and the active RDP desktop belong to Windows session 1; console session 5 is separate. The RDP desktop is 1920 × 1080. A physical display being off is therefore not a reason to block testing this interactive RDP desktop. No RDP policy, registry value, desktop lock or connection was changed.

The final monitored native Windows → local Chromium run passed **all eight application flows**, with normal WebView background settings and no compiler or Codebase MCP process present. Native DXGI → NVIDIA HEVC Encoder MFT → authenticated WebSocket → DedicatedWorker → RTCDataChannel → WebCodecs remained active through input-channel replacement and pause/resume.

| Measurement | Result |
| --- | --- |
| Actual displayed-frame callbacks, 12 seconds, 1920 × 1080 | 31.39 FPS |
| Minimized host, sender debugger disconnected, 12 seconds | 31.40 FPS |
| Source timestamp → decoded image age, 25 valid samples | mean 164.68 ms; p95 203 ms |
| Hardware encoder | NVIDIA HEVC Encoder MFT, verified native hardware MFT |
| Hardware decoder | Unknown; preference is not proof |
| Connection | Same-machine UDP / DTLS-SCTP; approximately 1 ms RTT |
| Total system CPU during foreground/background measurements | 35.75% / 30.63% mean |
| Compiler or indexer during the run | None |

The workload is a static desktop with one moving 300 × 240 region and a 48-bit timestamp barcode; its own redraw log is retained with the local evidence. These are same-machine RDP results, not two-device LAN or physical scanout measurements. The user's 32 FPS / about 60 ms baseline remains **not certified**, and macOS physical audio and hardware decode still need manual testing.

The first RDP trial exposed a control-recovery bug: during input-channel replacement, failed delivery of an ordinary HEVC control message permanently failed the healthy video Worker. The fix queues and coalesces at most eight control messages / 16 KiB, retaining the highest frame ACK, and flushes when the replacement channel opens or its buffer drains. Session revision, pause and close discard stale messages. Configuration ACK and first-frame timeouts still apply. The subsequent real test verified advancing HEVC frames after recovery and that control permission stays suspended. The harness also now compares frame increments within the same presentation type; its previous cross-codec absolute counter comparison could misreport a fallback as frozen video.

Local raw reports: `2026-10-10-remote-rdp-session.json` (initial failure) and `2026-10-10-remote-rdp-recovery.json` (eight checks passed). This published record contains aggregate evidence rather than desktop screenshots or temporary session URLs.

The user authorized release **0.1.14**, commit/push followed by Windows installer builds, and explicitly included the existing autostart and group-presence changes. See [release validation](../plans/2026-10-10-v0.1.14-release-validation.md).

## Tasks

- [x] Inspect repository instructions, existing working tree and code graph.
- [x] Confirm Context7 is already connected and resolve official RustDesk libraries.
- [x] Research RustDesk capture, encoding, QoS and transport implementation.
- [x] Prepare and verify updated UI prototype in `ui-ref/xchat-desktop-prototype.html`.
- [x] Obtain explicit prototype approval before changing production UI (AGENTS.md review gate). User approved remote-v3 with “已审阅，批准按此实现”.
- [x] Fix microphone and bidirectional voice call lifecycle (Windows synthetic WebRTC verified; macOS runtime remains unavailable).
- [x] Fix erroneous remote input channel disconnect behavior.
- [x] Implement and measure capture / frame delivery improvements; cross-device minimum remains unverified.
- [x] Implement approved independent remote window and compact sharer UI.
- [x] Run focused frontend tests, desktop/web compile checks and isolated desktop smoke test.
- [x] Record evidence, platform limitations and remaining acceptance gaps.

## Working tree baseline

Pre-existing modifications include autostart, App.jsx, styles.css, xchat.js, command registrations, Cargo files, generated frontend assets and the UI prototype. They were preserved during remote work; the user subsequently explicitly approved including autostart and group presence in the 0.1.14 release. Current source is React/Vite despite historical AGENTS.md describing an older static frontend.

## Environment

Windows host; macOS native runtime is not available locally. All shell commands use `rtk`. The knowledge graph was queried and re-indexed but has no current remote symbols, so direct file reads and `rg` fallback are necessary for those modules. Following the user's CPU feedback, Codebase MCP is kept stopped and no graph tool is called during this work.

### CPU interference and benchmark validity

Several old MCP server processes still had automatic watching enabled in memory. Stopping only their indexing children did not stop the workload: their parents started replacement workers. After changing `auto_watch` to `false`, the old servers and workers were stopped together. Process snapshots subsequently found no Codebase MCP or `rustc` processes. Automatic watching remains disabled; it must not be restored at the end of this task.

Earlier results without continuous process monitoring are diagnostic observations, not controlled performance acceptance evidence. They may include background indexing or other desktop contention. The integration harness now requires five baseline samples without an indexer/compiler and records CPU/process samples throughout the test. The default idle threshold is 20%; a deliberately relaxed 50% diagnostic threshold is explicitly labelled `diagnostic_desktop_load`. The monitored host still had ordinary application load around 24–28%, even without the indexer/compiler; those other applications were left running. Builds and FPS measurements are performed separately, and the QA launcher limits Cargo to two jobs.

The first Worker trial negotiated native NVIDIA HEVC but failed after 15 sent frames with `remote_hevc_encode: timed out awaiting hardware output`; it fell back to RTP. Two default-idle baseline attempts were rejected before measurement. A later diagnostic stopped when the native desktop became unavailable, and subsequent trials established ICE but timed out waiting for a first displayed frame. None of these runs establishes the requested FPS or latency.

The expanded native Worker diagnostics show repeated DXGI unchanged responses with no initial surface, zero encoded frames and zero encode time. The bounded first-frame watchdog reports the failure and releases capture ownership to the compatible RTP path. The RGBA path had a separate liveness defect: DXGI timeout returned unchanged indefinitely, so it never reached GDI fallback. The new four-timeout startup budget now reaches real GDI and delivered an initial 1920 × 1080 image in the application. GDI retries DXGI every five seconds while continuing its own captures. Existing static DXGI sessions are not downgraded after a first successful surface. A scoped execution-state guard is released on capture error, worker exit and the two-second idle timeout. Five focused native strategy tests passed.

The latest diagnostic still failed sustained delivery: 109 completed RGBA reads produced four changed frames and 105 unchanged frames, with no read errors. Source, encoded, sent and decoded counts all stopped at four. The visible animation fixture reported mapped 2560 × 1440 bounds and continuing redraws. The failure is therefore upstream of transport/receiver decoding; it is not a Worker reclaiming capture after failure. A regression exercising the real capture loop with a failed Worker proxy confirms that later changed RGBA frames still draw and request RTP frames. The fixed `ownsCapture` property identifies the implementation type; `hevcWorker.captureOwned` is the current ownership state.

Read-only Windows power notifications subsequently reported both session and console display state as **off**. During a separate native receiving-video test they reported **on**, then returned to **off** after that test ended. A two-second scoped `SetThreadExecutionState` probe succeeded but received no display-on notification. These observations are recorded in `2026-10-10-display-power-state.json`; they support checking the physical display state but do not prove the exact cause of the earlier trials. Microsoft documents the immediate current-state callback and the 0/off, 1/on, 2/dimmed values: [registration API](https://learn.microsoft.com/en-us/windows/win32/api/powersetting/nf-powersetting-powersettingregisternotification), [display-state GUIDs](https://learn.microsoft.com/en-us/windows/win32/power/power-setting-guids). The user has been asked to turn on and unlock the test display before further DXGI/FPS acceptance work. No input was injected and no persistent power policy was changed.

## Verification progress

- UI prototype: 23 checks passed before approval. User explicitly approved remote-v3; production UI followed that approval.
- Production React UI: 23 checks passed, no uncaught exceptions. Covers separated chat/viewer, no sharer preview, toolbar bounds, fullscreen, audio unlock in the media-owning window and late conversation mounting. Synthetic session fixtures establish UI behavior only.
- Latest frontend verification: `rtk npm test` passed **293 tests** (0 failed) and `rtk npm run build` passed for the final production JavaScript, including the bundled DedicatedWorker, receiver backpressure/ACK fixes, startup deadlines, capture ownership handoff and late-metrics capability race. The additional test exercises continued RGBA capture after Worker failure; it does not alter the production bundle. The default FPS remains 30, with explicit 10/20/30/60 supported; performance trials request 60 FPS so the 30 FPS default does not cap acceptance measurements.
- Voice: 8 real Chromium WebRTC loopback scenarios passed using synthetic microphones; macOS and physical audio are unverified. See the separate voice report.
- Before the shared-buffer refinement, desktop and web compile checks passed, and the isolated native app built and started. `rtk cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib remote:: -- --nocapture`: 15 passed, 1 interactive capture benchmark ignored. This includes GPU/header changes, viewer authorization, atomic session-scoped polling and monitor/DPI placement. Shared-buffer integration requires a new native build and checks.
- Isolated `cargo tauri dev` initially failed due to CLI argument separator, then a concurrent in-progress command edit, then exhausted `K:` build storage (`rustc-LLVM ERROR: no space on device`). The launcher uses `-- --` to forward app arguments with the installed CLI. Three XChat incremental cache directories were moved from `K:/cargo/debug/incremental` into `F:/XChat-remote-followup-build-cache-20261010`; no source files or dependency cache were removed. Roughly 7 GB free was restored for the final build.
- QA uses a separate application identifier, disposable database, discovery disabled and port 18888. Browser/CDP profiles and peers are local test instances; the installed Xchat process is not used or terminated. A fixed CDP port collided with a UI fixture, so the launcher now selects a free port.

## Real integration findings

The first animated 1920 × 1080 native Windows → local Chromium trial delivered only **3.73 decoded FPS** over 12 seconds despite the faster capture-only benchmark. The sender measured 12.99 ms native capture, 187.89 ms local bridge overhead, and 3.1 ms canvas presentation. Codec was H.264; hardware acceleration was not reported by the engine. The selected connection was UDP with SRTP, with approximately 1 ms RTT and 354 Kbps for this synthetic striped scene. This is same-machine communication, not a physical LAN acceptance result.

A 48-bit timestamp barcode in that scene gave 25 valid source-draw → decoded-image age samples: mean 449.24 ms, p95 759 ms. This includes capture/encoding/network/decode and fixture scheduling; it does not measure physical scanout or input-to-photon latency. The poor result motivated replacing the 8 MB-per-frame ordinary IPC response with WebView2's read-only shared-buffer API. No performance improvement is claimed until the complete new path is measured.

The same native/browser trial verified late voice invitation and answering through the main conversation, input-channel replacement without terminating video/voice or automatically restoring control, and pause/resume. A test-harness reverse invitation initially omitted a required `note`; the harness is corrected. A subsequent reverse native-viewer test exposed a real codec preference regression: merging sender/receiver capabilities with distinct HEVC levels caused a rejected video SDP and RTCP mux failure. The codec helper is being reduced to a real-browser-verified capability list before rerunning the flow.

The codec regression above was corrected and verified with real HEVC negotiation. The subsequent native hardware HEVC test passed all eight application flows, including the reverse independent native viewer, fullscreen and closing. It delivered **13.03 displayed FPS** at 1080p and **1.33 FPS while the host was minimized**; mean timestamp age was **303.08 ms**, p95 **510 ms**. The encoder was actually NVIDIA HEVC Encoder MFT and capture was DXGI. These results still failed the requested performance standard. Evidence: `2026-10-10-remote-native-hevc-initial.json`.

A diagnostic run with direct canvas display and browser background flags also failed to establish acceptable delivery. Those flags are not in production. Its report is retained as a rejected experiment, not an acceptance result.

The current implementation moves the per-frame path out of the main document: native authorized WebSocket → DedicatedWorker → transferred RTCDataChannel → WebCodecs → visible canvas. Native hardware encoding now waits on MF completion callbacks instead of polling. Token scope, bounded requests, revocation, stale revisions and fallback are tested. Real Edge transfer and synthetic Worker loopback confirm that a blocked Window task does not stop frame forwarding; they do not certify native performance. See `2026-10-10-remote-worker-capture.md` and `2026-10-10-remote-native-worker-stream.md`.

The final desktop and web compile checks passed with the native initial-frame fallback change (`-j 2`). The web check retains three dead-code warnings for native-only capture capability helpers. The isolated native app rebuilt and started in 8m 23s; Cargo also reports the existing library/binary PDB filename collision. The earlier web executable build and production frontend build passed. `git diff --check` passed.

The fresh native viewer-only smoke passed all three checks: an independent WebView2 owns reception, native fullscreen enters/exits, and closing that window ends its remote session while keeping the main window alive. Evidence: `2026-10-10-remote-native-viewer-final.json`. Native first-frame recovery and read-count diagnostics are in `2026-10-10-remote-native-worker-first-frame-recovery.json` and `2026-10-10-remote-native-worker-capture-diagnostics.json`. The latter recorded no compiler/indexer during measurement, but 56.77% mean total CPU under ordinary desktop plus test load and zero newly delivered frames during the 12-second interval. It failed; it is not a performance pass.

The original full-height Tk animation itself reached only about 28 source redraws/sec, so the fixture now uses one moving 300 × 240 region plus a timestamp barcode, flushes idle drawing and skips missed deadlines. Future measurements must retain that workload definition and the actual source callback log; the revised lighter scene has not yet established an accepted native performance result.

The later RDP test passed sustained native Worker delivery and minimized-window functional verification under normal background settings, with the measured performance limits above. macOS hardware decode, physical audio and a genuine two-device LAN remain unavailable in this Windows environment. Capture-only timings must not be reported as delivered FPS or end-to-end latency.

## Latest cleanup and continuation

The owned isolated QA launchers (initial PID 61184 and later RDP PID 59616, verified by their exact disposable configuration path) and their process trees were stopped after verification. Process snapshots found no Codebase MCP or rustc during measurement; the installed XChat process remained running. Automatic graph watching stays disabled. The active-RDP test is complete; cross-device/macOS acceptance remains outstanding. The RDP clarification and successful capture supersede the earlier request to turn on the physical display. The final 0.1.14 frontend test run passed **299 tests**, with no failures, and the production frontend build passed.
