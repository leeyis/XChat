# HEVC worker capture investigation and verification

The native application test initially delivered 13.03 displayed FPS at 1080p,
then 1.33 FPS with its host window minimized and its debugger disconnected.
Its native capture and HEVC encode measurements were much shorter than the
observed frame interval. Removing the document's capture timer had not removed
the entire document scheduling dependency.

The new path is:

`authorized native capture → one binary localhost WebSocket response → DedicatedWorker → transferred existing-PC RTCDataChannel → WebCodecs receiver → visible canvas`

The window still owns session authorization, signaling, reliable peer control,
and infrequent diagnostic updates. It no longer forwards each native frame.
The worker does not create another RTCPeerConnection. Only one native request
and its processing are outstanding; the existing sender also applies whole-frame
SCTP admission limits. The native response's revision is bound by the one-use
authorization token. A revision update closes the old socket and invalidates
pending output before a fresh descriptor can be consumed.

Public API and source findings:

- The WebRTC specification exposes transferable RTCDataChannel objects to
  DedicatedWorker. Transfer must occur in the creating task, before sending.
  The original object becomes closed after transfer while the transferred
  object retains the transport. This implementation calls postMessage with
  the transfer list immediately after createDataChannel, without an await.
  [WebRTC specification](https://w3c.github.io/webrtc-pc/#rtcdatachannel)
- Tauri 2.10.2 `scripts/core.js`, `scripts/ipc-protocol.js`, and
  `src/ipc/channel.rs` were inspected locally. invoke and Channel dispatch rely
  on the injected window callbacks; the invoke key is intentionally enclosed
  in a private closure. Simply using Channel or forwarding its callback to a
  worker would leave a document callback in the hot path. The implementation
  uses a separately authenticated native route instead of reproducing private
  Tauri IPC internals.
- Wry 0.54.2 `src/webview2/mod.rs` registers custom-protocol request filters for
  all request source kinds through ICoreWebView2_22. A worker-accessible custom
  protocol was another viable route. The chosen loopback WebSocket also avoids
  native WebResourceRequested dispatch for every frame.
- Wry 0.54.2's `with_background_throttling` documents Windows as unsupported.
  Microsoft's PreferredBackgroundTimerWakeInterval API remains prerelease in
  the checked documentation and explicitly leaves other background policies
  independent. Neither is treated as a supported blanket fix.
  [Microsoft API documentation](https://learn.microsoft.com/en-us/dotnet/api/microsoft.web.webview2.core.corewebview2settings.preferredbackgroundtimerwakeinterval?view=webview2-dotnet-1.0.4071-prerelease)
- Canvas requestFrame marks a requested capture; capture occurs when the canvas
  is painted. Recapturing a decoded canvas into a MediaStream and displaying
  it in a video element adds another rendering/capture boundary. Direct canvas
  presentation removes that boundary. This source explains a plausible cause
  of decoded-versus-displayed frame differences, not proof of a particular
  measured latency reduction.
  [Canvas capture specification](https://www.w3.org/TR/mediacapture-fromelement/#html-canvas-element-media-capture-extensions)

Implementation files are `frontend/src/remote-hevc-worker.js` and
`frontend/src/remote-hevc-worker-client.js`. A synchronous transfer failure
reuses the still-owned, unsent channel with the existing sender. A failure after
successful transfer produces explicit RTP fallback instead of trying to reclaim
the detached object. Native terminal errors notify the peer through reliable
control so its presentation also returns to RTP. `remote_capture_busy` retries
the same socket with 25 ms waits, bounded to five seconds. A normal pause,
revision change, or session end stops requests and waits for the authoritative
session update. Stale capture callbacks cannot undo a pause or decrease revision.

Verification performed:

- `rtk proxy node --test frontend/src/remote-hevc.test.js frontend/src/remote-hevc-worker.test.js`:
  39 passing tests after the receiver backpressure/ACK follow-up. The static-desktop reset test verifies that a new epoch
  is not suppressed by the previous epoch's 200 ms IDR-request throttle.
- `rtk proxy python docs/verification/2026-10-10-remote-worker-transfer.py`:
  isolated Edge 154.0.4258.62 transferred the channel and completed a real
  1,200-byte binary roundtrip from the worker, with zero exceptions.
- `rtk proxy python docs/verification/2026-10-10-remote-hevc-worker-loopback.py`:
  production modules, two real peer connections, transferred worker channel,
  synthetic native WebSocket, and Edge WebCodecs decoded 83 frames at unchanged
  1920×1080. Two capture calls drove two revisions. The 500 ms deliberate window
  JavaScript stall still allowed eight native reads from the worker. Pause and
  resume presented zero stale frames and decoded 12 frames in the new revision.
- The last replay's pre-stall delivery mean was 18.71 FPS, with four receiver
  drops recovered by IDR, while its latest sender interval was 30.20 FPS. This
  is not a 30/32 FPS acceptance result. Replay timing and the synthetic native
  header values are not live native capture or end-to-end latency measurements.
- `rtk npm run build -- --outDir ../analysis/remote-perf-20261010/worker-vite-build-final`:
  passed. The production entry references one self-contained 15,195-byte worker
  asset, with no unresolved relative import. The default branch uses Vite's
  literal `new Worker(new URL(..., import.meta.url), ...)` bundling pattern.

The transfer and synthetic-worker reports are stored beside this note. These
tests establish transfer support, bounded operation, revision cancellation, and
independence from a blocked window JavaScript task. They do not establish
minimized WebView2 behavior, sustained live desktop FPS, physical LAN latency,
or hardware decoding on macOS. Those require the separate native application
and platform checks. Worker APIs do not promise exemption from every browser
lifecycle suspension policy. No global browser throttling flags are part of
this production implementation.

Receiver follow-up: reaching three in-flight decode operations now retains the bounded pending queue instead of immediately discarding valid references. Decoder output/dequeue schedules a guarded microtask to continue draining. Queue, memory and age limits are unchanged; overflow and timeout still request an independent frame. Reset/close and synchronous decoder callbacks cannot restore an old generation. Decoded-frame ACKs are immediate for the first output and subsequently coalesced at 200 ms, including a trailing ACK for a static final frame. The sender clears its loss watchdog only when the acknowledged sequence reaches the latest sent frame. Nine additional tests cover these boundaries; the earlier synthetic replay numbers predate this follow-up and are not updated performance evidence.
