import assert from "node:assert/strict";
import test from "node:test";
import {
  directConversationId,
  EMOJI_SET,
  fileKind,
  fileStatus,
  insertTextAtSelection,
  isPhysicalPointInsideRect,
  localFileAvailable,
  matchesShortcut,
  measureTransfers,
  mergeMessages,
  normalizeMessage,
  runtimeCapabilities,
  shortcutLabelFromEvent,
} from "./xchat.js";

test("direct conversation IDs are stable on both peers", () => {
  assert.equal(
    directConversationId("peer-b", "peer-a"),
    directConversationId("peer-a", "peer-b"),
  );
});

test("cross-platform capture shortcut accepts either primary modifier", () => {
  const base = { key: "A", shiftKey: true, altKey: false };
  assert.equal(
    matchesShortcut({ ...base, metaKey: true, ctrlKey: false }, "Ctrl/⌘ ⇧ A"),
    true,
  );
  assert.equal(
    matchesShortcut({ ...base, metaKey: false, ctrlKey: true }, "Ctrl/⌘ ⇧ A"),
    true,
  );
  assert.equal(
    matchesShortcut({ ...base, metaKey: false, ctrlKey: false }, "Ctrl/⌘ ⇧ A"),
    false,
  );
  assert.equal(
    matchesShortcut(
      { ...base, metaKey: true, ctrlKey: false },
      "CommandOrControl+Shift+A",
    ),
    true,
  );
});

test("shortcut capture stores the actual modifier combination", () => {
  assert.equal(
    shortcutLabelFromEvent({
      key: "a",
      metaKey: true,
      ctrlKey: false,
      altKey: false,
      shiftKey: true,
    }),
    "⌘ ⇧ A",
  );
  assert.equal(shortcutLabelFromEvent({ key: "Shift", shiftKey: true }), "");
});

test("message merge replaces optimistic rows and never regresses receipts", () => {
  const pending = normalizeMessage(
    {
      client_message_id: "client-1",
      sender_id: "self",
      content: "hello",
      timestamp: 10,
      status: "pending",
    },
    "self",
    "conversation-1",
  );
  const read = { ...pending, id: 42, status: "read", read_count: 2 };
  const stale = { ...pending, id: 42, status: "sent", read_count: 0 };
  const merged = mergeMessages(mergeMessages([pending], [read]), [stale]);

  assert.equal(merged.length, 1);
  assert.equal(merged[0].id, 42);
  assert.equal(merged[0].status, "read");
  assert.equal(merged[0].read_count, 2);
});

test("web-only APIs are decided by the browser, not server platform flags", () => {
  const navigatorDescriptor = Object.getOwnPropertyDescriptor(globalThis, "navigator");
  const notificationDescriptor = Object.getOwnPropertyDescriptor(globalThis, "Notification");
  try {
    Object.defineProperty(globalThis, "navigator", {
      configurable: true,
      value: { mediaDevices: { getDisplayMedia() {} } },
    });
    Object.defineProperty(globalThis, "Notification", {
      configurable: true,
      value: class Notification {},
    });

    const capabilities = runtimeCapabilities(
      "web",
      { capture: false, captureShortcut: false, notifications: false },
      false,
    );

    assert.equal(capabilities.capture, true);
    assert.equal(capabilities.captureShortcut, true);
    assert.equal(capabilities.notifications, true);
    assert.equal(capabilities.revealFile, false);
    assert.equal(capabilities.nativeFilePicker, false);
  } finally {
    if (navigatorDescriptor) {
      Object.defineProperty(globalThis, "navigator", navigatorDescriptor);
    } else {
      delete globalThis.navigator;
    }
    if (notificationDescriptor) {
      Object.defineProperty(globalThis, "Notification", notificationDescriptor);
    } else {
      delete globalThis.Notification;
    }
  }
});

test("file status prefers transfer state and keeps completed outgoing files usable", () => {
  assert.equal(fileStatus({ status: "received", file_status: "accepted" }), "accepted");
  assert.equal(fileStatus({ status: "completed", file_status: "" }), "completed");
});

test("file classification and availability use backend metadata when present", () => {
  assert.equal(fileKind({ file_name: "capture.PNG" }), "image");
  assert.equal(fileKind({ file_name: "notes.md" }), "document");
  assert.equal(fileKind({ mime_type: "video/mp4" }), "video");
  assert.equal(localFileAvailable({ file_status: "completed", local_available: false }), false);
  assert.equal(localFileAvailable({ file_status: "removed" }), false);
});

test("emoji picker has a broad unique set and inserts at the current selection", () => {
  assert.ok(EMOJI_SET.length >= 80);
  assert.equal(new Set(EMOJI_SET).size, EMOJI_SET.length);
  assert.deepEqual(insertTextAtSelection("你好世界", "😀", 2, 4), {
    value: "你好😀",
    caret: 4,
  });
});

test("physical Tauri drag coordinates are matched against the CSS composer rect", () => {
  const rect = { left: 100, right: 300, top: 100, bottom: 200 };
  assert.equal(isPhysicalPointInsideRect({ x: 400, y: 300 }, rect, 2), true);
  assert.equal(isPhysicalPointInsideRect({ x: 40, y: 300 }, rect, 2), false);
});

test("transfer snapshots derive percentage and monotonic speed from byte deltas", () => {
  const previous = [
    {
      id: "transfer-1",
      status: "transferring",
      bytes_transferred: 1024,
      bytes_total: 4096,
    },
  ];
  const [active] = measureTransfers(previous, [
    { ...previous[0], bytes_transferred: 3072 },
  ], 1000);
  assert.equal(active.progress_percent, 75);
  assert.equal(active.speed_bps, 2048);

  const [completed] = measureTransfers([active], [
    { ...active, status: "completed", bytes_transferred: 4096 },
  ], 500);
  assert.equal(completed.progress_percent, 100);
  assert.equal(completed.speed_bps, 0);
});
