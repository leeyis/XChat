import assert from "node:assert/strict";
import test from "node:test";
import {
  directConversationId,
  EMOJI_SET,
  fileKind,
  fileStatus,
  insertTextAtSelection,
  isAppActive,
  isPhysicalPointInsideRect,
  incomingMessageAlert,
  isMessageAlertControlType,
  localFileAvailable,
  matchesShortcut,
  measureTransfers,
  mergeMessages,
  nativeDragDropTarget,
  normalizeDraftAttachment,
  normalizeMessage,
  runtimeCapabilities,
  shortcutLabelFromEvent,
  TauriAdapter,
} from "./xchat.js";

test("read receipts and attention clear only while the app is active", () => {
  assert.equal(isAppActive("visible", true), true);
  assert.equal(isAppActive("visible", false), false);
  assert.equal(isAppActive("hidden", true), false);
});

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

test("incoming alerts accept remote messages and reject self/control events", () => {
  assert.deepEqual(
    incomingMessageAlert(
      {
        client_message_id: "message-1",
        from_id: "peer-1",
        from_name: "Alice",
        msg_type: "text",
        content: "hello",
      },
      "self",
    ),
    {
      key: "peer-1:message-1",
      fromId: "peer-1",
      title: "Alice",
      body: "hello",
    },
  );
  assert.equal(
    incomingMessageAlert(
      { from_id: "self", msg_type: "text", content: "own message" },
      "self",
    ),
    null,
  );
  assert.equal(
    incomingMessageAlert(
      {
        from_id: "peer-1",
        msg_type: "file_status_update",
        content: "progress",
      },
      "self",
    ),
    null,
  );
});

test("incoming file alerts use the remote file name and control filtering", () => {
  assert.equal(isMessageAlertControlType("file_download_progress"), true);
  assert.equal(isMessageAlertControlType("file"), false);
  assert.deepEqual(
    incomingMessageAlert(
      {
        id: 42,
        from_id: "peer-1",
        from_name: "Alice",
        msg_type: "file",
        file_name: "report.pdf",
        content: "",
      },
      "self",
    ),
    {
      key: "peer-1:42",
      fromId: "peer-1",
      title: "Alice",
      body: "收到文件：report.pdf",
    },
  );
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

test("desktop capture is available on every desktop platform but not Android", () => {
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, "navigator");
  const agents = {
    macos: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)",
    windows: "Mozilla/5.0 (Windows NT 10.0; Win64; x64)",
    linux: "Mozilla/5.0 (X11; Linux x86_64)",
    android: "Mozilla/5.0 (Linux; Android 14; Pixel 8)",
  };
  try {
    for (const [platform, userAgent] of Object.entries(agents)) {
      Object.defineProperty(globalThis, "navigator", {
        configurable: true,
        value: { userAgent },
      });
      const capabilities = runtimeCapabilities("tauri", {}, false);
      const expected = platform !== "android";
      assert.equal(capabilities.capture, expected, `capture on ${platform}`);
      assert.equal(capabilities.captureShortcut, expected, `shortcut on ${platform}`);
    }
  } finally {
    if (descriptor) {
      Object.defineProperty(globalThis, "navigator", descriptor);
    } else {
      delete globalThis.navigator;
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

test("native file drops fall back to the Tauri window API", () => {
  const target = { onDragDropEvent() {} };
  assert.equal(
    nativeDragDropTarget({
      webview: {},
      window: { getCurrentWindow: () => target },
    }),
    target,
  );
});

test("Finder drops survive fs metadata scope rejection", async () => {
  const adapter = new TauriAdapter({
    core: { invoke() {} },
    fs: {
      async stat() {
        throw new Error("forbidden path");
      },
    },
  });

  const result = await adapter.validateDroppedPaths(["/Users/eason/Desktop/report.pdf"]);

  assert.deepEqual(result.errors, []);
  assert.equal(result.files[0].file_path, "/Users/eason/Desktop/report.pdf");
});

test("draft attachment normalization accepts browser files and native paths", () => {
  const browserFile = { name: "notes.txt", size: 12, type: "text/plain" };
  const browser = normalizeDraftAttachment(browserFile);
  assert.equal(browser.file_name, "notes.txt");
  assert.equal(browser.file_size, 12);
  const native = normalizeDraftAttachment("/tmp/archive.zip");
  assert.equal(native.file_path, "/tmp/archive.zip");
  assert.equal(native.file_name, "archive.zip");
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
