import assert from "node:assert/strict";
import test from "node:test";
import {
  avatarText,
  createXChatModule,
  directConversationId,
  encodeQuoteMessage,
  EMOJI_SET,
  fileKind,
  fileStatus,
  groupMentionCandidates,
  groupAvatarCells,
  groupAvatarRows,
  HttpWsAdapter,
  insertTextAtSelection,
  isAppActive,
  isCopyableMessage,
  isPhysicalPointInsideRect,
  isTextFile,
  incomingMessageAlert,
  isMessageAlertControlType,
  localFileAvailable,
  matchesShortcut,
  measureTransfers,
  mergeMessages,
  messageTimeDividerIndices,
  formatMessageTime,
  mentionQueryAtCaret,
  mentionToken,
  nativeClipboardPaths,
  nativeDragDropTarget,
  nativeCaptureShortcutAvailable,
  normalizeConversation,
  normalizeDraftAttachment,
  normalizeMessage,
  retainedMentionIds,
  runtimeCapabilities,
  shortcutLabelFromEvent,
  TauriAdapter,
} from "./xchat.js";

test("text avatars use the last two characters and groups use member tail characters", () => {
  assert.equal(avatarText("张三丰"), "三丰");
  assert.equal(avatarText("A"), "A");
  assert.equal(avatarText("👨‍👩‍👧‍👦家庭"), "家庭");
  assert.deepEqual(
    groupAvatarCells([
      { display_name: "张三" },
      { display_name: "李四" },
      { display_name: "Alice" },
      { display_name: "王五" },
    ]),
    ["三", "四", "e", "五"],
  );
  const members = Array.from({ length: 9 }, (_, index) => ({ name: `成员${index}` }));
  assert.deepEqual(groupAvatarRows(members.slice(0, 3)).map((row) => row.length), [1, 2]);
  assert.deepEqual(groupAvatarRows(members.slice(0, 5)).map((row) => row.length), [2, 3]);
  assert.deepEqual(groupAvatarRows(members.slice(0, 9)).map((row) => row.length), [3, 3, 3]);
});

test("message time dividers follow the five-minute WeChat-style cadence", () => {
  const messages = [0, 120, 299, 300, 599, 600, 901].map((timestamp, id) => ({ id, timestamp }));
  assert.deepEqual(messageTimeDividerIndices(messages), [0, 3, 5, 6]);
});

test("message time labels distinguish today, yesterday, weekdays, dates, and years", () => {
  const stamp = (year, month, day, hour, minute) =>
    Math.floor(new Date(year, month - 1, day, hour, minute).getTime() / 1000);
  const now = new Date(2026, 7, 11, 12, 0);

  assert.equal(formatMessageTime(stamp(2026, 8, 11, 9, 37), "zh-CN", now), "09:37");
  assert.equal(formatMessageTime(stamp(2026, 8, 10, 9, 37), "zh-CN", now), "昨天 09:37");
  assert.equal(formatMessageTime(stamp(2026, 8, 8, 9, 37), "zh-CN", now), "星期六 09:37");
  assert.equal(formatMessageTime(stamp(2026, 7, 20, 9, 37), "zh-CN", now), "7月20日 09:37");
  assert.equal(formatMessageTime(stamp(2025, 12, 31, 9, 37), "zh-CN", now), "2025年12月31日 09:37");
});

test("copyable message classification only allows text and image content", () => {
  assert.equal(isTextFile({ file_name: "notes.md" }), true);
  assert.equal(isTextFile({ mime_type: "text/plain", file_name: "notes" }), true);
  assert.equal(isTextFile({ file_name: "archive.zip", mime_type: "application/zip" }), false);
  assert.equal(isCopyableMessage({ msg_type: "text", content: "hello" }), true);
  assert.equal(isCopyableMessage({ msg_type: "quote", content: "hello" }), true);
  assert.equal(isCopyableMessage({ msg_type: "file", file_name: "notes.md" }), true);
  assert.equal(isCopyableMessage({ msg_type: "file", file_name: "photo.png" }), true);
  assert.equal(isCopyableMessage({ msg_type: "file", file_name: "manual.pdf" }), false);
  assert.equal(isCopyableMessage({ msg_type: "file", file_name: "archive.zip" }), false);
});

test("quote messages keep a structured jump target", () => {
  const content = encodeQuoteMessage("收到", {
    id: 42,
    client_message_id: "original-42",
    sender_id: "peer-a",
    sender_name: "Alice",
    content: "原消息",
    msg_type: "text",
  });
  const message = normalizeMessage({ msg_type: "quote", content }, "me", "group-1");
  assert.equal(message.content, "收到");
  assert.equal(message.quote.client_message_id, "original-42");
  assert.equal(message.quote.message_id, 42);
  assert.equal(message.quote.content, "原消息");
});

test("read receipts and attention clear only while the app is active", () => {
  assert.equal(isAppActive("visible", true), true);
  assert.equal(isAppActive("visible", false), false);
  assert.equal(isAppActive("hidden", true), false);
});

test("desktop capture shortcuts use the native global listener", () => {
  assert.equal(nativeCaptureShortcutAvailable({ core: { invoke() {} } }), true);
  assert.equal(nativeCaptureShortcutAvailable(undefined), false);
});

test("direct conversation IDs are stable on both peers", () => {
  assert.equal(
    directConversationId("peer-b", "peer-a"),
    directConversationId("peer-a", "peer-b"),
  );
});

test("conversation normalization attaches the matching peer presence", () => {
  const conversation = normalizeConversation(
    {
      id: "direct:peer-1:self",
      kind: "direct",
      peer_id: "peer-1",
      created_by: "self",
    },
    [{ id: "peer-1", name: "Alice", is_offline: true }],
  );

  assert.equal(conversation.peer?.id, "peer-1");
  assert.equal(conversation.peer?.is_offline, true);
  assert.equal(conversation.created_by, "self");
  assert.equal(
    normalizeConversation({ peer_id: "peer-2", peer: { id: "peer-2" } }).peer?.id,
    "peer-2",
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
  assert.equal(shortcutLabelFromEvent({ key: "a" }), "");
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

test("message normalization preserves mention target IDs", () => {
  assert.deepEqual(
    normalizeMessage({ sender_id: "peer-1", mentionIds: ["self-id"] }).mention_ids,
    ["self-id"],
  );
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

test("group alerts notify only mentioned recipients while direct alerts stay unchanged", () => {
  const groupMessage = {
    client_message_id: "group-message-1",
    wire_msg_type: "group_message",
    group_id: "group-1",
    from_id: "peer-1",
    from_name: "Alice",
    content: "hello group",
    mention_ids: ["other-id"],
  };
  assert.equal(incomingMessageAlert(groupMessage, "self-id"), null);
  assert.ok(
    incomingMessageAlert(
      { ...groupMessage, mention_ids: ["self-id"] },
      "self-id",
    ),
  );
  assert.equal(
    incomingMessageAlert(
      {
        from_id: "peer-1",
        msg_type: "file",
        file_name: "report.pdf",
        conversation_id: "group-1",
      },
      "self-id",
      "group",
    ),
    null,
  );
  assert.ok(
    incomingMessageAlert(
      {
        ...groupMessage,
        wire_msg_type: undefined,
        group_id: undefined,
        conversation_id: "direct:peer-1:self-id",
        mention_ids: [],
      },
      "self-id",
    ),
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

test("group mention candidates search display names and current IPs but not device IDs", () => {
  const conversation = {
    kind: "group",
    members: [
      { peer_id: "self-id", display_name: "Me" },
      { peer_id: "alice-long-device-id", display_name: "Alice" },
      { peer_id: "bob-long-device-id", display_name: "Bob" },
    ],
  };
  const devices = [
    { id: "alice-long-device-id", addr: "192.168.1.21" },
    { id: "bob-long-device-id", addr: "10.0.0.8" },
  ];

  assert.deepEqual(
    groupMentionCandidates(conversation, devices, "self-id").map(
      ({ peer_id }) => peer_id,
    ),
    ["alice-long-device-id", "bob-long-device-id"],
  );
  assert.deepEqual(
    groupMentionCandidates(conversation, devices, "self-id", "ali").map(
      ({ peer_id, display_name, addr }) => ({ peer_id, display_name, addr }),
    ),
    [{ peer_id: "alice-long-device-id", display_name: "Alice", addr: "192.168.1.21" }],
  );
  assert.deepEqual(
    groupMentionCandidates(conversation, devices, "self-id", "10.0.0").map(
      ({ peer_id }) => peer_id,
    ),
    ["bob-long-device-id"],
  );
  assert.deepEqual(
    groupMentionCandidates(conversation, devices, "self-id", "long-device-id"),
    [],
  );
});

test("mention query follows the active @ token at the textarea caret", () => {
  assert.deepEqual(mentionQueryAtCaret("hello @ali", 10), {
    start: 6,
    query: "ali",
  });
  assert.equal(mentionQueryAtCaret("email@example.com", 17), null);
  assert.deepEqual(mentionQueryAtCaret("你好，@张", 5), {
    start: 3,
    query: "张",
  });
  assert.equal(mentionQueryAtCaret("你好@张", 4), null);
  assert.equal(mentionQueryAtCaret("hello @ali done", 15), null);
});

test("mention targets survive conversation switches but deleted tokens are not sent", () => {
  const targets = [
    { conversation_id: "group-1", peer_id: "alice-id", token: "@Alice" },
    { conversation_id: "group-1", peer_id: "bob-id", token: "@Bob" },
    { conversation_id: "group-2", peer_id: "carol-id", token: "@Carol" },
  ];

  assert.deepEqual(retainedMentionIds("hello @Alice", targets, "group-1"), [
    "alice-id",
  ]);
  assert.deepEqual(retainedMentionIds("hello", targets, "group-1"), []);
  assert.deepEqual(retainedMentionIds("welcome back @Carol", targets, "group-2"), [
    "carol-id",
  ]);
  assert.deepEqual(
    retainedMentionIds(
      "hello @Anna",
      [{ conversation_id: "group-1", peer_id: "ann-id", token: "@Ann" }],
      "group-1",
    ),
    [],
  );
  const alexes = [
    { peer_id: "alex-1", display_name: "Alex", addr: "10.0.0.1" },
    { peer_id: "alex-2", display_name: "Alex", addr: "10.0.0.2" },
  ];
  const alexTargets = alexes.map((candidate) => ({
    conversation_id: "group-1",
    peer_id: candidate.peer_id,
    token: mentionToken(candidate, alexes),
  }));
  assert.deepEqual(
    alexTargets.map((target) => target.token),
    ["@Alex(10.0.0.1)", "@Alex(10.0.0.2)"],
  );
  assert.deepEqual(
    retainedMentionIds("only @Alex(10.0.0.2)", alexTargets, "group-1"),
    ["alex-2"],
  );
  const unnamed = [{ peer_id: "one" }, { peer_id: "two" }];
  assert.deepEqual(
    unnamed.map((candidate) => mentionToken(candidate, unnamed, "Unnamed")),
    ["@Unnamed#1", "@Unnamed#2"],
  );
});

test("native clipboard paths normalize Windows file URLs and UNC shares", () => {
  assert.deepEqual(
    nativeClipboardPaths(
      "file:///C:/Users/Alice/report.pdf\nfile://server/share/design.png\n/tmp/note.txt",
    ),
    ["C:/Users/Alice/report.pdf", "//server/share/design.png", "/tmp/note.txt"],
  );
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

test("pathless Tauri attachments fail without reopening the file picker", async () => {
  let pickerOpened = false;
  const adapter = new TauriAdapter({
    core: { invoke() {} },
    dialog: {
      async open() {
        pickerOpened = true;
        return "/Users/eason/Desktop/report.pdf";
      },
    },
  });

  await assert.rejects(
    adapter.sendFiles({ id: "conversation-1" }, [{ name: "report.pdf" }]),
    (error) => error.code === "file_unavailable",
  );
  assert.equal(pickerOpened, false);
});

test("Tauri clipboard file reads return native Finder paths", async () => {
  const calls = [];
  const adapter = new TauriAdapter({
    core: {
      invoke(command, payload) {
        calls.push([command, payload]);
        return ["/Users/eason/Desktop/report.pdf"];
      },
    },
  });

  assert.deepEqual(await adapter.readClipboardFiles(), [
    "/Users/eason/Desktop/report.pdf",
  ]);
  assert.deepEqual(calls, [["read_clipboard_files", undefined]]);
});

test("Tauri path selection and file-content copy use native commands", async () => {
  const calls = [];
  const adapter = new TauriAdapter({
    core: {
      invoke(command, payload) {
        calls.push([command, payload]);
        return command === "pick_workspace_directory" ? "C:\\XChat" : undefined;
      },
    },
  });

  assert.equal(await adapter.pickDirectory("选择文件夹"), "C:\\XChat");
  await adapter.copyFileMessage({ message_id: 7, file_name: "notes.md" });
  await adapter.copyFileMessage({ id: 8, file_name: "photo.png" });

  assert.deepEqual(calls, [
    ["pick_workspace_directory", { title: "选择文件夹" }],
    ["copy_file_message_content", { messageId: 7, kind: "text" }],
    ["copy_file_message_content", { messageId: 8, kind: "image" }],
  ]);
});

test("desktop and web adapters send the same group rename operation", async () => {
  const tauriCalls = [];
  const tauri = new TauriAdapter({
    core: {
      invoke(command, payload) {
        tauriCalls.push([command, payload]);
        return undefined;
      },
    },
  });
  await tauri.updateGroup("group-1", "rename", "产品设计组", []);
  assert.deepEqual(tauriCalls, [[
    "update_group",
    {
      conversationId: "group-1",
      operation: "rename",
      value: "产品设计组",
      memberIds: [],
    },
  ]]);

  const webCalls = [];
  const web = new HttpWsAdapter();
  web.json = (...args) => {
    webCalls.push(args);
    return undefined;
  };
  await web.updateGroup("group-1", "rename", "产品设计组", []);
  assert.deepEqual(webCalls, [[
    "/api/groups/group-1",
    "POST",
    {
      operation: "rename",
      value: "产品设计组",
      member_ids: [],
    },
  ]]);
});

test("Tauri capture copy forwards the edited PNG to the native command", async () => {
  const calls = [];
  const adapter = new TauriAdapter({
    core: {
      invoke(command, payload) {
        calls.push([command, payload]);
        return Promise.resolve();
      },
    },
  });
  const dataUrl = "data:image/png;base64,capture";

  await adapter.copyCapture(dataUrl);

  assert.deepEqual(calls, [["copy_capture_editor", { dataUrl }]]);
});

async function withTauriCaptureWorkspace(capture, run) {
  const windowDescriptor = Object.getOwnPropertyDescriptor(globalThis, "window");
  const calls = [];
  let workspace;
  try {
    Object.defineProperty(globalThis, "window", {
      configurable: true,
      value: {
        __TAURI__: {
          core: {
            invoke(command, payload) {
              calls.push([command, payload]);
              if (command === "get_workspace_snapshot") {
                return Promise.resolve({
                  self: { id: "self", name: "Me", hostname: "mac", addr: "" },
                  devices: [],
                  conversations: [],
                  files: [],
                  transfers: [],
                  settings: { language: "zh-CN", capture_shortcut: "⌘ ⇧ A" },
                  capabilities: { capture, captureShortcut: capture },
                });
              }
              if (command === "start_capture_editor") {
                return Promise.resolve({ session_id: "capture", conversation_id: null });
              }
              return Promise.resolve([]);
            },
          },
          event: { listen: () => Promise.resolve(() => {}) },
        },
      },
    });

    workspace = createXChatModule();
    assert.equal((await workspace.dispatch({ type: "bootstrap" })).ok, true);
    await run({ workspace, calls });
  } finally {
    await workspace?.dispatch({ type: "shutdown" });
    if (windowDescriptor) {
      Object.defineProperty(globalThis, "window", windowDescriptor);
    } else {
      delete globalThis.window;
    }
  }
}

test("Tauri capture starts without a discovered conversation", async () => {
  await withTauriCaptureWorkspace(true, async ({ workspace, calls }) => {
    const result = await workspace.dispatch({ type: "capture.start" });

    assert.equal(result.ok, true);
    assert.deepEqual(
      calls.find(([command]) => command === "start_capture_editor"),
      ["start_capture_editor", { conversationId: null }],
    );
  });
});

test("unsupported desktop capture never invokes the backend", async () => {
  await withTauriCaptureWorkspace(false, async ({ workspace, calls }) => {
    const result = await workspace.dispatch({ type: "capture.start" });

    assert.equal(result.ok, false);
    assert.equal(result.error.code, "capture_unsupported");
    assert.equal(
      calls.some(([command]) => command === "start_capture_editor"),
      false,
    );
  });
});

test("desktop and web message adapters send stable mention IDs", async () => {
  const tauriCalls = [];
  const tauri = new TauriAdapter({
    core: {
      invoke(command, payload) {
        tauriCalls.push([command, payload]);
        return {};
      },
    },
  });
  await tauri.sendMessage(
    { id: "group-1", kind: "group" },
    "message-1",
    "hello @Alice",
    "text",
    ["alice-id"],
  );
  assert.deepEqual(tauriCalls[0], [
    "send_conversation_message",
    {
      conversationId: "group-1",
      clientMessageId: "message-1",
      content: "hello @Alice",
      msgType: "text",
      mentionIds: ["alice-id"],
    },
  ]);

  const webCalls = [];
  const web = new HttpWsAdapter();
  web.json = (...args) => {
    webCalls.push(args);
    return {};
  };
  await web.sendMessage(
    { id: "group-1", kind: "group" },
    "message-1",
    "hello @Alice",
    "text",
    ["alice-id"],
  );
  assert.deepEqual(webCalls[0], [
    "/api/conversations/group-1/messages",
    "POST",
    {
      client_message_id: "message-1",
      content: "hello @Alice",
      msg_type: "text",
      mention_ids: ["alice-id"],
    },
  ]);
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
