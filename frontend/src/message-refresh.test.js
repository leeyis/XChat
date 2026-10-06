import assert from "node:assert/strict";
import test from "node:test";
import { createXChatModule } from "./xchat.js";

const conversationId = "direct:peer:self";

async function waitFor(check, description) {
  const deadline = Date.now() + 1500;
  while (!check()) {
    assert.ok(Date.now() < deadline, description);
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
}

async function withWorkspace(run, rowCount = 120) {
  const descriptors = new Map(["window", "document"].map((name) =>
    [name, Object.getOwnPropertyDescriptor(globalThis, name)]));
  const events = new Map();
  const fetches = [];
  const heldFetches = [];
  const backend = {
    self: { id: "self" },
    devices: [{ id: "peer", addr: "192.168.1.42:8888", is_offline: false }],
    conversations: [{ id: conversationId, kind: "direct", peer_id: "peer" }],
    settings: {}, files: [], transfers: [],
  };
  let rows = Array.from({ length: rowCount }, (_, index) => ({
    id: index + 1,
    client_message_id: `message-${index + 1}`,
    conversation_id: conversationId,
    sender_id: "peer",
    content: `row ${index + 1}`,
    msg_type: "text",
    status: "received",
    timestamp: index + 1,
  }));
  let holdNextFetch = false;
  let onFetch = () => {};
  let workspace;
  try {
    Object.defineProperty(globalThis, "document", {
      configurable: true, value: { visibilityState: "hidden", hasFocus: () => false },
    });
    Object.defineProperty(globalThis, "window", {
      configurable: true,
      value: { __TAURI__: {
        core: { invoke: async (command, payload) => {
          if (command === "get_workspace_snapshot") return backend;
          if (command === "get_conversation_messages") {
            fetches.push(payload);
            const end = rows.length - payload.offset;
            const page = structuredClone(rows.slice(Math.max(0, end - Math.min(payload.limit, 200)), end));
            onFetch(payload);
            if (holdNextFetch) {
              holdNextFetch = false;
              return new Promise((resolve) => heldFetches.push(() => resolve(page)));
            }
            return page;
          }
          return [];
        } },
        event: { listen: async (name, listener) => {
          events.set(name, listener);
          return () => {};
        } },
      } },
    });
    workspace = createXChatModule();
    assert.equal((await workspace.dispatch({ type: "bootstrap" })).ok, true);
    const messages = () => workspace.getSnapshot().messagesByConversation[conversationId];
    const emit = (name, payload = {}) => {
      assert.ok(events.has(name), `${name} is subscribed`);
      events.get(name)({ payload });
    };
    await run({
      workspace, fetches, messages, emit, heldFetches,
      holdNext: () => { holdNextFetch = true; },
      afterFetch: (listener) => { onFetch = listener; },
      editRows: (edit) => { rows = edit(rows); },
    });
  } finally {
    heldFetches.forEach((release) => release());
    await workspace?.dispatch({ type: "shutdown" });
    for (const [name, descriptor] of descriptors) {
      if (descriptor) Object.defineProperty(globalThis, name, descriptor);
      else delete globalThis[name];
    }
  }
}

test("transfer updates keep loaded history and still apply authoritative deletion", async () => {
  await withWorkspace(async ({ workspace, fetches, messages, emit, editRows }) => {
    await workspace.dispatch({ type: "conversation.loadOlder" });
    const loadedIds = messages().map((message) => message.id);
    assert.equal(loadedIds.length, 80);
    editRows((rows) => rows.filter((message) => message.id !== 50));
    emit("transfer-changed", { transfer_id: 1, status: "downloading" });
    await waitFor(() => fetches.length === 3, "transfer update must reload the messages");
    await waitFor(() => !messages().some((message) => message.id === 50), "deleted history must disappear");
    const remainingIds = new Set(messages().map((message) => message.id));
    for (const id of loadedIds.filter((id) => id !== 50)) {
      assert.ok(remainingIds.has(id), `loaded message ${id} must survive the refresh`);
    }
  });
});

test("download progress cannot replace a file message before its authoritative refresh", async () => {
  await withWorkspace(async ({ workspace, messages, emit, editRows, holdNext, heldFetches }) => {
    editRows((rows) => rows.map((message) => message.id === 119 ? {
      ...message,
      content: "ProPlus2019Retail.img",
      msg_type: "file",
      sender_name: "ad",
      file_name: "ProPlus2019Retail.img",
      file_path: "C:/Downloads/ProPlus2019Retail.img",
      file_size: 4_200_000_000,
      file_status: "downloading",
      local_available: false,
    } : message));
    await workspace.dispatch({ type: "conversation.open", id: conversationId });
    const before = structuredClone(messages());
    holdNext();
    // This is the actual stable receive event shape: file_name is present,
    // while timestamp, content, sender and the full file metadata are absent.
    const progress = {
      msg_type: "file_download_progress",
      id: 119,
      client_message_id: "message-119",
      conversation_id: conversationId,
      file_name: "ProPlus2019Retail.img",
      file_status: "downloading",
      received: 65_200_000,
      total: 4_200_000_000,
      speed_mb_s: 2.9,
      transfer_id: "receive-progress-regression",
    };
    emit("new-message", progress);
    assert.deepEqual(messages(), before, "a partial progress event must leave message fields and order intact");
    // Tauri can also deliver progress via the event name without msg_type.
    const { msg_type: ignored, ...namedProgress } = progress;
    for (const eventName of ["file_download_progress", "upload_progress"]) {
      emit(eventName, namedProgress);
      assert.deepEqual(messages(), before, `${eventName} must preserve message fields and order without a payload type`);
    }
    await waitFor(() => heldFetches.length === 1, "the authoritative refresh must be pending");
    assert.deepEqual(messages(), before, "a delayed history refresh must not expose a temporary control-message row");
    heldFetches[0]();
    await new Promise((resolve) => setTimeout(resolve, 20));
    assert.deepEqual(messages(), before, "refreshing the same message must keep its original model and order");

    const completed = {
      ...before.find((message) => message.id === 119),
      file_status: "accepted",
      local_available: true,
    };
    editRows((rows) => rows.map((message) => message.id === 119 ? completed : message));
    emit("new-message", completed);
    assert.deepEqual(messages().map((message) => message.id), before.map((message) => message.id));
    assert.equal(messages().find((message) => message.id === 119).file_status, "accepted", "the full completion message must still update immediately");
    await waitFor(() => messages().find((message) => message.id === 119).local_available, "the completed file must become locally available");
  });
});

test("a slow refresh cannot discard history loaded while it was pending", async () => {
  await withWorkspace(async ({ workspace, fetches, messages, emit, holdNext, heldFetches }) => {
    holdNext();
    emit("transfer-changed", { transfer_id: 1, status: "downloading" });
    await waitFor(() => heldFetches.length === 1, "background refresh must be pending");
    await workspace.dispatch({ type: "conversation.loadOlder" });
    assert.equal(messages().length, 80);
    heldFetches[0]();
    await waitFor(() => fetches.length >= 4 || messages().length !== 80, "pending refresh must settle");
    assert.equal(messages().length, 80);
    assert.ok(messages().some((message) => message.id === 41));
  });
});

test("background refresh retains history beyond the backend's 200-row page limit", async () => {
  await withWorkspace(async ({ workspace, fetches, messages, emit, editRows }) => {
    for (let page = 0; page < 5; page += 1) {
      await workspace.dispatch({ type: "conversation.loadOlder" });
    }
    const loadedIds = messages().map((message) => message.id);
    assert.equal(loadedIds.length, 240);
    editRows((rows) => rows.filter((message) => message.id !== 70));
    emit("transfer-changed", { transfer_id: 1, status: "downloading" });
    await waitFor(() => fetches.length >= 7, "transfer update must reload the messages");
    await waitFor(() => !messages().some((message) => message.id === 70), "deleted history must disappear");
    const remainingIds = new Set(messages().map((message) => message.id));
    for (const id of loadedIds.filter((id) => id !== 70)) {
      assert.ok(remainingIds.has(id), `loaded message ${id} must survive the paginated refresh`);
    }
  }, 300);
});

test("a newer first-page response wins over an older background refresh", async () => {
  await withWorkspace(async ({ workspace, messages, emit, holdNext, heldFetches, editRows }) => {
    holdNext();
    emit("transfer-changed", { transfer_id: 1, status: "downloading" });
    await waitFor(() => heldFetches.length === 1, "old background refresh must be pending");
    editRows((rows) => rows.map((message) => message.id === 120
      ? { ...message, content: "updated authoritative content" } : message));
    assert.equal((await workspace.dispatch({ type: "conversation.open", id: conversationId })).ok, true);
    heldFetches[0]();
    await new Promise((resolve) => setTimeout(resolve, 20));
    assert.equal(messages().find((message) => message.id === 120).content, "updated authoritative content");
  });
});

test("an insertion between refresh pages does not evict the loaded oldest message", async () => {
  await withWorkspace(async ({ workspace, fetches, messages, emit, editRows, afterFetch }) => {
    for (let page = 0; page < 5; page += 1) {
      await workspace.dispatch({ type: "conversation.loadOlder" });
    }
    const loadedIds = messages().map((message) => message.id);
    let inserted = false;
    afterFetch(({ offset, limit }) => {
      if (inserted || offset !== 0 || limit !== 200) return;
      inserted = true;
      editRows((rows) => [...rows, {
        ...rows.at(-1), id: 301, client_message_id: "message-301", timestamp: 301,
      }]);
    });
    emit("transfer-changed", { transfer_id: 1, status: "downloading" });
    await waitFor(() => fetches.length >= 8, "the background refresh must fetch both pages");
    await new Promise((resolve) => setTimeout(resolve, 20));
    const remainingIds = new Set(messages().map((message) => message.id));
    for (const id of loadedIds) {
      assert.ok(remainingIds.has(id), `loaded message ${id} must survive a shifted refresh page`);
    }
    assert.ok(remainingIds.has(301));
  }, 300);
});

test("repeated page overlap defers replacement after one retry", async () => {
  await withWorkspace(async ({ workspace, fetches, messages, emit, editRows, afterFetch }) => {
    for (let page = 0; page < 5; page += 1) {
      await workspace.dispatch({ type: "conversation.loadOlder" });
    }
    const loadedIds = messages().map((message) => message.id);
    let inserted = 0;
    afterFetch(({ offset, limit }) => {
      if (offset !== 0 || limit !== 200) return;
      inserted += 1;
      editRows((rows) => [...rows, {
        ...rows.at(-1), id: 300 + inserted,
        client_message_id: `message-${300 + inserted}`, timestamp: 300 + inserted,
      }]);
    });
    emit("transfer-changed", { transfer_id: 1, status: "downloading" });
    await waitFor(() => fetches.length >= 8, "the background refresh must fetch both pages");
    await new Promise((resolve) => setTimeout(resolve, 20));
    assert.deepEqual(messages().map((message) => message.id), loadedIds);
    assert.equal(inserted, 2, "an inconsistent refresh must retry only once");
    assert.equal(fetches.length, 10);
  }, 300);
});

test("an incoming message during a refresh keeps the older loaded boundary", async () => {
  await withWorkspace(async ({ workspace, fetches, messages, emit, holdNext, heldFetches, editRows }) => {
    await workspace.dispatch({ type: "conversation.loadOlder" });
    holdNext();
    emit("transfer-changed", { transfer_id: 1, status: "downloading" });
    await waitFor(() => heldFetches.length === 1, "background refresh must be pending");
    const incoming = {
      id: 121, client_message_id: "message-121", conversation_id: conversationId,
      sender_id: "peer", content: "new incoming message", msg_type: "text",
      status: "received", timestamp: 121,
    };
    editRows((rows) => [...rows, incoming]);
    emit("new-message", incoming);
    assert.equal(messages().length, 81);
    heldFetches[0]();
    await waitFor(() => fetches.length >= 4 || messages().length !== 81, "pending refresh must settle");
    assert.equal(messages().length, 81);
    assert.ok(messages().some((message) => message.id === 41));
    assert.ok(messages().some((message) => message.id === 121));
  });
});

test("repeating an explicit message jump advances its sequence while transfer refresh does not", async () => {
  await withWorkspace(async ({ workspace, fetches, emit }) => {
    assert.equal(workspace.getSnapshot().focusedMessageSequence, 0);
    const jump = { type: "conversation.open", id: conversationId, targetClientMessageId: "message-120" };
    assert.equal((await workspace.dispatch(jump)).ok, true);
    assert.equal(workspace.getSnapshot().focusedMessageSequence, 1);
    assert.equal((await workspace.dispatch(jump)).ok, true);
    assert.equal(workspace.getSnapshot().focusedMessageSequence, 2);
    assert.equal(workspace.getSnapshot().focusedMessageId, "message-120");
    emit("transfer-changed", { transfer_id: 1, status: "downloading" });
    await waitFor(() => fetches.length === 4, "transfer update must reload the messages");
    assert.equal(workspace.getSnapshot().focusedMessageSequence, 2);
    assert.equal((await workspace.dispatch({ type: "conversation.open", id: conversationId })).ok, true);
    assert.equal(workspace.getSnapshot().focusedMessageId, null);
    assert.equal(workspace.getSnapshot().focusedMessageSequence, 2);
  });
});
