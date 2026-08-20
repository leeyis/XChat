import assert from "node:assert/strict";
import test from "node:test";
import {
  avatarText,
  createXChatModule,
  conversationPreview,
  directConversationId,
  discoverySettingsEqual,
  discoveryInterfaceState,
  discoverySummary,
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
  applyReactionUpdate,
  canSaveVerifiedEndpoint,
  localFileAvailable,
  markConversationReadState,
  matchesShortcut,
  measureTransfers,
  mergeMessages,
  messageDeliveryStatus,
  messageTimeDividerIndices,
  formatMessageTime,
  mentionQueryAtCaret,
  mentionToken,
  nativeClipboardPaths,
  nativeDragDropTarget,
  retainInFlightMessages,
  nativeCaptureShortcutAvailable,
  normalizeCustomPeer,
  normalizeConversation,
  normalizeDraftAttachment,
  normalizeMessage,
  normalizeSettings,
  recommendedDiscoverySettings,
  numericMessageId,
  retainedMentionIds,
  runtimeCapabilities,
  settingsFormDirty,
  settingsPatch,
  shortcutLabelFromEvent,
  TauriAdapter,
  validServerPort,
  withDiscoveryInterfaceSelection,
} from "./xchat.js";

test("fixed addresses stay inactive until the current input passes an identity test", () => {
  assert.deepEqual(normalizeCustomPeer("192.168.10.22:8888"), {
    endpoint: "192.168.10.22:8888",
    device_id: null,
    name: null,
    hostname: null,
    mac_address: null,
    app_version: null,
    last_verified_at: null,
    verified: false,
  });
  assert.equal(
    canSaveVerifiedEndpoint("192.168.10.22", "192.168.10.22", {
      identity_matches: true,
      identity: { device_id: "device-zhangsan" },
    }),
    true,
  );
  assert.equal(
    canSaveVerifiedEndpoint("192.168.10.111", "192.168.10.22", {
      identity_matches: true,
      identity: { device_id: "device-zhangsan" },
    }),
    false,
  );
  assert.equal(
    canSaveVerifiedEndpoint("192.168.10.22", "192.168.10.22", {
      identity_matches: false,
      identity: { device_id: "device-lisi" },
    }),
    false,
  );
});

test("desktop and web fixed-address adapters test and save the bound device identity", async () => {
  const tauriCalls = [];
  const tauri = new TauriAdapter({
    core: {
      invoke(command, payload) {
        tauriCalls.push([command, payload]);
        return Promise.resolve();
      },
    },
  });
  await tauri.testEndpoint("192.168.10.22", null);
  await tauri.addEndpoint("192.168.10.22:8888", "device-zhangsan");
  assert.deepEqual(tauriCalls, [
    ["test_custom_peer", { peer: "192.168.10.22", expectedDeviceId: null }],
    [
      "add_custom_peer",
      { peer: "192.168.10.22:8888", expectedDeviceId: "device-zhangsan" },
    ],
  ]);

  const webCalls = [];
  const web = new HttpWsAdapter();
  web.json = (...args) => {
    webCalls.push(args);
    return Promise.resolve();
  };
  await web.testEndpoint("192.168.10.22", null);
  await web.addEndpoint("192.168.10.22:8888", "device-zhangsan");
  assert.deepEqual(webCalls, [
    [
      "/api/test_custom_peer",
      "POST",
      { peer: "192.168.10.22", expected_device_id: null },
    ],
    [
      "/api/add_custom_peer",
      "POST",
      { peer: "192.168.10.22:8888", expected_device_id: "device-zhangsan" },
    ],
  ]);
});

test("server port validation accepts only integer ports from 1 through 65535", () => {
  for (const value of [1, "1", 8888, "65535"]) assert.equal(validServerPort(value), true);
  for (const value of ["", "0", 0, 65536, "1.5", "1e3", "+1", " 1", "abc", null]) {
    assert.equal(validServerPort(value), false, String(value));
  }
});

test("parallel channel settings normalize to the supported 4, 8, and 16 values", () => {
  for (const value of [undefined, null, 0, 12, 32, "8", "invalid"]) {
    assert.equal(
      normalizeSettings({ max_parallel_channels: value }).max_parallel_channels,
      4,
      String(value),
    );
  }
  for (const value of [4, 8, 16]) {
    assert.equal(
      normalizeSettings({ max_parallel_channels: value }).max_parallel_channels,
      value,
    );
  }
});

test("parallel channel changes participate in settings dirty, save, and reset state", () => {
  const baseline = normalizeSettings({ max_parallel_channels: 4 });
  const changed = { ...baseline, max_parallel_channels: 16 };

  assert.equal(settingsFormDirty(baseline, baseline), false);
  assert.equal(settingsFormDirty(changed, baseline), true);
  assert.deepEqual(settingsPatch(changed, baseline), { max_parallel_channels: 16 });

  const saved = normalizeSettings({ ...baseline, ...settingsPatch(changed, baseline) });
  assert.equal(settingsFormDirty(changed, saved), false);
  assert.equal(settingsFormDirty(baseline, baseline), false);
});

test("workspace settings normalize discovery defaults and backend interface facts", async () => {
  const windowDescriptor = Object.getOwnPropertyDescriptor(globalThis, "window");
  let workspace;
  try {
    Object.defineProperty(globalThis, "window", {
      configurable: true,
      value: {
        __TAURI__: {
          core: {
            invoke(command) {
              if (command !== "get_workspace_snapshot") return Promise.resolve([]);
              return Promise.resolve({
                self: { id: "self", name: "Me", hostname: "mac", addr: "192.168.10.20" },
                devices: [],
                conversations: [],
                files: [],
                transfers: [],
                settings: {
                  language: "zh-CN",
                  network_interfaces: [
                    {
                      id: "if:name:en0",
                      name: "en0",
                      index: 7,
                      addresses: [{ ipv4: "192.168.10.20", prefix_length: 24 }],
                      category: "physical_lan",
                      is_up: true,
                      default_enabled: true,
                      selected: true,
                      enabled: true,
                      exclusion_reason: null,
                    },
                  ],
                },
                capabilities: {},
              });
            },
          },
          event: { listen: () => Promise.resolve(() => {}) },
        },
      },
    });

    workspace = createXChatModule();
    assert.equal((await workspace.dispatch({ type: "bootstrap" })).ok, true);
    const settings = workspace.getSnapshot().settings;
    assert.deepEqual(settings.discovery_settings, {
      local_discovery: true,
      vpn_discovery: true,
      interface_overrides: {},
    });
    assert.deepEqual(settings.network_interfaces[0].addresses, [
      { ipv4: "192.168.10.20", prefix_length: 24 },
    ]);
    assert.equal(settings.network_interfaces[0].category, "physical_lan");
  } finally {
    await workspace?.dispatch({ type: "shutdown" });
    if (windowDescriptor) {
      Object.defineProperty(globalThis, "window", windowDescriptor);
    } else {
      delete globalThis.window;
    }
  }
});

test("desktop and web settings adapters send the same discovery and channel selection", async () => {
  const discoverySettings = {
    local_discovery: false,
    vpn_discovery: true,
    interface_overrides: { "if:name:en0": false, "if:name:meta tunnel": true },
  };
  const current = {
    download_path: "/tmp/downloads",
    port: "8888",
    db_path: "/tmp/xchat",
    auto_download: false,
    max_parallel_channels: 4,
    discovery_settings: recommendedDiscoverySettings(),
  };

  const tauriCalls = [];
  const tauri = new TauriAdapter({
    core: {
      invoke(command, payload) {
        tauriCalls.push([command, payload]);
        return Promise.resolve();
      },
    },
  });
  await tauri.patchSettings(
    { discovery_settings: discoverySettings, max_parallel_channels: 16 },
    current,
  );
  assert.deepEqual(tauriCalls, [[
    "update_settings",
    {
      downloadPath: "/tmp/downloads",
      port: "8888",
      dbPath: "/tmp/xchat",
      autoDownload: false,
      maxParallelChannels: 16,
      discoverySettings,
    },
  ]]);

  const webCalls = [];
  const web = new HttpWsAdapter();
  web.json = (...args) => {
    webCalls.push(args);
    return Promise.resolve();
  };
  await web.patchSettings(
    { discovery_settings: discoverySettings, max_parallel_channels: 16 },
    current,
  );
  assert.deepEqual(webCalls, [[
    "/api/update_settings",
    "POST",
    {
      download_path: "/tmp/downloads",
      port: 8888,
      db_path: "/tmp/xchat",
      auto_download: false,
      max_parallel_channels: 16,
      discovery_settings: discoverySettings,
    },
  ]]);
});

test("discovery interface state preserves overrides behind category master switches", () => {
  const physical = {
    id: "if:name:en0",
    category: "physical_lan",
    is_up: true,
    default_enabled: true,
  };
  const proxy = {
    id: "if:name:meta tunnel",
    category: "proxy_tun",
    is_up: true,
    default_enabled: false,
  };
  const settings = {
    local_discovery: false,
    vpn_discovery: true,
    interface_overrides: { "if:name:en0": true, "if:name:meta tunnel": true },
  };

  assert.deepEqual(
    discoveryInterfaceState(physical, settings),
    { selected: true, enabled: false, category_disabled: true },
  );
  assert.deepEqual(
    discoveryInterfaceState(proxy, settings),
    { selected: true, enabled: true, category_disabled: false },
  );
  assert.deepEqual(recommendedDiscoverySettings(), {
    local_discovery: true,
    vpn_discovery: true,
    interface_overrides: {},
  });
  assert.deepEqual(discoverySummary([physical, proxy], settings), {
    enabled: 1,
    paused: 1,
    excluded: 0,
    all_off: false,
  });

  const defaults = recommendedDiscoverySettings();
  const physicalOff = withDiscoveryInterfaceSelection(defaults, physical, false);
  assert.deepEqual(physicalOff.interface_overrides, { "if:name:en0": false });
  assert.deepEqual(
    withDiscoveryInterfaceSelection(physicalOff, physical, true),
    defaults,
  );
  const proxyOn = withDiscoveryInterfaceSelection(defaults, proxy, true);
  assert.deepEqual(proxyOn.interface_overrides, { "if:name:meta tunnel": true });
  assert.deepEqual(withDiscoveryInterfaceSelection(proxyOn, proxy, false), defaults);

  const reordered = withDiscoveryInterfaceSelection(
    withDiscoveryInterfaceSelection(
      {
        ...defaults,
        interface_overrides: {
          [physical.id]: false,
          [proxy.id]: true,
        },
      },
      physical,
      true,
    ),
    physical,
    false,
  );
  assert.equal(
    discoverySettingsEqual(
      reordered,
      {
        ...defaults,
        interface_overrides: {
          [physical.id]: false,
          [proxy.id]: true,
        },
      },
      [physical, proxy],
    ),
    true,
  );
  assert.equal(
    discoverySettingsEqual(
      { ...defaults, interface_overrides: { [physical.id]: true } },
      defaults,
      [physical, proxy],
    ),
    true,
  );
});

test("message reactions are idempotent and toggle per user", () => {
  const messages = [{ client_message_id: "message-1", reactions: [] }];
  const active = {
    conversation_id: "direct:a:b",
    client_message_id: "message-1",
    from_id: "peer-a",
    emoji: "👍",
    active: true,
  };

  const once = applyReactionUpdate(messages, active);
  const duplicate = applyReactionUpdate(once, active);
  assert.deepEqual(duplicate[0].reactions, [{ from_id: "peer-a", emoji: "👍" }]);

  const removed = applyReactionUpdate(duplicate, { ...active, active: false });
  assert.deepEqual(removed[0].reactions, []);

  const changed = applyReactionUpdate(once, { ...active, emoji: "😂" });
  assert.deepEqual(changed[0].reactions, [{ from_id: "peer-a", emoji: "😂" }]);
});

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

test("first-page reload keeps unacknowledged local messages visible", () => {
  const optimistic = normalizeMessage(
    {
      client_message_id: "client-1",
      sender_id: "self",
      content: "刚发出的消息",
      timestamp: 20,
      status: "pending",
    },
    "self",
    "conversation-1",
  );
  const persisted = normalizeMessage(
    { id: 7, client_message_id: "client-0", sender_id: "self", content: "旧消息", timestamp: 10 },
    "self",
    "conversation-1",
  );

  const retained = retainInFlightMessages([persisted, optimistic], [persisted]);
  assert.deepEqual(
    retained.map((message) => message.client_message_id),
    ["client-1"],
  );

  const reloaded = mergeMessages(retained, [persisted]);
  assert.deepEqual(
    reloaded.map((message) => message.client_message_id),
    ["client-0", "client-1"],
  );
});

test("first-page reload drops local rows the backend has confirmed or removed", () => {
  const acknowledged = normalizeMessage(
    { id: 7, client_message_id: "client-1", sender_id: "self", content: "已确认", timestamp: 10 },
    "self",
    "conversation-1",
  );
  const recalledLocally = normalizeMessage(
    { id: 8, client_message_id: "client-2", sender_id: "self", content: "被撤回", timestamp: 11 },
    "self",
    "conversation-1",
  );
  const fromPeer = normalizeMessage(
    { id: 9, client_message_id: "client-3", sender_id: "peer", content: "对方消息", timestamp: 12 },
    "self",
    "conversation-1",
  );

  // 后端这一页只剩 acknowledged：已送达的本机消息、被撤回的消息、对方消息都不应被保留
  assert.deepEqual(
    retainInFlightMessages([acknowledged, recalledLocally, fromPeer], [acknowledged]),
    [],
  );
});

test("failed sends survive a first-page reload so retry stays reachable", () => {
  const failed = normalizeMessage(
    {
      client_message_id: "client-9",
      sender_id: "self",
      content: "发送失败",
      timestamp: 30,
      status: "failed",
    },
    "self",
    "conversation-1",
  );

  assert.deepEqual(retainInFlightMessages([failed], []), [failed]);
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

test("incoming files stay unavailable until a local path is published", () => {
  const downloading = normalizeMessage(
    {
      id: 9,
      msg_type: "file",
      status: "received",
      file_status: "downloading",
      content: "incoming.png",
    },
    "self",
    "direct:test",
  );

  assert.equal(localFileAvailable(downloading), false);
  assert.equal(
    localFileAvailable({
      direction: "incoming",
      file_status: "accepted",
      file_path: "/downloads/incoming.png",
    }),
    true,
  );
  assert.equal(
    localFileAvailable({
      direction: "outgoing",
      file_status: "waiting_peer",
      file_path: "C:\\Users\\Eason\\Pictures\\outgoing.png",
    }),
    true,
  );
});

test("waiting file transfer overrides a stale sent message status", () => {
  assert.equal(
    messageDeliveryStatus(
      {
        own: true,
        msg_type: "file",
        status: "sent",
        file_status: "waiting_peer",
      },
      true,
    ),
    "waiting_peer",
  );
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

test("marking a visible conversation read clears its badge and local message state", () => {
  const previous = {
    conversations: [
      { id: "conversation-1", unread_count: 2, forced_unread: true },
      { id: "conversation-2", unread_count: 3, forced_unread: false },
    ],
    messagesByConversation: {
      "conversation-1": [
        { client_message_id: "message-1", status: "received" },
        { client_message_id: "message-2", status: "received" },
      ],
    },
  };

  const next = markConversationReadState(previous, "conversation-1", ["message-1"]);

  assert.equal(next.conversations[0].unread_count, 0);
  assert.equal(next.conversations[0].forced_unread, false);
  assert.equal(next.conversations[1].unread_count, 3);
  assert.equal(next.messagesByConversation["conversation-1"][0].status, "read");
  assert.equal(next.messagesByConversation["conversation-1"][1].status, "received");
});

test("manual file acceptance converts persisted string IDs at the Tauri boundary", async () => {
  const calls = [];
  const adapter = new TauriAdapter({
    core: {
      invoke(command, payload) {
        calls.push([command, payload]);
      },
    },
  });

  await adapter.acceptFile({ id: 17, sender_msg_id: "17" });

  assert.deepEqual(calls, [["request_file", { messageId: 17, senderMsgId: 17 }]]);
  assert.equal(numericMessageId("17"), 17);
  assert.equal(numericMessageId("not-an-id"), null);
});

test("conversation previews decode quote payloads without exposing transport JSON", () => {
  const encoded = encodeQuoteMessage("是的", {
    client_message_id: "source-1",
    sender_name: "chenwei",
    content: "就一个文件吗",
  });

  assert.equal(conversationPreview(encoded), "是的");
  assert.equal(normalizeConversation({ id: "group-1", last_message: encoded }).last_message, "是的");
  assert.equal(conversationPreview("普通消息"), "普通消息");
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

test("a peer going offline raises a warning notice, not an error", async () => {
  const windowDescriptor = Object.getOwnPropertyDescriptor(globalThis, "window");
  const listeners = new Map();
  const alerts = [];
  let workspace;
  try {
    Object.defineProperty(globalThis, "window", {
      configurable: true,
      value: {
        __TAURI__: {
          core: {
            invoke(command) {
              if (command === "get_workspace_snapshot") {
                return Promise.resolve({
                  self: { id: "self", name: "Me", hostname: "pc", addr: "" },
                  devices: [],
                  conversations: [],
                  files: [],
                  transfers: [],
                  settings: { language: "zh-CN", notifications_enabled: true },
                  capabilities: { notifications: true },
                });
              }
              if (command === "show_alert") {
                alerts.push(command);
              }
              return Promise.resolve([]);
            },
          },
          event: {
            listen(name, handler) {
              listeners.set(name, handler);
              return Promise.resolve(() => {});
            },
          },
        },
      },
    });

    workspace = createXChatModule();
    assert.equal((await workspace.dispatch({ type: "bootstrap" })).ok, true);

    // 后端 emit 的事件名必须真的有人接：加进 EVENT_NAMES 但没写 handler
    // 就是「下线了却毫无提示」的原因。
    const handler = listeners.get("peer-offline");
    assert.ok(handler, "peer-offline 没有被订阅");

    handler({ payload: { id: "peer-1", name: "Alice", addr: "127.0.0.1:8888" } });

    const notice = workspace.getSnapshot().notices.at(-1);
    assert.match(notice.message, /Alice/);
    assert.match(notice.message, /下线/);
    // 下线是正常事件，不该按错误红点渲染
    assert.equal(notice.kind, "warning");
  } finally {
    await workspace?.dispatch({ type: "shutdown" });
    if (windowDescriptor) {
      Object.defineProperty(globalThis, "window", windowDescriptor);
    } else {
      delete globalThis.window;
    }
  }
});

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
