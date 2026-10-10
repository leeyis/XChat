# Native HEVC / SCTP / WebCodecs verification

Date: 2026-10-10. This report covers the independent JavaScript transport and decoder. It does **not** establish the complete Windows → macOS product's frame rate or end-to-end latency.

## Implemented boundary

`frontend/src/remote-hevc.js` uses the existing authenticated `RTCPeerConnection`. Its `xchat-hevc-video-v1` DataChannel is unordered with `maxRetransmits: 0`. The existing reliable control channel carries capability, decoder configuration/acknowledgement, frame acknowledgements, keyframe requests, revision resets, bounded encoder metadata, and explicit fallback messages.

- `createRemoteHevcSender(...)`: `supported`, `ready`, `failed`, `sendFrame(value, {revision})`, `handleControl`, `stats`, `reset`, `close`, `fail`.
- `createRemoteHevcReceiver(...)`: `probe`, `bindChannel`, `handleControl`, `stats`, `reset({revision, paused})`, `close`, `ready`, `failed`.
- `decodeNativeHevcFrame(...)`: validates XHV1 bytes and returns the Annex B access unit, native sequence/PTS, dimensions, actual encoder identity/hardware flag, and capture/encode/native timings. It accepts the original 64-byte fixed header and the 68-byte header with `native_us` at offset 64.
- `hevcCodecFromAnnexB(...)`: obtains profile, bit-reversed compatibility flags, tier, level, and constraint flags from the actual SPS, including emulation-prevention removal. The native fixture yields `hev1.1.6.H120.90`.

`onFrame(frame, metadata)` borrows a `VideoFrame` synchronously. The module closes it in `finally`; a consumer that needs to retain it must explicitly clone and own the clone. Metadata includes the accepted session revision. `onReady` occurs only after the first actual decoder-output acknowledgement, so the adapter can retain RTP until decoding succeeds. The first frame is the sole pending capture during cold decoder initialization; subsequent frames do not wait for per-frame acknowledgements.

The receiver calls `VideoDecoder.isConfigSupported` again for each actual SPS configuration. Omitting `description` selects Annex B; independently decodable key access units contain IRAP plus VPS/SPS/PPS. These format rules come from the [W3C HEVC registration](https://www.w3.org/TR/webcodecs-hevc-codec-registration/). `hardwareAcceleration: "prefer-hardware"` is a hint, so the module deliberately reports `hardwareDecoder: null`, and never equates a hardware-only encoder with power efficiency. See the [WebCodecs specification](https://www.w3.org/TR/webcodecs/).

## Bounds and lifecycle

Native access units are limited to 4 MiB; coded dimensions are bounded to at most 4K pixel area. The application packet header is 48 bytes (`XHC1`), containing the random stream epoch, monotonically assigned transport sequence, PTS, total frame size, fragment offset/index/count, and duration. Each SCTP message is smaller than 60,000 bytes and no larger than `pc.sctp.maxMessageSize`.

The sender admits a complete frame only within a 4 MiB `bufferedAmount` budget and has no frame queue. The receiver holds at most three assemblies / 8 MiB and three decoder submissions. Assemblies expire after 180 ms. Missing references, overflow, or partial sends request an independent keyframe. Existing valid decoder submissions are allowed to complete rather than repeatedly destroying the hardware decoder on congestion. An independently decodable keyframe can bridge an entirely missing predecessor.

Decoded-frame acknowledgements also detect a completely missing **last** desktop update, where no later screen change exists to expose a sequence gap. The sender requests another IDR after an acknowledgement timeout. These are application frame counters, not a measurement of IP packet loss; `lossPercent` stays null.

All session/revision changes synchronously invalidate sender and receiver generations. Pending config, channel-open, and first-output waits are cancellable. Late decoder outputs are closed without presentation. Receiver reset retains the expected revision even before the control channel opens, and requests a fresh configuration/IDR once delivery is available. A native worker restart that resets native sequence/PTS opens a new random stream epoch and decoder configuration; native sequence is not reused as a transport sequence.

`stats()` computes actual interval frame/byte rates. `kbps` counts application video packets, excluding reliable control messages and DTLS/SCTP/IP overhead; it is not total NIC utilization. `decodeMs` is decoder submission to output, and `receiveToDecodeMs` begins with the first received fragment. Neither is a cross-machine one-way or glass-to-glass latency measurement. `transport` is `dtls-sctp`, not SRTP. ICE path/protocol/RTT remain the parent connection's measurements.

## Verification

Command: `rtk proxy node --test frontend/src/remote-hevc.test.js`.

Result: **17 passed, 0 failed**, covering native header validation, actual SPS codec derivation, bounded fragmentation/reordering/duplicates, explicit unsupported/config-timeout fallback, IDR recovery, a wholly lost static last frame, native sequence reset, revision/session isolation, first-output handoff, pause during config/channel waits, late decoder output disposal, and initial reset before control opens.

Command: `rtk proxy python docs/verification/2026-10-10-remote-hevc-loopback.py`.

This launches an isolated headless Chrome profile and two peer connections using only the generated synthetic native-MFT fixture. It neither captures nor reads a real desktop. The owned test browser is closed on completion.

Latest full report: [remote-hevc-loopback.json](2026-10-10-remote-hevc-loopback.json).

- Chrome **154.0.8037.98**, actual HEVC codec **hev1.1.6.H120.90**.
- **1920 × 1080, 60/60 access units decoded**, with 89 application video packets when forcing 1,200-byte fragmentation; zero clean-phase drops.
- Cold-start handoff waits for actual output. Latest 30-rate timer-paced fixture measured **25.73 FPS including configuration and scheduling**, not a 30 FPS acceptance result.
- Exactly one deliberately omitted fragment caused a keyframe request; a forced IRAP and following delta decoded successfully.
- After 2.1 seconds idle, resetting native sequence to 1 and PTS to 33,333 μs produced a distinct epoch and decoded two frames.
- Three delayed old-revision fragments delivered after pause produced **zero stale frames**; the resumed frame carried revision 3.
- No runtime exceptions. Decoder hardware identity remains unknown.

An earlier 60-rate capacity run decoded 60/60 at **47.20 FPS** ([sample report](2026-10-10-remote-hevc-loopback-60.json)). Another 60-rate run during the concurrent native build reached the bounded decoder queue and requested IDR; its prerecorded producer then stopped rather than dynamically encoding a fresh IDR. Consequently that single capacity sample is not evidence of sustained 60 FPS. The latest 30-rate report includes the subsequent revision/cancellation tests; the earlier 60-rate sample predates those additions.

The remaining acceptance work is the parent's real native-capture → IPC → HEVC channel → displayed remote-view measurement, including minimized/occluded host behavior and macOS WKWebView. This module's tests establish protocol correctness and observable throughput samples, not those platform-wide acceptance thresholds.
