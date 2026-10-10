# Native HEVC Worker transport

This change removes the main WebView's per-frame IPC callback from native HEVC
capture. It does not by itself establish a frame-rate result. Full application
and background-window measurements remain separate from the checks below.

## Protocol

Only the native main window can invoke `remote_capture_stream` with the current
`actor`, session `id`, and screen `revision`. The command checks ownership,
accepted native host consent, the unexpired owner lease, and a visible sharing
toolbar. It returns:

```json
{
  "url": "ws://127.0.0.1:<actual listener port>/api/remote/native/stream",
  "token": "<one-use 64-character random capability>",
  "format": "hevc-v1",
  "expires_in_ms": 15000
}
```

The token is sent in the first WebSocket text message, never in the URL:

```json
{"type":"auth","token":"<capability>"}
```

Authentication must arrive within three seconds. Success returns
`{"type":"ready","format":"hevc-v1","revision":<bound revision>}`.
`{"type":"next","request_keyframe":true}` requests one frame and returns one
binary XHV1 packet. Subsequent requests can omit `request_keyframe`. The client
cannot provide or change actor, session, revision, capture format, or destination.
`{"type":"stop"}` closes and revokes the active capability.

One native frame future and one bounded binary reply can be in flight. The
server calls the existing `Hub::frame` before sending a frame and checks the
capability again after encoding. The maximum reply is 8 MiB and a stalled socket
write times out after two seconds. There is no frame queue accumulated for a
slow consumer. Native capture still performs GPU NV12 conversion followed by
CPU staging readback and hardware MF encoding; this is not a zero-copy claim.

## Authorization and lifecycle

- Only loopback peers are accepted. Host must be the exact returned
  `127.0.0.1:<port>`, Origin must match the native issuer's actual WebView origin,
  and the running Axum `AppState` service generation must match the capability.
- A ticket can be claimed once and expires after 15 seconds. Minting a new ticket
  revokes the prior active socket; an old socket's close cannot revoke the new
  ticket. Tickets are held only in memory and are not written to history or URLs.
- Every pull validates the exact actor, session and revision under the same
  state lock before renewing the owner lease. Expired, paused, replaced or ended
  sessions cannot be revived. Minimized main-window JavaScript polling is not
  required to maintain an otherwise current Worker capture lease.
- Native main-window existence, toolbar visibility and scope are rechecked every
  second. Main-window visibility or minimization is deliberately not a condition;
  a destroyed main window invalidates capture. State is also
  checked for every pull and before an encoded result is sent. Closing the
  socket, sending stop, or stopping the network service revokes the lease. The
  pending frame future is dropped before the close handshake, so queued native
  work sees its cancelled reply and is skipped. An already running native call
  can finish, but its result cannot be sent.
- `remote_capture_busy` returns an error with `retryable:true` on the same socket.
  The Worker bounds retries to five seconds. Other errors close the socket.
  `remote_paused`, `remote_revision_changed`, and `remote_session_ended` identify
  expected authoritative session changes; a replacement revision requires a new
  main-window capability.
- The existing main capability includes the new command through
  `allow-remote-assistance`. The independent viewer's narrow command permission
  does not include it. Both native entry points register the command.

## Focused verification

No Cargo build, actual screen capture, or GPU benchmark was run for these checks
while the root agent's native build was active.

```text
rtk proxy python analysis/remote-perf-20261010/build-capture-stream-harness.py --desktop
rtk proxy python analysis/remote-perf-20261010/build-capture-stream-harness.py
```

Both configurations typechecked the actual `capture_stream.rs` and passed all
four tests: single use/TTL/origin/listener generation binding; replacement and
late-close isolation; authorized owner renewal with expiry/pause/revision/ended
denials; and loopback/request-field restrictions. The harness imports the real
model, Session, and `native_session` authorization function. Database, native
window and frame I/O are explicit stubs; these results do not claim integrated
Tauri or real WebSocket transport success. Initial harness setup exposed a
missing Clone on its server-lifetime stub and a missing Windows import-library
search path; both harness issues were corrected before the passing runs.

`rustfmt --check` for the new module and `git diff --check` for touched integration
files passed. Full desktop/web feature builds and real background Worker
transport verification must use the complete application.

The complete desktop compile check subsequently passed with the main-window
existence and pending-future cleanup fixes. The isolated `cargo tauri dev`
application was rebuilt in 4m 55s and started on port 18888 with its disposable
database. The shared web feature check and web executable build also passed.
These compilation results do not override the failed first-frame/performance
trials recorded in the follow-up report.
