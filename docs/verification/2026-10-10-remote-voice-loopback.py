"""Real WebRTC loopback with Chrome's synthetic microphone; no Xchat app/data.

Run from the repository root: rtk python docs/verification/2026-10-10-remote-voice-loopback.py
Requires Chrome and Python websockets. Serves only an empty test page and the
production remote-voice.js module, on the loopback interface with a new profile.
"""
import asyncio
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.request import urlopen

import websockets

ROOT = Path(__file__).resolve().parents[2]
REPORT = ROOT / "docs/verification/2026-10-10-remote-voice-loopback.json"
PORT = 18943
CHROME = Path(os.environ.get("PROGRAMFILES", "C:/Program Files")) / "Google/Chrome/Application/chrome.exe"


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/remote-voice.js":
            content = (ROOT / "frontend/src/remote-voice.js").read_bytes()
            mime = "text/javascript; charset=utf-8"
        elif self.path == "/":
            content = b"<!doctype html><title>Xchat isolated synthetic voice loopback</title>"
            mime = "text/html; charset=utf-8"
        else:
            self.send_error(404)
            return
        self.send_response(200)
        self.send_header("Content-Type", mime)
        self.send_header("Cache-Control", "no-store")
        self.send_header("Content-Length", str(len(content)))
        self.end_headers()
        self.wfile.write(content)

    def log_message(self, *_):
        pass


class Page:
    async def connect(self, target):
        self.ws = await websockets.connect(target["webSocketDebuggerUrl"], max_size=4 * 1024 * 1024)
        self.sequence = 0
        self.errors = []
        return self

    async def call(self, method, params=None):
        self.sequence += 1
        token = self.sequence
        await self.ws.send(json.dumps({"id": token, "method": method, "params": params or {}}))
        while True:
            value = json.loads(await asyncio.wait_for(self.ws.recv(), 45))
            if value.get("method") == "Runtime.exceptionThrown":
                self.errors.append(value["params"])
            if value.get("id") == token:
                if "error" in value:
                    raise RuntimeError(value["error"])
                return value.get("result", {})

    async def js(self, source):
        result = await self.call("Runtime.evaluate", {"expression": source, "returnByValue": True, "awaitPromise": True, "userGesture": True})
        if "exceptionDetails" in result:
            raise AssertionError(result["exceptionDetails"])
        return result.get("result", {}).get("value")


TEST = r"""
(async () => {
  const {syncRemoteVoice, microphoneCapabilityError} = await import('/remote-voice.js');
  const assert = (condition, message) => {if (!condition) throw new Error(message);};
  const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
  const until = async (condition, name, timeout=10000) => {
    const end = performance.now() + timeout;
    while (!condition() && performance.now() < end) await wait(25);
    assert(condition(), `Timeout: ${name}`);
  };
  const checks = [], streams = [], audios = [], pcs = [], failures = [];
  let requests = 0, offers = 0, answers = 0;
  const realMicrophone = navigator.mediaDevices.getUserMedia.bind(navigator.mediaDevices);
  const acquire = async options => {requests++;const stream = await realMicrophone(options);streams.push(stream);return stream;};
  navigator.mediaDevices.getUserMedia = acquire;
  assert(!microphoneCapabilityError(), 'Secure loopback should expose getUserMedia');
  const peer = () => {
    const pc = new RTCPeerConnection({iceServers: [], bundlePolicy: 'max-bundle'});pcs.push(pc);
    pc.ontrack = event => {const audio = new Audio();audio.autoplay = true;audio.muted = true;audio.srcObject = new MediaStream([event.track]);audios.push(audio);void audio.play().catch(() => {});};
    return pc;
  };
  const a = peer(), b = peer();
  const aVoice = a.addTransceiver('audio', {direction: 'sendrecv'});
  await a.setLocalDescription(await a.createOffer());offers++;
  await until(() => a.iceGatheringState === 'complete', 'offer ICE');
  await b.setRemoteDescription(a.localDescription);
  const bVoice = b.getTransceivers().find(t => t.receiver.track.kind === 'audio');bVoice.direction = 'sendrecv';
  await b.setLocalDescription(await b.createAnswer());answers++;
  await until(() => b.iceGatheringState === 'complete', 'answer ICE');
  await a.setRemoteDescription(b.localDescription);
  await until(() => a.connectionState === 'connected' && b.connectionState === 'connected', 'connected before microphone');
  assert(requests === 0, 'Negotiation must complete before microphone acquisition in this test');
  assert(aVoice.currentDirection === 'sendrecv' && bVoice.currentDirection === 'sendrecv', 'Both negotiated audio transceivers must remain sendrecv');
  checks.push('real ICE/DTLS + sendrecv audio negotiated before microphone acquisition');
  const state = (voice, name) => ({voice, name, audioEpoch: 0, session: {voice: {stage: 'active', id: 'call-1', local_muted: false}}, changes: [],
    changed(extra) {this.changes.push(extra);}, voiceFailed(error, call) {failures.push({name, message: error.message, call});}});
  const left = state(aVoice, 'left'), right = state(bVoice, 'right'), states = [left, right];
  const metrics = async pc => {
    const stats = await pc.getStats(), summary = {inbound: 0, outbound: 0, received: 0, sent: 0};
    stats.forEach(report => {
      if (report.kind !== 'audio') return;
      if (report.type === 'outbound-rtp') {summary.outbound += report.bytesSent || 0;summary.sent += report.packetsSent || 0;summary.codec = stats.get(report.codecId)?.mimeType;}
      if (report.type === 'inbound-rtp') {summary.inbound += report.bytesReceived || 0;summary.received += report.packetsReceived || 0;}
    });
    return summary;
  };
  const interval = async name => {
    await wait(400);
    const before = await Promise.all(pcs.map(metrics));await wait(1100);
    const after = await Promise.all(pcs.map(metrics));
    const delta = after.map((value, i) => ({side: i ? 'right' : 'left', codec: value.codec,
      inbound_bytes: value.inbound - before[i].inbound, outbound_bytes: value.outbound - before[i].outbound,
      received_packets: value.received - before[i].received, sent_packets: value.sent - before[i].sent}));
    assert(delta.every(x => x.inbound_bytes > 0 && x.outbound_bytes > 0 && x.received_packets > 0 && x.sent_packets > 0), `${name}: RTP not increasing ${JSON.stringify(delta)}`);
    checks.push(`${name}: inbound and outbound audio RTP increases on both peers`);
    return {name, interval_ms: 1100, delta};
  };
  await Promise.all(states.map(s => syncRemoteVoice(s)));
  assert(states.every(s => s.microphone?.getAudioTracks()[0].readyState === 'live'), 'Both microphone tracks should be live');
  const samples = [await interval('late microphone attachment')];
  const firstTracks = states.map(s => s.microphone.getAudioTracks()[0]);
  states.forEach(s => {s.session.voice = {stage: 'idle', id: '', local_muted: false};});
  await Promise.all(states.map(s => syncRemoteVoice(s)));
  assert(firstTracks.every(t => t.readyState === 'ended'), 'Hangup must release old microphones');
  assert(states.every(s => s.voice.sender.track === null), 'Hangup must detach the audio sender');
  assert(pcs.every(pc => pc.connectionState === 'connected'), 'Hangup must retain the peer connections');
  checks.push('voice hangup ends old microphone tracks and leaves both peer connections connected');
  states.forEach(s => {s.session.voice = {stage: 'active', id: 'call-2', local_muted: false};});
  await Promise.all(states.map(s => syncRemoteVoice(s)));
  samples.push(await interval('redial on the same negotiated connections'));
  const previous = left.microphone;
  const device = (await navigator.mediaDevices.enumerateDevices()).find(d => d.kind === 'audioinput' && d.deviceId !== 'default');
  assert(device, 'Chrome fake microphone must provide an input device');
  await syncRemoteVoice(left, device.deviceId);
  assert(left.microphone !== previous && previous.getTracks().every(t => t.readyState === 'ended'), 'Microphone selection must replace and release the previous stream');
  assert(left.voice === aVoice && right.voice === bVoice && offers === 1 && answers === 1, 'Switching must not require SDP renegotiation');
  samples.push(await interval('selected synthetic microphone replacement'));
  const working = left.microphone;
  await syncRemoteVoice(left, 'xchat-qa-no-such-microphone');
  assert(left.microphone === working && working.getAudioTracks()[0].readyState === 'live', 'Failed selection must retain working microphone');
  assert(left.changes.at(-1)?.voiceError && failures.length === 0, 'Device error must not hang up working call');
  samples.push(await interval('failed device selection preserves the call'));
  states.forEach(s => {s.session.voice = {stage: 'idle', id: '', local_muted: false};});
  await Promise.all(states.map(s => syncRemoteVoice(s)));
  let release;
  navigator.mediaDevices.getUserMedia = options => new Promise(resolve => {release = async () => resolve(await acquire(options));});
  left.session.voice = {stage: 'active', id: 'late-consent', local_muted: false};
  const pending = syncRemoteVoice(left);
  assert(release, 'Consent gate must be pending');
  left.session.voice = {stage: 'idle', id: '', local_muted: false};await syncRemoteVoice(left);
  await release();await pending;
  assert(left.microphone === null && streams.at(-1).getTracks().every(t => t.readyState === 'ended'), 'Late permission after hangup must stop returned tracks');
  assert(left.voice.sender.track === null, 'Late permission must not reattach ended audio');
  checks.push('late permission after hangup releases the returned real synthetic MediaStream');
  navigator.mediaDevices.getUserMedia = realMicrophone;
  states.forEach(s => {s.closed = true;s.audioEpoch++;s.microphone?.getTracks().forEach(t => t.stop());});
  pcs.forEach(pc => pc.close());streams.forEach(s => s.getTracks().forEach(t => t.stop()));
  audios.forEach(audio => {audio.pause();audio.srcObject = null;});
  assert(streams.every(s => s.getTracks().every(t => t.readyState === 'ended')), 'Every acquired track must be ended');
  assert(pcs.every(pc => pc.connectionState === 'closed'), 'Every test connection must be closed');
  assert(failures.length === 0, `Unexpected voice failures ${JSON.stringify(failures)}`);
  checks.push('all acquired media tracks and test peer connections closed');
  return {result: 'passed', checks, samples, microphone_requests: requests, offers, answers, failures,
    secure_context: isSecureContext, input_source: 'Chrome --use-fake-device-for-media-stream; no physical microphone',
    network: 'two RTCPeerConnections in one isolated Chromium page, loopback ICE; no cross-device evidence',
    module: 'frontend/src/remote-voice.js'};
})()
"""


async def run(profile, browser):
    deadline = time.monotonic() + 30
    active = profile / "DevToolsActivePort"
    while not active.exists() and time.monotonic() < deadline:
        if browser.poll() is not None:
            raise RuntimeError("Isolated Chrome exited before CDP became available")
        await asyncio.sleep(.1)
    port = active.read_text().splitlines()[0]
    targets = json.load(urlopen(f"http://127.0.0.1:{port}/json/list", timeout=3))
    page = await Page().connect(next(target for target in targets if target["type"] == "page"))
    try:
        await page.call("Runtime.enable")
        await page.call("Page.enable")
        await page.call("Page.navigate", {"url": f"http://127.0.0.1:{PORT}/"})
        for _ in range(60):
            if await page.js("location.port === '18943' && document.readyState === 'complete'"):
                break
            await asyncio.sleep(.1)
        report = await page.js(TEST)
        report["browser"] = await page.call("Browser.getVersion")
        report["runtime_exceptions"] = page.errors
        report["executed_at"] = time.strftime("%Y-%m-%dT%H:%M:%S%z")
        assert not page.errors, page.errors
        REPORT.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        print(json.dumps({"result": report["result"], "checks": len(report["checks"]), "report": str(REPORT)}, ensure_ascii=False), flush=True)
        await page.call("Browser.close")
    finally:
        await page.ws.close()


if __name__ == "__main__":
    if not CHROME.exists():
        raise FileNotFoundError(CHROME)
    server = ThreadingHTTPServer(("127.0.0.1", PORT), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    profile = Path(tempfile.mkdtemp(prefix="xchat-voice-loopback-"))
    browser = subprocess.Popen([str(CHROME), "--headless=new", "--disable-extensions", "--no-first-run", "--no-default-browser-check",
        "--disable-background-timer-throttling", "--autoplay-policy=no-user-gesture-required", "--use-fake-ui-for-media-stream",
        "--use-fake-device-for-media-stream", "--remote-debugging-port=0", "--user-data-dir=" + str(profile), "about:blank"],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
    try:
        asyncio.run(run(profile, browser))
    finally:
        server.shutdown()
        server.server_close()
        if browser.poll() is None:
            browser.terminate()
        try:
            browser.wait(timeout=5)
        except subprocess.TimeoutExpired:
            browser.kill()
        print("Isolated browser profile:", profile, flush=True)
