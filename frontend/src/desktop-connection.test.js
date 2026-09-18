import assert from "node:assert/strict";
import test from "node:test";
import {
  canRetryMessage,
  createXChatModule,
  fileMessageActions,
  HttpWsAdapter,
  mergeMessages,
  messageDeliveryStatus,
  normalizeMessage,
  peerConnectionStatus,
  TauriAdapter,
} from "./xchat.js";

test("discovery presence never implies a verified connection", () => {
  assert.equal(peerConnectionStatus({ is_offline: false }), "stale");
  assert.equal(peerConnectionStatus({ is_offline: true }), "missing");
  assert.equal(peerConnectionStatus({ connection: { status: "mismatch" } }), "mismatch");
  assert.equal(peerConnectionStatus({ connection: { status: "updated" } }), "updated");
  assert.equal(peerConnectionStatus({ connection: { status: "ready" } }, true), "verifying");
});

test("receipt metadata distinguishes uncertainty and cannot downgrade delivery", () => {
  const message = normalizeMessage({ own: true, client_message_id: "stable", msg_type: "quote", status: "sent", delivery_state: "unconfirmed", delivery_error: "ack timeout" });
  assert.equal(messageDeliveryStatus(message), "unconfirmed");
  assert.equal(canRetryMessage(message), true);
  for (const status of ["delivered", "read"]) {
    const [merged] = mergeMessages([{ ...message, status }], [{ ...message, status: "failed" }]);
    assert.equal(messageDeliveryStatus(merged), status);
    assert.equal(canRetryMessage(merged), false);
  }
  assert.equal(canRetryMessage({ ...message, delivery_state: "sending" }), false);
  assert.equal(canRetryMessage({ ...message, msg_type: "file" }), false);
});

test("file menu requires a completed local file and supported platform operations", () => {
  const file = { file_status: "accepted", file_path: "/downloads/test.xlsx", local_available: true };
  const capabilities = { revealFile: true, openOutgoingFile: true, saveFileAs: true };
  assert.deepEqual(fileMessageActions(file, capabilities), { open: true, reveal: true, saveAs: true });
  for (const status of ["offered", "downloading", "invalid", "removed", "failed"]) {
    assert.deepEqual(fileMessageActions({ ...file, file_status: status }, capabilities), { open: false, reveal: false, saveAs: false });
  }
  assert.deepEqual(fileMessageActions({ ...file, local_available: false }, capabilities), { open: false, reveal: false, saveAs: false });
  assert.deepEqual(fileMessageActions(file, {}), { open: true, reveal: false, saveAs: false });
  assert.equal(fileMessageActions({ ...file, own: true }, {}).open, false);
});

test("desktop and web refresh adapters use the shared refresh and rediscovery APIs", async () => {
  const desktopCalls = [];
  const desktop = new TauriAdapter({ core: { invoke: (...args) => { desktopCalls.push(args); return Promise.resolve(); } } });
  const webCalls = [];
  const web = new HttpWsAdapter();
  web.json = (...args) => { webCalls.push(args); return Promise.resolve(); };
  await desktop.refreshPeerConnection("peer/id");
  await desktop.rediscoverPeers();
  await web.refreshPeerConnection("peer/id");
  await web.rediscoverPeers();
  assert.deepEqual(desktopCalls, [["refresh_peer_connection", { peerId: "peer/id" }], ["rediscover_peers", undefined]]);
  assert.deepEqual(webCalls, [["/api/peers/peer%2Fid/refresh", "POST", {}], ["/api/peers/rediscover", "POST", {}]]);
});

async function withWorkspace(handler, run, messages = []) {
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, "window");
  const events = new Map();
  const calls = [];
  const backend = {
    self: { id: "self" },
    devices: [{ id: "peer", addr: "192.168.1.42:8888", remark: "Alice", is_offline: false }],
    conversations: [{ id: "direct:peer:self", kind: "direct", peer_id: "peer" }],
    settings: {},
    files: [],
    transfers: [],
  };
  let workspace;
  try {
    Object.defineProperty(globalThis, "window", { configurable: true, value: { __TAURI__: {
      core: { invoke: async (command, payload) => {
        calls.push([command, payload]);
        if (command === "get_workspace_snapshot") return backend;
        if (command === "get_conversation_messages") return messages;
        return handler(command, payload, backend);
      } },
      event: { listen: async (name, listener) => { events.set(name, listener); return () => {}; } },
    } } });
    workspace = createXChatModule();
    assert.equal((await workspace.dispatch({ type: "bootstrap" })).ok, true);
    await run({ workspace, backend, calls, events });
  } finally {
    await workspace?.dispatch({ type: "shutdown" });
    if (descriptor) Object.defineProperty(globalThis, "window", descriptor);
    else delete globalThis.window;
  }
}

test("refresh clicks coalesce and a verified address reaches the device and conversation", async () => {
  let finish;
  await withWorkspace((command) => command === "refresh_peer_connection" ? new Promise((resolve) => { finish = resolve; }) : [], async ({ workspace, backend, calls, events }) => {
    const first = workspace.dispatch({ type: "device.refreshConnection", id: "peer" });
    const duplicate = workspace.dispatch({ type: "device.refreshConnection", id: "peer" });
    assert.equal(workspace.getSnapshot().peerRefreshes.peer, true);
    assert.equal(calls.filter(([command]) => command === "refresh_peer_connection").length, 1);
    const connection = { peer_id: "peer", status: "updated", address: "192.168.1.86:8888", previous_address: backend.devices[0].addr, verified_at: 1234 };
    backend.devices[0] = { ...backend.devices[0], addr: connection.address, connection };
    events.get("peer-connection-changed")({ payload: { peer_id: "peer", connection } });
    finish(connection);
    assert.equal((await first).ok, true);
    assert.equal((await duplicate).ok, true);
    const snapshot = workspace.getSnapshot();
    assert.equal(snapshot.devices[0].addr, connection.address);
    assert.equal(snapshot.conversations[0].peer.addr, connection.address);
    assert.equal(snapshot.devices[0].remark, "Alice");
    assert.equal(snapshot.devices.length, 1);
    assert.deepEqual(snapshot.peerRefreshes, {});
  });
});

test("identity mismatches preserve the saved address", async () => {
  await withWorkspace((command, payload, backend) => {
    if (command !== "refresh_peer_connection") return [];
    const connection = { peer_id: payload.peerId, status: "mismatch", address: "192.168.1.99:8888", error: "different device" };
    backend.devices[0].connection = connection;
    return connection;
  }, async ({ workspace }) => {
    const result = await workspace.dispatch({ type: "device.refreshConnection", id: "peer" });
    assert.equal(result.ok, true);
    assert.equal(workspace.getSnapshot().devices[0].addr, "192.168.1.42:8888");
    assert.equal(workspace.getSnapshot().conversations[0].peer.connection.status, "mismatch");
  });
});

test("retry reuses the quote identity, wire payload and mentions without duplicating a bubble", async () => {
  const content = JSON.stringify({ text: "reply", reply: { client_message_id: "quoted", content: "original" } });
  const message = { id: 42, own: true, client_message_id: "retry-id", conversation_id: "direct:peer:self", sender_id: "self", content, msg_type: "quote", status: "sent", delivery_state: "unconfirmed", mention_ids: ["peer"] };
  let finish;
  await withWorkspace((command) => command === "send_conversation_message" ? new Promise((resolve) => { finish = resolve; }) : [], async ({ workspace, calls }) => {
    const first = workspace.dispatch({ type: "message.retry", conversationId: message.conversation_id, clientMessageId: message.client_message_id });
    const duplicate = workspace.dispatch({ type: "message.retry", conversationId: message.conversation_id, clientMessageId: message.client_message_id });
    assert.equal(calls.filter(([command]) => command === "send_conversation_message").length, 1);
    assert.deepEqual(calls.find(([command]) => command === "send_conversation_message")[1], { conversationId: message.conversation_id, clientMessageId: "retry-id", content, msgType: "quote", mentionIds: ["peer"] });
    finish({ ...message, status: "delivered", delivery_state: "delivered" });
    assert.equal((await first).ok, true);
    assert.equal((await duplicate).ok, true);
    const rows = workspace.getSnapshot().messagesByConversation[message.conversation_id];
    assert.equal(rows.length, 1);
    assert.equal(rows[0].content, "reply");
    assert.equal(rows[0].status, "delivered");
    assert.equal(rows[0].client_message_id, "retry-id");
  }, [message]);
});

test("native delivery ACKs arriving as new-message survive a stale message reload", async () => {
  const message = { id: 10, own: true, client_message_id: "ack-id", conversation_id: "direct:peer:self", sender_id: "self", content: "hello", msg_type: "text", status: "sent", delivery_state: "unconfirmed" };
  await withWorkspace(() => [], async ({ workspace, events }) => {
    events.get("new-message")({ payload: { msg_type: "delivery_ack", conversation_id: message.conversation_id, message_ids: ["ack-id"] } });
    assert.equal(messageDeliveryStatus(workspace.getSnapshot().messagesByConversation[message.conversation_id][0]), "delivered");
    await workspace.dispatch({ type: "conversation.loadOlder" });
    assert.equal(messageDeliveryStatus(workspace.getSnapshot().messagesByConversation[message.conversation_id][0]), "delivered");
    // The event-triggered first-page fetch must also preserve the local ACK.
    await new Promise((resolve) => setTimeout(resolve, 160));
    assert.equal(messageDeliveryStatus(workspace.getSnapshot().messagesByConversation[message.conversation_id][0]), "delivered");
  }, [message]);
});

test("stable-ID retries never fall back to an API that creates another message", async () => {
  const desktopCalls = [];
  const desktop = new TauriAdapter({ core: { invoke: async (command) => { desktopCalls.push(command); throw new Error("unknown command"); } } });
  const webCalls = [];
  const web = new HttpWsAdapter();
  web.json = async (path) => { webCalls.push(path); throw Object.assign(new Error("not found"), { status: 404 }); };
  const conversation = { id: "direct", kind: "direct", peer_id: "peer" };
  await assert.rejects(desktop.sendMessage(conversation, "same-id", "text", "text", [], false));
  await assert.rejects(web.sendMessage(conversation, "same-id", "text", "text", [], false));
  assert.deepEqual(desktopCalls, ["send_conversation_message"]);
  assert.deepEqual(webCalls, ["/api/conversations/direct/messages"]);
});

test("message change notifications refresh authoritative delivery attempts without adding a message", async () => {
  const message = { id: 10, own: true, client_message_id: "attempt-id", conversation_id: "direct:peer:self", sender_id: "self", content: "hello", msg_type: "text", status: "sent", delivery_state: "sending" };
  await withWorkspace(() => [], async ({ workspace, events }) => {
    for (const [eventName, state] of [["message-changed", "awaiting_ack"], ["message.changed", "unconfirmed"]]) {
      assert.ok(events.get(eventName), `${eventName} is subscribed`);
      message.delivery_state = state;
      events.get(eventName)({ payload: { conversation_id: message.conversation_id, client_message_id: message.client_message_id } });
      await new Promise((resolve) => setTimeout(resolve, 160));
      const rows = workspace.getSnapshot().messagesByConversation[message.conversation_id];
      assert.equal(rows.length, 1);
      assert.equal(messageDeliveryStatus(rows[0]), state);
      assert.equal(rows[0].client_message_id, "attempt-id");
    }
  }, [message]);
});
