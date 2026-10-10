# Remote capture and transport verification — 2026-10-10

## Findings

The Windows sender previously performed the following for every requested frame:

1. Enumerate monitors and capture a GDI screenshot with `xcap::Monitor::capture_image`.
2. Resize on the CPU, convert RGBA to RGB, encode a complete JPEG.
3. Transfer the JPEG through Tauri IPC, decode it in JavaScript, and draw it to a canvas.
4. Ask WebRTC to encode that decoded picture again for the peer.

The `30 fps` quality setting was only a ceiling. It could not overcome this serial capture/encode/decode path. WebRTC could also lower resolution under load; the reported 480 × 270 was not the original desktop resolution. The old stats reader depended on `framesPerSecond`, which is not supplied by all WebKit versions.

The native capture loop also depended on a JavaScript timer. Background or occluded WebViews may clamp these timers, independently of LAN speed. Native capture now waits for its next deadline on the capture thread and chains IPC completion to the next request without a JavaScript timer. Paused sessions do not capture.

## Source research

Context7 was already connected. `/rustdesk/rustdesk` and `/rustdesk-org/hwcodec` were available; no new MCP installation was needed. Context7 documents the persistent capturer and `WouldBlock` contract; the implementation details below were checked against the official source.

- [RustDesk video service](https://github.com/rustdesk/rustdesk/blob/master/src/server/video_service.rs): keeps expensive capture/encoder resources alive, combines capture with video QoS and frame-consumption feedback, recreates failed DXGI capture, and can use a texture encoder path. This motivated a persistent worker, demand-driven backpressure and explicit capture-recovery handling.
- [RustDesk DXGI capture](https://github.com/rustdesk/rustdesk/blob/master/libs/scrap/src/dxgi/mod.rs): provides desktop duplication and GPU/CPU capture paths. [Microsoft Desktop Duplication documentation](https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/desktop-dup-api) defines BGRA surfaces, row pitch, rotation and frame ownership. The new implementation owns all D3D resources on one worker and releases each acquired/mapped frame.
- [RustDesk codec selection](https://github.com/rustdesk/rustdesk/blob/master/libs/scrap/src/common/codec.rs): codec choice depends on actual endpoint support. [Chrome 136 release information](https://developer.chrome.com/blog/chrome-136-beta) documents WebRTC HEVC support; engine/device capability still must be queried. XChat now prioritizes locally supported H.265 and H.264 while retaining common fallback codecs and reporting the actual negotiated RTP codec.
- [WebRTC statistics specification](https://w3c.github.io/webrtc-stats/): FPS is derived from encoded/decoded frame counters when the browser omits the instantaneous field; interval encode/decode time, receive jitter buffer delay, packet loss, negotiated codec and selected ICE transport are distinct measurements. None is presented as measured end-to-end screen latency.
- [Microsoft video processor input requirements](https://learn.microsoft.com/en-us/windows/win32/api/d3d11/nf-d3d11-id3d11videodevice-createvideoprocessorinputview) and [VideoProcessorBlt](https://learn.microsoft.com/en-us/windows/win32/api/d3d11/nf-d3d11-id3d11videocontext-videoprocessorblt): reusable GPU textures resize and convert BGRA to opaque RGBA before readback. A render-target binding is required for the input view used here; a shader-resource-only binding was rejected by the tested NVIDIA driver and was corrected before final measurement.

## Current native HEVC path

On supported Windows hardware, the production path now uses persistent DXGI capture, D3D11 NV12 conversion, a hardware-only Media Foundation HEVC encoder, an authenticated loopback WebSocket consumed by a DedicatedWorker, and an unreliable bounded WebRTC data channel transferred to that Worker. The receiver decodes with WebCodecs and draws directly to its visible canvas. GPU NV12 is still read back into the native encoder input; this is not a zero-copy GPU pipeline. Actual hardware decoder selection remains unknown where the engine does not expose proof.

The socket uses the existing native HTTP listener and accepts only loopback clients with an exact native-issued one-use capability. Its token binds the actor, session, revision, Origin, listener port and server lifetime. Main-window destruction, missing consent toolbar, pause, revised authorization and session termination invalidate capture. The Worker keeps at most one native frame request pending; it cannot authorize a session. The user-visible connection transport is still the actual selected encrypted WebRTC connection.

MF output now arrives through a bounded completion callback rather than a 1 ms polling loop. The frame-rate setting accepts 60 FPS as well as existing values. These changes require full application measurements; a faster encoder or a setting alone cannot prove delivery rate. Protocol details and synthetic Worker verification are in `2026-10-10-remote-worker-capture.md`, `2026-10-10-remote-native-worker-stream.md` and `2026-10-10-remote-hevc.md`.

## RGBA fallback path and initial work

`remote_frame(format: "rgba-v1")` uses a bounded, demand-driven capture worker. It reuses the selected output's DXGI Desktop Duplication and D3D staging texture instead of constructing capture resources for every frame. Capture stops when no request is pending; idle GPU resources are released after two seconds. Session/revision changes create a new capture generation, so resume and a new session receive a fresh full frame.

The D3D11 video processor resizes and converts the captured BGRA surface to RGBA on the GPU. Only the target resolution is read back. The CPU copies complete rows; it no longer performs the full-resolution color conversion or resize on the supported GPU path. Unsupported video processors fall back to CPU conversion. DXGI pointer-only events skip readback; GDI retains a pixel comparison because it supplies no change metadata.

An explicit binary header carries output/source size, sequence, capture duration, complete native request duration (including scheduling/waiting), and capture backend. The 44-byte header reuses the pixel allocation, avoiding a second whole-frame copy. The frontend accepts both 40-byte and 44-byte headers. RGBA pixels go directly into the WebRTC canvas; there is no JPEG encode/decode in this path. Unchanged frames have no pixel payload and do not invoke `requestFrame`. The original JPEG response remains available for callers that omit the optional format. The raw payload crosses only the local native/WebView bridge; LAN traffic remains negotiated WebRTC video.

If DXGI is unavailable or the display is rotated, the implementation uses the GDI fallback and identifies it as GDI. It periodically retries DXGI. Desktop locking, monitor changes, owner/session validation and post-capture revision validation remain enforced.

The frame-rate default is 30, with `maintain-resolution` requested from the encoder. HEVC preference does not imply hardware use: actual codec, encoder/decoder implementation and power-efficiency indicators are reported only if the browser provides them. The RTP statistics include actual selected UDP/TCP and direct/relay state rather than treating every connection as UDP P2P.

Bounded host telemetry shares capture diagnostics and actual OS/version/architecture/client/CPU/RAM metadata with the viewer. Its whitelist does not allow the sender to replace measured receiver FPS, packet loss or RTT. `captureMs` includes DXGI waiting and capture processing. `bridgeMs` is the frontend RPC round trip minus the native request duration, so it includes IPC, dispatch and scheduling overhead outside the worker. `canvasMs` measures local pixel presentation. These are not glass-to-glass screen latency measurements.

## Verification

- `rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib` — passed before the final GPU/copy refinements; final application verification is coordinated by the integration agent.
- `rtk proxy node --test frontend/src/remote-performance.test.js frontend/src/remote-model.test.js` — 12 passed after the shared-memory bridge: raw-frame validation, codec preferences, shared-buffer request identity/lifetime/error paths, real RTP stats, bounded telemetry, quality defaults and input mappings.
- Focused native harness compiled the actual unchanged production capture/model/input modules with `rustc --test`, `opt-level=1`, `debuginfo=2`, debug assertions on, and the same prebuilt dependencies as the desktop development profile. Six tests passed, including stride/BGRA/SIMD-tail/scaling checks and verification that packet headers preserve the pixel allocation. The interactive benchmark is explicitly ignored during ordinary test runs.
- A full application test invocation was initially blocked by an unrelated viewer deserialization compile error; the owning agent repaired that error. A later desktop link exhausted the build drive; the integration agent moved rebuildable incremental caches and will report final desktop/web builds separately. Neither failed attempt is counted as a passing check.

The manual benchmark is opt-in because it captures the interactive Windows desktop. It stores only timing/size results; it does not save desktop pixels. Both paths use the same selected display and `clear` quality. Run with changing desktop content (set `XCHAT_CAPTURE_BENCH_LEGACY=0` to skip the expensive legacy reference after recording it):

```text
rtk cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib remote::capture::windows_capture::benchmark::compare_capture_paths -- --ignored --nocapture
```

The native benchmark is separate from transport and decoder verification. On the test host's 2560 × 1440 primary screen, output stayed at 1920 × 1080 throughout. The fixture in `analysis/remote-perf-20261010/capture-animation.py` schedules a visibly moving 1080p striped scene at 60 Hz and prints its observed callback rate. The fixture saves no screen content and closes automatically. `analysis/remote-perf-20261010/build-harness.py` was used to avoid repeated whole-application linking during native profiling; it reuses this machine's existing Cargo dependencies and is a local diagnostic helper, not the portable test command above.

Initial profiling exposed two distinct problems. CPU scaling after DXGI still averaged about 300–380 ms. GPU scaling alone reduced work but left a BGRA conversion averaging 80 ms in one run; GPU RGBA conversion and row copying removed that operation. These intermediate results were rejected, not treated as completion.

Final same-scene comparison before full-application QA, with both paths run consecutively while the visible motion fixture remained active:

| Capture path, fixed 1920 × 1080 output | Changed samples | Mean call | p95 call | Actual capture rate |
| --- | ---: | ---: | ---: | ---: |
| Previous GDI / CPU resize / JPEG | 10 | 771.13 ms | 880.42 ms | Not separately sampled |
| Persistent DXGI / GPU resize and RGBA / row copy | 240 | 16.53 ms | 18.59 ms | 57.75 frames/s |

The DXGI phase means were 9.60 ms waiting/acquiring the next frame, 0.03 ms GPU submission, 2.63 ms readback, and 4.19 ms CPU row copy. The previous 8 MB clone/compare is absent on this path. GPU scaling was actually active. Two unchanged requests were excluded from the capture-frame numerator. The complete native test took 12.32 seconds. The fixture reported 61.76 animation callbacks/s (callbacks are not themselves proof of presented display FPS). A prior final-path run also produced 56.27 capture frames/s, mean 16.63 ms / p95 21.55 ms. Both are capture-only results, not network-delivered FPS.

## Integration follow-up: local bridge

The first full-application moving-scene test at unchanged 1920 × 1080 delivered only 3.726 decoded frames/s. Its sender sample showed DXGI capture 12.991 ms, canvas presentation 3.1 ms and bridge overhead 187.886 ms. The fixture's visible timestamp reached the decoded image with mean 449 ms / p95 759 ms. These results rejected the raw IPC bridge as sufficient even though native capture alone exceeded 30 fps.

The installed Tauri 2.10.2 raw response avoids JSON serialization, but Wry 0.54.2 still copies it into `SHCreateMemStream` and returns a WebView2 web-resource response. The new Windows `rgba-shared-v1` transport uses Microsoft's public [CreateSharedBuffer](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2environment12) and [PostSharedBufferToScript](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2_17) APIs. It preserves full RGBA dimensions and sends only a 12-byte acknowledgement through ordinary IPC. Each request owns one read-only mapping; capture and presentation remain sequential with no frame queue. Native closes its mapping after posting; JavaScript releases its independent view after synchronous canvas presentation, following the documented [shared-buffer lifetime](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2sharedbuffer).

Each event carries a per-request random token, session ID, revision and frame sequence. The dispatcher rejects mismatches and releases duplicate or late buffers after errors, timeout or close. The capture loop releases in `finally`, including obsolete revisions and failed presentation. Native rechecks session authority/revision and the originating page URL immediately before sharing. Only an unavailable WebView2 interface can select the raw fallback; authorization errors never fall back. Other platforms retain the compatible raw path. No localhost listener or external port was added.

The new Rust module passed an isolated metadata typecheck using the actual existing Tauri/WebView2 dependencies. Full application compilation and end-to-end shared-buffer performance are recorded separately by the integration checks; implementation alone does not establish the frame-rate target.

## Codec negotiation regression

Whole-FMTP equality incorrectly excluded HEVC when encoder `level-id=123` and decoder `level-id=180` were otherwise compatible. [WebRTC's codec preference rules](https://w3c.github.io/webrtc-pc/#dom-rtcrtptransceiver-setcodecpreferences) delegate asymmetric codec parameters to the browser. The implementation now sorts the unchanged receiver capability objects, retaining RTX/FEC and supported profiles. It does not manufacture a codec configuration or append encoder variants of the same profile. A first attempt using the entire sender/receiver union passed API validation in Chromium 154 but produced `m=video 0` in the answer; actual video negotiation caught that regression and the approach was removed.

The final production function passed a real isolated Chrome 154 loopback: HEVC level 123 was negotiated against the decoder's level 180 capability, with 65 frames encoded and 65 decoded at 320 × 180, connected ICE and no runtime exceptions. Run `rtk proxy python docs/verification/2026-10-10-remote-codec-loopback.py`; evidence is in `2026-10-10-remote-codec-loopback-production.json`. This is a codec regression check, not 1080p performance or cross-device evidence. The tested native WebView2 154 did not expose HEVC capabilities; preference ordering cannot provide an absent engine capability.

## Latest active RDP result

With an active 1920 × 1080 RDP desktop in the test process session, the complete native Worker path delivered **31.39 displayed FPS** in the foreground and **31.40 FPS** while minimized with its sender debugger disconnected. All eight application flows passed after fixing HEVC control-message handling during input-channel replacement. DXGI and NVIDIA hardware HEVC were active; mean source-pixel age was **164.68 ms**, p95 **203 ms**. No compiler or Codebase MCP was present. The moving-region/barcode workload and ordinary desktop load are recorded in the [follow-up](2026-10-10-remote-followup.md).

The earlier powered-off/unchanged-frame trials below describe a different desktop state and are not the current RDP outcome. Microsoft's [RDP frame-rate guidance](https://learn.microsoft.com/en-us/troubleshoot/windows-server/remote/frame-rate-limited-to-30-fps) describes remote-session frame-rate limits; this is a possible environmental factor, not proof of the exact bottleneck in this Windows 11 test. The system's DWMFRAMEINTERVAL override was absent and no registry or RDP policy was modified. Hardware decoding, physical audio, genuine two-device LAN and the requested latency still require verification.

## Earlier acceptance findings

The RGBA/shared-buffer fallback and the initial native HEVC path were measured inside the complete application. Raw RGBA delivered 3.73 FPS; shared buffer delivered 4.68 FPS. Native hardware HEVC over document IPC delivered 13.03 displayed FPS at 1080p, falling to 1.33 FPS with the host minimized. That HEVC run passed the eight functional scenarios but still failed the performance target. Mean source-pixel age was 303.08 ms and p95 was 510 ms. A separate background-flags diagnostic was unsuccessful and those flags were not added to production. Reports are preserved beside this document.

The current Worker transport and direct canvas remove document scheduling and canvas recapture from the HEVC hot path. Integrated normal-configuration trials have not passed. The final native startup fix switches to actual GDI after four first-frame DXGI timeouts and retries DXGI every five seconds, without downgrading a previously successful static DXGI stream. It delivered the first 1080p fallback image, but the latest read-count trial had 4 changed and 105 unchanged native frames across 109 completed reads. The 12-second observation interval delivered no new frames. There were no compiler/indexer processes during that interval; ordinary desktop and test load averaged 56.77% total CPU.

Windows later reported session/console display power off, on during a native receiving-video check, then off again. A scoped execution-state request did not produce an on notification. The physical display state needs confirmation before further DXGI performance measurements; these observations do not by themselves prove the earlier failure's cause. See the follow-up report and `2026-10-10-display-power-state.json`. This document does not certify the new path's frame rate or latency.

The requested baseline (about 32 delivered fps, about 60 ms frame latency, hardware HEVC at both endpoints, encrypted UDP and low bandwidth) is not certified by a capture-only benchmark, a 30 fps setting, a capability list, or a localhost loopback. It requires an animated, fixed-resolution Windows-to-macOS test with negotiated codec/hardware proof, display-frame timing and loss/bandwidth measurements. The 405.5 Kbps screenshot is workload-dependent and must be compared using a defined scene; a static desktop cannot establish performance for motion/video.

macOS receiving-engine/hardware verification and genuine two-device LAN acceptance remain necessary. Unsupported HEVC/hardware fields stay unknown rather than being filled from preferences or the installed GPU name.
