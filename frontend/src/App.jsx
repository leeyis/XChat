import {
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import {
  fileKind,
  fileStatus,
  isImageFile,
  localFileAvailable,
  matchesShortcut,
  shortcutLabelFromEvent,
} from "./xchat.js";
import CaptureEditor from "./CaptureEditor.jsx";

const EMOJI_SET = [
  "😀", "😄", "😂", "🥰", "😎", "🤝", "👍",
  "👏", "🎉", "❤️", "😮", "😢", "😡", "🤔",
  "🙏", "💪", "✅", "📎", "💻", "📁", "🚀",
];

const FILE_KINDS = ["all", "image", "document", "audio", "video", "other"];

const ACTIVE_TRANSFER_STATES = new Set([
  "queued",
  "waiting_peer",
  "offering",
  "awaiting_acceptance",
  "transferring",
  "cancelling",
]);

const copy = {
  "zh-CN": {
    locale: "zh-CN",
    chat: "聊天",
    hosts: "主机",
    files: "文件",
    settings: "设置",
    searchChat: "搜索会话或消息",
    searchHosts: "搜索名称、地址或设备 ID",
    searchFiles: "搜索文件或来源",
    noConversation: "还没有会话",
    noConversationHint: "发现设备后，可从主机页开始聊天。",
    noSearchResults: "没有搜索结果",
    noSearchResultsHint: "换个关键词再试。",
    noMessages: "暂无消息",
    noMessagesHint: "发送一条消息开始对话。",
    send: "发送",
    online: "在线",
    offline: "离线",
    unnamedDevice: "未命名设备",
    groupAvatar: "群",
    myDevice: "我的设备",
    groupChat: "群聊",
    pinned: "置顶",
    draftPrefix: "[草稿] ",
    remarked: "已备注",
    manual: "手动",
    unknownHostname: "主机名未知",
    unknownAddress: "地址未知",
    fileFilters: {
      all: "全部文件",
      outgoing: "我发送的",
      incoming: "我接收的",
      active: "进行中",
      failed: "失败",
    },
    fileKinds: {
      all: "全部",
      image: "图片",
      document: "文档",
      audio: "音频",
      video: "视频",
      other: "其他",
    },
    groupSources: "群聊",
    peerSources: "设备与联系人",
    allFileSources: "所有设备与群聊",
    sourceGroup: "群聊",
    sourceDevice: "设备",
    newGroup: "新建群聊",
    addDevice: "手动添加设备",
    closeList: "关闭列表",
    messageSearch: "消息搜索",
    chatHistory: "聊天记录",
    noDevices: "没有发现设备",
    noDevicesHint: "自动发现的主机会显示在这里。",
    allConversations: "所有会话",
    filterByStatus: "按状态筛选",
    settingsSections: {
      identity: "身份",
      appearance: "外观",
      notification: "通知",
      download: "下载与传输",
      network: "网络",
      shortcut: "快捷键",
    },
    attachment: "附件",
    emoji: "表情",
    removeAttachment: "移除附件",
    attachmentReady: "待发送",
    imagePreview: "图片预览",
    localFileUnavailable: "本地文件不可用",
    previewFile: "预览文件",
    sentDirection: "我发送的",
    receivedDirection: "我接收的",
    file: "文件",
    receive: "接收",
    open: "打开",
    messagePlaceholder: "输入消息…",
    message: "消息",
    capture: "截屏",
    captureUnsupported: "当前平台不支持截屏",
    sendFile: "发送文件",
    memberCount: (count) => `${count} 位成员`,
    backConversationList: "返回会话列表",
    conversationInfo: "会话信息",
    clearHistoryTitle: "清空聊天记录？",
    clearHistoryDetail: "这只会删除本机聊天记录，无法撤销。",
    clearHistoryShortDetail: "这只会删除本机记录，无法撤销。",
    clearHistoryAction: "确认清空",
    clearHistory: "清空聊天记录",
    loadOlder: "加载更早消息",
    deleteMessageTitle: "删除这条本地消息？",
    deleteMessageDetail: "只会删除这台设备保存的副本，无法撤销。",
    deleteMessageAction: "删除消息",
    deleteLocalMessage: "删除本地消息",
    delete: "删除",
    activeTransfers: (count) => `${count} 项传输进行中`,
    openFileCenterProgress: "打开文件中心查看进度",
    selectHost: "选择一台主机",
    selectHostHint: "查看发现来源、地址和稳定设备身份。",
    backHostList: "返回主机列表",
    unknownDiscovery: "发现方式未知",
    sendMessage: "发消息",
    editRemark: "修改备注",
    deviceIdentity: "设备身份",
    hostname: "主机名",
    currentAddress: "当前地址",
    macAddress: "MAC 地址",
    deviceId: "设备 ID",
    discoveryMethod: "发现方式",
    lastOnline: "最后在线",
    availableMemory: "可用内存",
    notProvided: "未提供",
    unknown: "未知",
    deleteLocalContact: "删除本地联系人",
    deleteLocalContactHint: "设备再次上线时仍可重新发现。",
    deleteDeviceTitle: (name) => `删除「${name}」？`,
    deleteDeviceDetail: "聊天记录和本地联系人信息会被删除。",
    deleteDeviceAction: "确认删除",
    deleteDevice: "删除设备",
    receiveAgain: "重新接收",
    retry: "重试",
    openFile: "打开文件",
    revealFile: "打开所在目录",
    deleteLocalCopy: "删除本地副本",
    backFileFilters: "返回文件筛选",
    fileCenter: "文件中心",
    fileSummary: (files, transfers) => `${files} 个文件 · ${transfers} 个传输记录`,
    refresh: "刷新",
    transferStatus: "传输状态",
    transferName: (id) => `传输 ${id}`,
    progress: (value) => `进度 ${value}%`,
    cancelling: "正在取消",
    cancel: "取消",
    sourceTarget: "来源 / 目标",
    time: "时间",
    size: "大小",
    actions: "动作",
    unnamedFile: "未命名文件",
    unknownStatus: "未知状态",
    thisDevice: "本机",
    deleteFileTitle: (name) => `删除「${name}」？`,
    deleteFileDetail: "只删除本机下载副本，聊天记录仍会保留。",
    deleteFileAction: "删除文件",
    noMatchingFiles: "没有符合条件的文件",
    noMatchingFilesHint: "调整来源、状态或搜索条件后再试。",
    noFiles: "还没有文件",
    noFilesHint: "会话中发送或接收的文件会显示在这里。",
    previewUnavailable: "此类型暂不支持在线预览，可以打开所在文件夹。",
    documentPreview: "文档预览",
    audioPreview: "音频预览",
    videoPreview: "视频预览",
    backSettingsList: "返回设置列表",
    settingsSubtitle: "本机偏好与网络参数",
    saveSettings: "保存设置",
    identity: "身份",
    localName: "本机名称",
    deviceIdDetail: (id) => `设备 ID ${id}`,
    deviceIdUnavailable: "设备 ID 暂不可用",
    localAvatar: "本机头像",
    localAvatarHint: "仅保存在这台设备",
    chooseAvatar: (avatar) => `选择头像 ${avatar}`,
    appearance: "外观",
    theme: "主题",
    systemTheme: "跟随系统",
    lightTheme: "浅色",
    darkTheme: "深色",
    language: "语言",
    simplifiedChinese: "简体中文",
    english: "English",
    notification: "通知",
    newMessageNotification: "新消息通知",
    permissionManagedBySystem: "权限由系统管理",
    platformUnavailable: "当前平台不可用",
    downloadsAndTransfers: "下载与传输",
    downloadPath: "下载路径",
    autoReceiveFiles: "自动接收文件",
    autoReceiveFilesHint: "关闭后文件停在待接收状态",
    network: "网络",
    serverPort: "服务端口",
    restartRequired: "重启后生效",
    databasePath: "数据库路径",
    shortcuts: "快捷键",
    captureShortcut: "截屏快捷键",
    captureShortcutHint: "点击输入框后按下字母或数字组合键",
    captureShortcutFocusedHint: "仅在 Xchat 窗口聚焦时生效",
    groupMembers: (count) => `群成员 · ${count}`,
    editDeviceRemark: "修改设备备注",
    unpinConversation: "取消置顶",
    pinConversation: "置顶会话",
    unmarkUnread: "取消标记未读",
    markUnread: "标记未读",
    close: "关闭",
    cancelAction: "取消",
    createGroup: "创建群聊",
    groupName: "群聊名称",
    groupHelper: "至少选择两台支持群聊的远端设备。",
    deviceAddress: "设备地址",
    endpointPlaceholder: "192.168.1.100:8888 或 myhost.local",
    endpointHelper: "适用于跨 VLAN 或 WireGuard。保存后会立即尝试连接。",
    saveRemark: "保存备注",
    remarkHelper: "备注绑定设备 UUID，不受 IP 地址变化影响。",
    connecting: "正在连接 Xchat…",
    reconnecting: "连接已中断，正在重试；已加载的数据仍可查看。",
    connectionFailed: "无法连接本地 Xchat 服务。",
    sendFailed: "发送失败",
    deliveredCount: (delivered, total) => `已送达 ${delivered}/${total}`,
    readCount: (read, total) => `已读 ${read}/${total}`,
    status: {
      pending: "发送中",
      sent: "已发送",
      delivered: "已送达",
      read: "已读",
      received: "已接收",
      queued: "排队中",
      waiting_peer: "等待对方上线",
      offering: "正在发送请求",
      offered: "等待接收",
      awaiting_acceptance: "等待接收",
      accepted: "已接收",
      transferring: "传输中",
      receiving: "接收中",
      uploading: "上传中",
      downloading: "下载中",
      cancelling: "正在取消",
      cancelled: "已取消",
      completed: "已完成",
      failed: "失败",
      invalid: "文件不可用",
      removed: "本地副本已删除",
      rejected: "已拒绝",
    },
    sources: {
      lan: "局域网",
      manual: "手动添加",
    },
  },
  en: {
    locale: "en",
    chat: "Chat",
    hosts: "Hosts",
    files: "Files",
    settings: "Settings",
    searchChat: "Search conversations or messages",
    searchHosts: "Search name, address, or device ID",
    searchFiles: "Search files or sources",
    noConversation: "No conversations",
    noConversationHint: "Open a discovered host to start chatting.",
    noSearchResults: "No search results",
    noSearchResultsHint: "Try another search term.",
    noMessages: "No messages",
    noMessagesHint: "Send a message to start the conversation.",
    send: "Send",
    online: "Online",
    offline: "Offline",
    unnamedDevice: "Unnamed device",
    groupAvatar: "G",
    myDevice: "My device",
    groupChat: "Group",
    pinned: "Pinned",
    draftPrefix: "[Draft] ",
    remarked: "Remarked",
    manual: "Manual",
    unknownHostname: "Unknown hostname",
    unknownAddress: "Unknown address",
    fileFilters: {
      all: "All files",
      outgoing: "Sent by me",
      incoming: "Received by me",
      active: "In progress",
      failed: "Failed",
    },
    fileKinds: {
      all: "All",
      image: "Images",
      document: "Documents",
      audio: "Audio",
      video: "Video",
      other: "Other",
    },
    groupSources: "Groups",
    peerSources: "Devices & contacts",
    allFileSources: "All devices and groups",
    sourceGroup: "Group",
    sourceDevice: "Device",
    newGroup: "New group",
    addDevice: "Add device manually",
    closeList: "Close list",
    messageSearch: "Message search",
    chatHistory: "Chat history",
    noDevices: "No devices found",
    noDevicesHint: "Automatically discovered hosts appear here.",
    allConversations: "All conversations",
    filterByStatus: "Filter by status",
    settingsSections: {
      identity: "Identity",
      appearance: "Appearance",
      notification: "Notifications",
      download: "Downloads & transfers",
      network: "Network",
      shortcut: "Shortcuts",
    },
    attachment: "Attachment",
    emoji: "Emoji",
    removeAttachment: "Remove attachment",
    attachmentReady: "Ready to send",
    imagePreview: "Image preview",
    localFileUnavailable: "Local file unavailable",
    previewFile: "Preview file",
    sentDirection: "Sent by me",
    receivedDirection: "Received by me",
    file: "File",
    receive: "Receive",
    open: "Open",
    messagePlaceholder: "Type a message…",
    message: "Message",
    capture: "Capture",
    captureUnsupported: "Screen capture is unavailable on this platform",
    sendFile: "Send file",
    memberCount: (count) => `${count} ${count === 1 ? "member" : "members"}`,
    backConversationList: "Back to conversations",
    conversationInfo: "Conversation info",
    clearHistoryTitle: "Clear chat history?",
    clearHistoryDetail: "This only deletes local chat history and cannot be undone.",
    clearHistoryShortDetail: "This only deletes local history and cannot be undone.",
    clearHistoryAction: "Clear history",
    clearHistory: "Clear chat history",
    loadOlder: "Load older messages",
    deleteMessageTitle: "Delete this local message?",
    deleteMessageDetail: "This only deletes the copy saved on this device and cannot be undone.",
    deleteMessageAction: "Delete message",
    deleteLocalMessage: "Delete local message",
    delete: "Delete",
    activeTransfers: (count) => `${count} ${count === 1 ? "transfer" : "transfers"} in progress`,
    openFileCenterProgress: "Open File Center to view progress",
    selectHost: "Select a host",
    selectHostHint: "View discovery source, address, and stable device identity.",
    backHostList: "Back to hosts",
    unknownDiscovery: "Unknown discovery method",
    sendMessage: "Message",
    editRemark: "Edit remark",
    deviceIdentity: "Device identity",
    hostname: "Hostname",
    currentAddress: "Current address",
    macAddress: "MAC address",
    deviceId: "Device ID",
    discoveryMethod: "Discovery method",
    lastOnline: "Last online",
    availableMemory: "Available memory",
    notProvided: "Not provided",
    unknown: "Unknown",
    deleteLocalContact: "Delete local contact",
    deleteLocalContactHint: "The device can be discovered again when it comes online.",
    deleteDeviceTitle: (name) => `Delete “${name}”?`,
    deleteDeviceDetail: "Chat history and local contact information will be deleted.",
    deleteDeviceAction: "Delete",
    deleteDevice: "Delete device",
    receiveAgain: "Receive again",
    retry: "Retry",
    openFile: "Open file",
    revealFile: "Show in folder",
    deleteLocalCopy: "Delete local copy",
    backFileFilters: "Back to file filters",
    fileCenter: "File Center",
    fileSummary: (files, transfers) =>
      `${files} ${files === 1 ? "file" : "files"} · ${transfers} ${
        transfers === 1 ? "transfer" : "transfer records"
      }`,
    refresh: "Refresh",
    transferStatus: "Transfer status",
    transferName: (id) => `Transfer ${id}`,
    progress: (value) => `Progress ${value}%`,
    cancelling: "Cancelling",
    cancel: "Cancel",
    sourceTarget: "Source / target",
    time: "Time",
    size: "Size",
    actions: "Actions",
    unnamedFile: "Unnamed file",
    unknownStatus: "Unknown status",
    thisDevice: "This device",
    deleteFileTitle: (name) => `Delete “${name}”?`,
    deleteFileDetail: "Only the downloaded copy on this device will be deleted; chat history remains.",
    deleteFileAction: "Delete file",
    noMatchingFiles: "No matching files",
    noMatchingFilesHint: "Adjust the source, status, or search terms and try again.",
    noFiles: "No files yet",
    noFilesHint: "Files sent or received in conversations appear here.",
    previewUnavailable: "Preview is unavailable for this file type. Open its folder instead.",
    documentPreview: "Document preview",
    audioPreview: "Audio preview",
    videoPreview: "Video preview",
    backSettingsList: "Back to settings",
    settingsSubtitle: "Local preferences and network settings",
    saveSettings: "Save settings",
    identity: "Identity",
    localName: "Device name",
    deviceIdDetail: (id) => `Device ID ${id}`,
    deviceIdUnavailable: "Device ID unavailable",
    localAvatar: "Device avatar",
    localAvatarHint: "Stored only on this device",
    chooseAvatar: (avatar) => `Choose avatar ${avatar}`,
    appearance: "Appearance",
    theme: "Theme",
    systemTheme: "Use system setting",
    lightTheme: "Light",
    darkTheme: "Dark",
    language: "Language",
    simplifiedChinese: "Simplified Chinese",
    english: "English",
    notification: "Notifications",
    newMessageNotification: "New message notifications",
    permissionManagedBySystem: "Permission is managed by the system",
    platformUnavailable: "Unavailable on this platform",
    downloadsAndTransfers: "Downloads & transfers",
    downloadPath: "Download path",
    autoReceiveFiles: "Automatically receive files",
    autoReceiveFilesHint: "When off, files wait for manual acceptance",
    network: "Network",
    serverPort: "Server port",
    restartRequired: "Takes effect after restart",
    databasePath: "Database path",
    shortcuts: "Shortcuts",
    captureShortcut: "Capture shortcut",
    captureShortcutHint: "Focus this field, then press a letter or number shortcut",
    captureShortcutFocusedHint: "Works only while the Xchat window is focused",
    groupMembers: (count) => `${count} group ${count === 1 ? "member" : "members"}`,
    editDeviceRemark: "Edit device remark",
    unpinConversation: "Unpin conversation",
    pinConversation: "Pin conversation",
    unmarkUnread: "Remove unread mark",
    markUnread: "Mark as unread",
    close: "Close",
    cancelAction: "Cancel",
    createGroup: "Create group",
    groupName: "Group name",
    groupHelper: "Select at least two remote devices that support group chat.",
    deviceAddress: "Device address",
    endpointPlaceholder: "192.168.1.100:8888 or myhost.local",
    endpointHelper: "For cross-VLAN or WireGuard connections. Xchat tries to connect immediately after saving.",
    saveRemark: "Save remark",
    remarkHelper: "The remark is linked to the device UUID and is unaffected by IP address changes.",
    connecting: "Connecting to Xchat…",
    reconnecting: "Connection interrupted. Retrying; loaded data remains available.",
    connectionFailed: "Could not connect to the local Xchat service.",
    sendFailed: "Failed to send",
    deliveredCount: (delivered, total) => `Delivered ${delivered}/${total}`,
    readCount: (read, total) => `Read by ${read}/${total}`,
    status: {
      pending: "Sending",
      sent: "Sent",
      delivered: "Delivered",
      read: "Read",
      received: "Received",
      queued: "Queued",
      waiting_peer: "Waiting for peer",
      offering: "Sending offer",
      offered: "Awaiting acceptance",
      awaiting_acceptance: "Awaiting acceptance",
      accepted: "Accepted",
      transferring: "Transferring",
      receiving: "Receiving",
      uploading: "Uploading",
      downloading: "Downloading",
      cancelling: "Cancelling",
      cancelled: "Cancelled",
      completed: "Completed",
      failed: "Failed",
      invalid: "File unavailable",
      removed: "Local copy deleted",
      rejected: "Rejected",
    },
    sources: {
      lan: "LAN",
      manual: "Manual",
    },
  },
};

function Icon({ name, size = 20 }) {
  let body;
  switch (name) {
    case "chat":
      body = (
        <>
          <path d="M7 4h10a4 4 0 0 1 4 4v6a4 4 0 0 1-4 4H9l-5 3v-5.5A4 4 0 0 1 3 13V8a4 4 0 0 1 4-4Z" />
          <path d="M8 11h.01M12 11h.01M16 11h.01" />
        </>
      );
      break;
    case "hosts":
      body = (
        <>
          <rect x="3" y="5" width="18" height="12" rx="2" />
          <path d="M8 21h8M12 17v4" />
        </>
      );
      break;
    case "files":
      body = (
        <>
          <path d="M3 6.5h7l2 2h9v9.5a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z" />
          <path d="M3 9h18" />
        </>
      );
      break;
    case "settings":
      body = (
        <>
          <path d="M4 6h7M15 6h5M4 12h2M10 12h10M4 18h10M18 18h2" />
          <circle cx="13" cy="6" r="2" />
          <circle cx="8" cy="12" r="2" />
          <circle cx="16" cy="18" r="2" />
        </>
      );
      break;
    case "search":
      body = (
        <>
          <circle cx="11" cy="11" r="7" />
          <path d="m20 20-4-4" />
        </>
      );
      break;
    case "plus":
      body = <path d="M12 5v14M5 12h14" />;
      break;
    case "info":
      body = (
        <>
          <circle cx="12" cy="12" r="9" />
          <path d="M12 11v5M12 8h.01" />
        </>
      );
      break;
    case "more":
      body = (
        <>
          <circle cx="5" cy="12" r="1" />
          <circle cx="12" cy="12" r="1" />
          <circle cx="19" cy="12" r="1" />
        </>
      );
      break;
    case "attach":
      body = (
        <>
          <path d="M3 6.5h7l2 2h9v9.5a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z" />
          <path d="M3 9h18" />
        </>
      );
      break;
    case "capture":
      body = (
        <>
          <circle cx="6" cy="7" r="3" />
          <circle cx="6" cy="17" r="3" />
          <path d="m8.5 8.5 11 7.5M8.5 15.5 20 8M4 12h4" />
        </>
      );
      break;
    case "emoji":
      body = (
        <>
          <circle cx="12" cy="12" r="9" />
          <path d="M8.5 10h.01M15.5 10h.01M8 14c1 2 2.3 3 4 3s3-1 4-3" />
        </>
      );
      break;
    case "download":
      body = <path d="M12 3v12m0 0 4-4m-4 4-4-4M5 21h14" />;
      break;
    case "refresh":
      body = (
        <>
          <path d="M20 6v5h-5" />
          <path d="M19 11a8 8 0 1 0 1 5" />
        </>
      );
      break;
    case "file":
      body = (
        <>
          <path d="M6 2h8l4 4v16H6zM14 2v5h5" />
          <path d="M9 13h6M9 17h4" />
        </>
      );
      break;
    case "eye":
      body = (
        <>
          <path d="M2 12s3.5-6 10-6 10 6 10 6-3.5 6-10 6S2 12 2 12Z" />
          <circle cx="12" cy="12" r="2.5" />
        </>
      );
      break;
    case "image":
      body = (
        <>
          <rect x="3" y="4" width="18" height="16" rx="2" />
          <circle cx="9" cy="9" r="1.5" />
          <path d="m4 17 5-5 4 4 2-2 5 4" />
        </>
      );
      break;
    case "audio":
      body = (
        <>
          <path d="M9 18V5l10-2v13" />
          <circle cx="6" cy="18" r="3" />
          <circle cx="16" cy="16" r="3" />
        </>
      );
      break;
    case "video":
      body = (
        <>
          <rect x="3" y="5" width="14" height="14" rx="2" />
          <path d="m17 10 4-3v10l-4-3Z" />
        </>
      );
      break;
    case "back":
      body = <path d="m15 18-6-6 6-6" />;
      break;
    case "close":
      body = <path d="M6 6l12 12M18 6 6 18" />;
      break;
    case "trash":
      body = (
        <>
          <path d="M4 7h16M9 3h6l1 4H8ZM7 7l1 14h8l1-14" />
          <path d="M10 11v6M14 11v6" />
        </>
      );
      break;
    case "folder":
      body = (
        <>
          <path d="M3 6.5h7l2 2h9v9.5a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z" />
          <path d="M3 9h18" />
        </>
      );
      break;
    default:
      body = <circle cx="12" cy="12" r="8" />;
  }
  return (
    <svg
      aria-hidden="true"
      className="icon"
      width={size}
      height={size}
      viewBox="0 0 24 24"
    >
      {body}
    </svg>
  );
}

function hashColor(id = "") {
  let value = 0;
  for (const character of id) value = (value + character.charCodeAt(0)) % 8;
  return `var(--avatar-${value + 1})`;
}

function displayName(entity = {}, labels = copy["zh-CN"]) {
  return entity.remark || entity.title || entity.name || entity.hostname || labels.unnamedDevice;
}

function Avatar({ entity = {}, labels, large = false, self = false }) {
  const label =
    entity.avatar ||
    (entity.kind === "group"
      ? labels.groupAvatar
      : displayName(entity, labels).slice(0, 1));
  return (
    <span
      aria-hidden="true"
      className={`avatar${large ? " avatar-large" : ""}`}
      style={{ "--avatar": hashColor(entity.id || entity.peer_id || (self ? "self" : label)) }}
    >
      {label}
    </span>
  );
}

function formatTime(timestamp, locale) {
  if (!timestamp) return "";
  return new Intl.DateTimeFormat(locale, {
    month: "numeric",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(timestamp * 1000));
}

function formatSize(bytes) {
  const value = Number(bytes || 0);
  if (!value) return "—";
  if (value < 1024) return `${value} B`;
  if (value < 1024 ** 2) return `${(value / 1024).toFixed(1)} KB`;
  if (value < 1024 ** 3) return `${(value / 1024 ** 2).toFixed(1)} MB`;
  return `${(value / 1024 ** 3).toFixed(1)} GB`;
}

function sourceIdForFile(file) {
  return file.conversation_id || file.peer_id || file.source_id || "unknown";
}

function sourceForFile(file, state) {
  const conversation = state.conversations.find(
    (item) => item.id === file.conversation_id,
  );
  if (conversation?.kind === "group") return conversation;
  const peerId = file.peer_id || conversation?.peer_id;
  return (
    state.devices.find((device) => device.id === peerId) ||
    conversation?.peer ||
    conversation ||
    {
      id: peerId || sourceIdForFile(file),
      name: file.peer_name || file.source_name || "",
    }
  );
}

function fileSources(state) {
  const sources = new Map();
  for (const file of state.files) {
    const entity = sourceForFile(file, state);
    const id = entity?.id || sourceIdForFile(file);
    const current = sources.get(id);
    sources.set(id, { id, entity, count: (current?.count || 0) + 1 });
  }
  return [...sources.values()].sort((a, b) => {
    const groupOrder = Number(b.entity?.kind === "group") - Number(a.entity?.kind === "group");
    return groupOrder || displayName(a.entity).localeCompare(displayName(b.entity));
  });
}

function fileMatchesSource(file, sourceId, state) {
  if (sourceId === "all") return true;
  return (sourceForFile(file, state)?.id || sourceIdForFile(file)) === sourceId;
}

function mediaResultUrl(payload) {
  if (!payload) return { url: "", revoke: false };
  if (typeof payload === "string") return { url: payload, revoke: false };
  if (payload.data_url || payload.preview_url) {
    return { url: payload.data_url || payload.preview_url, revoke: false };
  }
  if (payload.blob instanceof Blob) {
    return { url: URL.createObjectURL(payload.blob), revoke: true };
  }
  const bytes = payload.bytes ?? payload.data;
  if (Array.isArray(bytes) || bytes instanceof Uint8Array) {
    const blob = new Blob([new Uint8Array(bytes)], {
      type: payload.mime_type || "application/octet-stream",
    });
    return { url: URL.createObjectURL(blob), revoke: true };
  }
  return { url: "", revoke: false };
}

function useMessageMedia(message, workspace, enabled = true) {
  const [media, setMedia] = useState({ url: "", failed: false });
  const messageId = message.message_id ?? message.id;
  useEffect(() => {
    if (!enabled || messageId == null || !localFileAvailable(message)) {
      setMedia({ url: "", failed: false });
      return;
    }
    let disposed = false;
    let objectUrl = "";
    workspace
      .dispatch({ type: "media.readMessage", messageId })
      .then((result) => {
        if (disposed) return;
        if (!result.ok) {
          setMedia({ url: "", failed: true });
          return;
        }
        const value = mediaResultUrl(result.data);
        if (value.revoke) objectUrl = value.url;
        setMedia({ url: value.url, failed: !value.url });
      });
    return () => {
      disposed = true;
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [enabled, messageId, workspace]);
  return media;
}

function statusText(status, labels) {
  return labels.status[status] || status;
}

function sourceText(source, labels) {
  return labels.sources[source] || source || labels.unknown;
}

function statusLabel(message, group, labels) {
  if (!message.own) return "";
  if (message.status === "failed") return labels.sendFailed;
  if (group && message.recipient_count) {
    return `${labels.deliveredCount(
      message.delivered_count || 0,
      message.recipient_count,
    )} · ${labels.readCount(message.read_count || 0, message.recipient_count)}`;
  }
  return statusText(message.status, labels);
}

function useTheme(theme) {
  useEffect(() => {
    const media = matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const resolved = theme === "system" ? (media.matches ? "dark" : "light") : theme;
      document.documentElement.dataset.theme = resolved;
    };
    apply();
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [theme]);
}

function Rail({ state, labels, onOpen }) {
  const unread = state.conversations.reduce(
    (total, conversation) => total + (conversation.unread_count || 0),
    0,
  );
  const items = [
    ["chat", labels.chat],
    ["hosts", labels.hosts],
    ["files", labels.files],
  ];
  return (
    <aside className="rail" data-od-id="primary-navigation">
      <button
        className="self-button"
        aria-label={labels.myDevice}
        onClick={() => onOpen("settings")}
        title={state.self.name || labels.myDevice}
      >
        <Avatar entity={state.self} labels={labels} self />
        <i className="presence" />
      </button>
      <nav>
        {items.map(([section, label]) => (
          <button
            className={`rail-button ${state.activeSection === section ? "active" : ""}`}
            key={section}
            onClick={() => onOpen(section)}
            aria-label={label}
            title={label}
            data-od-id={`nav-${section}`}
          >
            <Icon name={section} size={24} />
            {section === "chat" && unread > 0 && (
              <span className="nav-badge">{Math.min(unread, 99)}</span>
            )}
          </button>
        ))}
      </nav>
      <button
        className={`rail-button rail-settings ${
          state.activeSection === "settings" ? "active" : ""
        }`}
        onClick={() => onOpen("settings")}
        aria-label={labels.settings}
        title={labels.settings}
        data-od-id="nav-settings"
      >
        <Icon name="settings" size={24} />
      </button>
    </aside>
  );
}

function SearchBox({ value, onChange, placeholder }) {
  return (
    <label className="search-box">
      <Icon name="search" size={16} />
      <span className="sr-only">{placeholder}</span>
      <input value={value} onChange={(event) => onChange(event.target.value)} placeholder={placeholder} />
    </label>
  );
}

function ConversationRow({ conversation, labels, selected, onOpen }) {
  return (
    <button
      className={`conversation-row ${selected ? "selected" : ""}`}
      onClick={onOpen}
      data-od-id={`conversation-${conversation.id}`}
    >
      <Avatar entity={conversation.peer || conversation} labels={labels} />
      <span className="row-main">
        <span className="row-name-line">
          <b>{displayName(conversation, labels)}</b>
          {conversation.kind === "group" && <span className="tag">{labels.groupChat}</span>}
          {conversation.pinned && <span className="tag">{labels.pinned}</span>}
        </span>
        <span className="row-preview">
          {conversation.draft && <i className="draft">{labels.draftPrefix}</i>}
          {conversation.draft || conversation.last_message || labels.noMessages}
        </span>
      </span>
      <span className="row-side">
        <time>{formatTime(conversation.last_message_at, labels.locale)}</time>
        {(conversation.unread_count > 0 || conversation.forced_unread) && (
          <span className="unread-badge">{conversation.unread_count || "•"}</span>
        )}
      </span>
    </button>
  );
}

function DeviceRow({ device, labels, selected, onOpen }) {
  return (
    <button
      className={`device-row ${selected ? "selected" : ""}`}
      onClick={onOpen}
      data-od-id={`device-${device.id}`}
    >
      <Avatar entity={device} labels={labels} />
      <span className="row-main">
        <span className="row-name-line">
          <b>{displayName(device, labels)}</b>
          {device.remark && <span className="tag">{labels.remarked}</span>}
          {device.discovery_source === "manual" && <span className="tag">{labels.manual}</span>}
        </span>
        <span className="device-meta">
          <span>{device.hostname || device.name || labels.unknownHostname}</span>
          <span>{device.addr || labels.unknownAddress}</span>
        </span>
      </span>
      <i
        className={device.is_offline ? "status-ring" : "status-dot"}
        title={device.is_offline ? labels.offline : labels.online}
      />
    </button>
  );
}

function ListPane({
  state,
  labels,
  query,
  setQuery,
  selectedDeviceId,
  fileFilter,
  onConversation,
  onDevice,
  onAdd,
  onFileFilter,
  onCloseMobile,
  settingsSection,
  onSettingsSection,
}) {
  const section = state.activeSection;
  const text = query.trim().toLocaleLowerCase();
  const conversations = [...state.conversations]
    .filter((item) =>
      `${displayName(item, labels)} ${item.last_message} ${item.draft}`
        .toLocaleLowerCase()
        .includes(text),
    )
    .sort((a, b) => Number(b.pinned) - Number(a.pinned) || b.last_message_at - a.last_message_at);
  const devices = state.devices.filter((item) =>
    `${displayName(item, labels)} ${item.hostname} ${item.addr} ${item.mac_address} ${item.id}`
      .toLocaleLowerCase()
      .includes(text),
  );
  const sources = fileSources(state);
  const chatEmpty =
    section === "chat" &&
    !conversations.length &&
    !(text && state.searchResults.length);
  const hostsEmpty = section === "hosts" && !devices.length;

  return (
    <aside className="list-pane" data-od-id={`${section}-list`}>
      <header className="list-head">
        {section === "settings" ? (
          <b>{labels.settings}</b>
        ) : (
          <SearchBox
            value={query}
            onChange={setQuery}
            placeholder={
              section === "chat"
                ? labels.searchChat
                : section === "hosts"
                  ? labels.searchHosts
                  : labels.searchFiles
            }
          />
        )}
        {(section === "chat" || section === "hosts") && (
          <button
            className="icon-button"
            onClick={onAdd}
            disabled={section === "chat" && !state.capabilities.groupChat}
            aria-label={section === "chat" ? labels.newGroup : labels.addDevice}
            title={section === "chat" ? labels.newGroup : labels.addDevice}
          >
            <Icon name="plus" />
          </button>
        )}
        <button
          className="mobile-close-list icon-button"
          onClick={onCloseMobile}
          aria-label={labels.closeList}
        >
          <Icon name="close" />
        </button>
      </header>
      <div
        className={`list-scroll ${
          chatEmpty || hostsEmpty ? "has-centered-empty" : ""
        }`}
      >
        {section === "chat" && (
          <>
            {text && state.searchResults.length > 0 && (
              <>
                <div className="list-group-label">{labels.messageSearch}</div>
                {state.searchResults.slice(0, 8).map((result) => (
                  <button
                    className="search-result"
                    key={result.client_message_id || result.id}
                    onClick={() =>
                      onConversation(result.conversation_id, {
                        targetMessageId: result.message_id ?? result.id,
                        targetClientMessageId: result.client_message_id,
                      })
                    }
                  >
                    <b>{result.conversation_title || labels.chatHistory}</b>
                    <span>{result.content || result.file_name}</span>
                  </button>
                ))}
              </>
            )}
            {conversations.length ? (
              conversations.map((conversation) => (
                <ConversationRow
                  conversation={conversation}
                  labels={labels}
                  selected={conversation.id === state.activeConversationId}
                  onOpen={() => onConversation(conversation.id)}
                  key={conversation.id}
                />
              ))
            ) : !text || !state.searchResults.length ? (
              <ListEmpty
                title={text ? labels.noSearchResults : labels.noConversation}
                detail={
                  text ? labels.noSearchResultsHint : labels.noConversationHint
                }
              />
            ) : null}
          </>
        )}
        {section === "hosts" && (
          <>
            {devices.length ? (
              <>
                {devices.some((device) => !device.is_offline) && (
                  <>
                    <div className="list-group-label">
                      {labels.online} ·{" "}
                      {devices.filter((device) => !device.is_offline).length}
                    </div>
                    {devices
                      .filter((device) => !device.is_offline)
                      .map((device) => (
                        <DeviceRow
                          device={device}
                          labels={labels}
                          selected={device.id === selectedDeviceId}
                          onOpen={() => onDevice(device.id)}
                          key={device.id}
                        />
                      ))}
                  </>
                )}
                {devices.some((device) => device.is_offline) && (
                  <>
                    <div className="list-group-label">
                      {labels.offline} ·{" "}
                      {devices.filter((device) => device.is_offline).length}
                    </div>
                    {devices
                      .filter((device) => device.is_offline)
                      .map((device) => (
                        <DeviceRow
                          device={device}
                          labels={labels}
                          selected={device.id === selectedDeviceId}
                          onOpen={() => onDevice(device.id)}
                          key={device.id}
                        />
                      ))}
                  </>
                )}
              </>
            ) : (
              <ListEmpty
                title={text ? labels.noSearchResults : labels.noDevices}
                detail={text ? labels.noSearchResultsHint : labels.noDevicesHint}
              />
            )}
          </>
        )}
        {section === "files" && (
          <>
            <button
              className={`source-filter ${fileFilter === "all" ? "selected" : ""}`}
              onClick={() => onFileFilter("all")}
            >
              <span className="source-icon"><Icon name="folder" /></span>
              <span className="row-main">
                <b>{labels.fileFilters.all}</b>
                <span className="row-preview">{labels.allFileSources}</span>
              </span>
              <span className="source-count">{state.files.length}</span>
            </button>
            {sources.some((source) => source.entity?.kind === "group") && (
              <div className="list-group-label">{labels.groupSources}</div>
            )}
            {sources
              .filter((source) => source.entity?.kind === "group")
              .map(({ id, entity, count }) => (
                <button
                  className={`source-filter ${fileFilter === id ? "selected" : ""}`}
                  onClick={() => onFileFilter(id)}
                  key={id}
                >
                  <Avatar entity={entity} labels={labels} />
                  <span className="row-main">
                    <b>{displayName(entity, labels)}</b>
                    <span className="row-preview">{labels.sourceGroup}</span>
                  </span>
                  <span className="source-count">{count}</span>
                </button>
              ))}
            {sources.some((source) => source.entity?.kind !== "group") && (
              <div className="list-group-label">{labels.peerSources}</div>
            )}
            {sources
              .filter((source) => source.entity?.kind !== "group")
              .map(({ id, entity, count }) => (
              <button
                  className={`source-filter ${fileFilter === id ? "selected" : ""}`}
                  onClick={() => onFileFilter(id)}
                  key={id}
              >
                  <Avatar entity={entity} labels={labels} />
                <span className="row-main">
                    <b>{displayName(entity, labels)}</b>
                    <span className="row-preview">
                      {entity.hostname || entity.name || labels.sourceDevice}
                    </span>
                </span>
                <span className="source-count">{count}</span>
              </button>
              ))}
          </>
        )}
        {section === "settings" &&
          Object.entries(labels.settingsSections).map(([id, label]) => (
            <button
              className={`settings-nav-row ${
                settingsSection === id ? "selected" : ""
              }`}
              key={id}
              onClick={() => onSettingsSection(id)}
              aria-current={settingsSection === id ? "location" : undefined}
            >
              {label}
            </button>
          ))}
      </div>
    </aside>
  );
}

function ListEmpty({ title, detail }) {
  return (
    <div className="list-empty">
      <b>{title}</b>
      <span>{detail}</span>
    </div>
  );
}

function EmptyState({ icon = "chat", title, detail }) {
  return (
    <div className="empty-state">
      <Icon name={icon} size={48} />
      <b>{title}</b>
      <p>{detail}</p>
    </div>
  );
}

function MessageFile({ message, state, workspace, labels }) {
  const status = fileStatus(message);
  const image = isImageFile(message);
  const available = localFileAvailable(message);
  const media = useMessageMedia(message, workspace, image && available);
  const messageId = message.message_id ?? message.id;
  const activeTransfer = state.transfers.find(
    (transfer) =>
      messageId != null &&
      transfer.message_id != null &&
      String(transfer.message_id) === String(messageId) &&
      ACTIVE_TRANSFER_STATES.has(transfer.status),
  );
  const direction = message.direction || (message.own ? "outgoing" : "incoming");
  const canOpen =
    available &&
    (direction === "incoming" || state.capabilities.openOutgoingFile);
  if (image && media.url) {
    return (
      <button
        className="message-image"
        type="button"
        onClick={() => workspace.dispatch({ type: "file.open", file: message })}
        aria-label={`${labels.openFile}: ${
          message.file_name || message.content || labels.attachment
        }`}
      >
        <img
          src={media.url}
          alt={message.file_name || message.content || labels.imagePreview}
        />
      </button>
    );
  }
  return (
    <div
      className={`message-file ${
        status === "failed" || !available ? "invalid" : ""
      }`}
    >
      <span className="file-icon">
        <Icon name={image ? "image" : "file"} />
      </span>
      <span className="message-file-main">
        <b>{message.file_name || message.content || labels.attachment}</b>
        <span>
          {!available
            ? labels.localFileUnavailable
            : statusText(status, labels) || labels.file}{" "}
          · {formatSize(message.file_size)}
        </span>
      </span>
      {["offered", "awaiting_acceptance"].includes(status) && (
        <button onClick={() => workspace.dispatch({ type: "file.accept", file: message })}>
          {labels.receive}
        </button>
      )}
      {status === "failed" && (
        <button
          onClick={() =>
            workspace.dispatch({
              type: direction === "incoming" ? "file.accept" : "file.retry",
              file: message,
            })
          }
        >
          {direction === "incoming" ? labels.receiveAgain : labels.retry}
        </button>
      )}
      {activeTransfer && state.capabilities.transferCancel && (
        <button
          disabled={activeTransfer.status === "cancelling"}
          onClick={() =>
            workspace.dispatch({
              type: "transfer.cancel",
              id: activeTransfer.id,
            })
          }
        >
          {activeTransfer.status === "cancelling"
            ? labels.cancelling
            : labels.cancel}
        </button>
      )}
      {canOpen && ["accepted", "completed", "sent"].includes(status) && (
        <button onClick={() => workspace.dispatch({ type: "file.open", file: message })}>
          {labels.open}
        </button>
      )}
    </div>
  );
}

function DraftAttachment({ attachment, labels, onRemove }) {
  const [preview, setPreview] = useState(attachment.preview_url || "");
  useEffect(() => {
    if (attachment.preview_url || !isImageFile(attachment) || !attachment.file) return;
    const url = URL.createObjectURL(attachment.file);
    setPreview(url);
    return () => URL.revokeObjectURL(url);
  }, [attachment]);
  return (
    <div className="draft-attachment">
      {preview && isImageFile(attachment) ? (
        <img src={preview} alt={attachment.file_name || labels.imagePreview} />
      ) : (
        <span className="draft-file-icon">
          <Icon name={isImageFile(attachment) ? "image" : "file"} />
        </span>
      )}
      <span className="draft-attachment-main">
        <b>{attachment.file_name || attachment.name || labels.attachment}</b>
        <small>
          {formatSize(attachment.file_size)} · {labels.attachmentReady}
        </small>
      </span>
      <button
        className="icon-button"
        type="button"
        onClick={onRemove}
        aria-label={labels.removeAttachment}
        title={labels.removeAttachment}
      >
        <Icon name="close" size={16} />
      </button>
    </div>
  );
}

function Composer({ state, conversation, workspace, labels }) {
  const [text, setText] = useState(conversation?.draft || "");
  const [emojiOpen, setEmojiOpen] = useState(false);
  const [sending, setSending] = useState(false);
  const textarea = useRef(null);
  const input = useRef(null);
  const emojiPanel = useRef(null);
  const attachments = state.draftAttachments?.[conversation.id] || [];

  useEffect(() => {
    setText(conversation?.draft || "");
    setEmojiOpen(false);
  }, [conversation?.id]);

  useEffect(() => {
    const element = textarea.current;
    if (!element) return;
    element.style.height = "auto";
    element.style.height = `${Math.min(element.scrollHeight, 200)}px`;
  }, [text]);

  useEffect(() => {
    if (!emojiOpen) return;
    const close = (event) => {
      if (event.key === "Escape") setEmojiOpen(false);
      if (
        event.type === "pointerdown" &&
        !emojiPanel.current?.contains(event.target) &&
        !event.target.closest?.("[data-emoji-toggle]")
      ) {
        setEmojiOpen(false);
      }
    };
    addEventListener("keydown", close);
    addEventListener("pointerdown", close);
    return () => {
      removeEventListener("keydown", close);
      removeEventListener("pointerdown", close);
    };
  }, [emojiOpen]);

  const send = async () => {
    const content = text.trim();
    if ((!content && !attachments.length) || sending) return;
    setSending(true);
    try {
      if (content) {
        const result = await workspace.dispatch({ type: "message.sendText", content });
        if (result.ok) {
          setText("");
          if (state.capabilities.conversationState) {
            workspace.dispatch({
              type: "conversation.saveDraft",
              id: conversation.id,
              draft: "",
            });
          }
        }
      }
      for (const attachment of attachments) {
        const result = await workspace.dispatch({
          type: "message.sendFiles",
          files: [attachment],
        });
        if (result.ok) {
          await workspace.dispatch({
            type: "draft.sent",
            conversationId: conversation.id,
            id: attachment.id,
          });
        }
      }
    } finally {
      setSending(false);
    }
  };

  const attach = () => {
    if (state.capabilities.nativeFilePicker) {
      workspace.dispatch({ type: "draft.pickFiles" });
    } else {
      input.current?.click();
    }
  };

  const insertEmoji = (emoji) => {
    const element = textarea.current;
    const start = element?.selectionStart ?? text.length;
    const end = element?.selectionEnd ?? start;
    const next = `${text.slice(0, start)}${emoji}${text.slice(end)}`;
    setText(next);
    requestAnimationFrame(() => {
      element?.focus();
      element?.setSelectionRange(start + emoji.length, start + emoji.length);
    });
  };

  return (
    <footer
      className="composer"
      data-od-id="message-composer"
      onDragOver={(event) => event.preventDefault()}
      onDrop={(event) => {
        event.preventDefault();
        event.stopPropagation();
        if (event.dataTransfer.files.length) {
          workspace.dispatch({
            type: "draft.addFiles",
            conversationId: conversation.id,
            files: [...event.dataTransfer.files],
          });
        }
      }}
    >
      <div className="compose-box">
        {attachments.length > 0 && (
          <div className="draft-attachments">
            {attachments.map((attachment) => (
              <DraftAttachment
                attachment={attachment}
                labels={labels}
                key={attachment.id}
                onRemove={() =>
                  workspace.dispatch({
                    type: "draft.remove",
                    conversationId: conversation.id,
                    id: attachment.id,
                  })
                }
              />
            ))}
          </div>
        )}
        <textarea
          ref={textarea}
          value={text}
          onChange={(event) => setText(event.target.value)}
          onBlur={() => {
            if (
              state.capabilities.conversationState &&
              text !== (conversation.draft || "")
            ) {
              workspace.dispatch({
                type: "conversation.saveDraft",
                id: conversation.id,
                draft: text,
              });
            }
          }}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
              event.preventDefault();
              send();
            }
          }}
          onPaste={(event) => {
            const files = [...event.clipboardData.files].filter(isImageFile);
            if (files.length) {
              workspace.dispatch({
                type: "draft.addFiles",
                conversationId: conversation.id,
                files,
              });
            }
          }}
          rows="2"
          placeholder={labels.messagePlaceholder}
          aria-label={labels.message}
        />
        <div className="compose-toolbar">
          <div className="compose-tools">
            <button
              className={`icon-button composer-tool ${emojiOpen ? "active" : ""}`}
              type="button"
              data-emoji-toggle
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => setEmojiOpen((value) => !value)}
              aria-label={labels.emoji}
              title={labels.emoji}
              aria-expanded={emojiOpen}
            >
              <Icon name="emoji" />
            </button>
            <button
              className="icon-button composer-tool"
              onClick={() => workspace.dispatch({ type: "capture.start" })}
              disabled={!state.capabilities.capture}
              aria-label={labels.capture}
              title={
                state.capabilities.capture
                  ? labels.capture
                  : labels.captureUnsupported
              }
            >
              <Icon name="capture" />
            </button>
            <button
              className="icon-button composer-tool"
              onClick={attach}
              aria-label={labels.sendFile}
              title={labels.sendFile}
            >
              <Icon name="attach" />
            </button>
            <input
              ref={input}
              type="file"
              multiple
              hidden
              onChange={(event) => {
                const files = [...event.target.files];
                if (files.length) {
                  workspace.dispatch({
                    type: "draft.addFiles",
                    conversationId: conversation.id,
                    files,
                  });
                }
                event.target.value = "";
              }}
            />
          </div>
          <button
            className="primary-button send-button"
            onMouseDown={(event) => event.preventDefault()}
            onClick={send}
            disabled={sending || (!text.trim() && !attachments.length)}
          >
            {labels.send}
          </button>
        </div>
        {emojiOpen && (
          <div className="emoji-panel" ref={emojiPanel}>
            <div className="emoji-grid" role="listbox" aria-label={labels.emoji}>
              {EMOJI_SET.map((emoji) => (
                <button
                  type="button"
                  key={emoji}
                  onMouseDown={(event) => event.preventDefault()}
                  onClick={() => insertEmoji(emoji)}
                  aria-label={emoji}
                >
                  {emoji}
                </button>
              ))}
            </div>
          </div>
        )}
      </div>
    </footer>
  );
}

function ChatWorkspace({ state, workspace, labels, onBack, onToggleInfo, infoOpen, onConfirm }) {
  const conversation = state.conversations.find(
    (item) => item.id === state.activeConversationId,
  );
  const messages = state.messagesByConversation[state.activeConversationId] || [];
  const scroll = useRef(null);

  useEffect(() => {
    scroll.current?.scrollTo({ top: scroll.current.scrollHeight });
  }, [conversation?.id, messages.length]);

  useEffect(() => {
    if (state.focusedMessageId == null) return;
    const target = String(state.focusedMessageId);
    const element = [...(scroll.current?.querySelectorAll("[data-message-key]") || [])].find(
      (item) =>
        item.dataset.messageKey === target ||
        item.dataset.messageId === target ||
        item.dataset.clientMessageId === target,
    );
    if (!element) return;
    element.scrollIntoView({ behavior: "smooth", block: "center" });
    const previousShadow = element.style.boxShadow;
    element.style.boxShadow = "0 0 0 3px var(--accent)";
    const timer = setTimeout(() => {
      element.style.boxShadow = previousShadow;
    }, 1600);
    return () => {
      clearTimeout(timer);
      element.style.boxShadow = previousShadow;
    };
  }, [conversation?.id, messages.length, state.focusedMessageId]);

  useEffect(() => {
    if (conversation && document.visibilityState === "visible") {
      workspace.dispatch({ type: "message.markRead", conversationId: conversation.id });
    }
  }, [conversation?.id, messages.length, workspace]);

  useEffect(() => {
    if (!conversation) return;
    const markReadWhenVisible = () => {
      if (document.visibilityState === "visible") {
        workspace.dispatch({
          type: "message.markRead",
          conversationId: conversation.id,
        });
      }
    };
    document.addEventListener("visibilitychange", markReadWhenVisible);
    return () =>
      document.removeEventListener("visibilitychange", markReadWhenVisible);
  }, [conversation?.id, workspace]);

  if (!conversation) {
    return (
      <main className="workspace chat-workspace no-selection">
        <EmptyState
          title={labels.noConversation}
          detail={labels.noConversationHint}
        />
      </main>
    );
  }
  const peer = conversation.peer;
  const subtitle =
    conversation.kind === "group"
      ? labels.memberCount(conversation.members?.length || 0)
      : `${peer?.addr || labels.unknownAddress} · ${
          peer?.is_offline ? labels.offline : labels.online
        }`;

  return (
    <main
      className="workspace chat-workspace"
      data-od-id="chat-workspace"
      onDragOver={(event) => event.preventDefault()}
      onDrop={(event) => {
        event.preventDefault();
        if (event.dataTransfer.files.length) {
          workspace.dispatch({
            type: "draft.addFiles",
            conversationId: conversation.id,
            files: [...event.dataTransfer.files],
          });
        }
      }}
    >
      <header className="workspace-head">
        <button
          className="mobile-back icon-button"
          onClick={onBack}
          aria-label={labels.backConversationList}
        >
          <Icon name="back" />
        </button>
        <div className="workspace-heading">
          <b>{displayName(conversation, labels)}</b>
          <span>{subtitle}</span>
        </div>
        <div className="head-actions">
          <button
            className={`icon-button info-toggle ${infoOpen ? "active" : ""}`}
            onClick={onToggleInfo}
            aria-label={labels.conversationInfo}
            title={labels.conversationInfo}
          >
            <Icon name="info" />
          </button>
          <button
            className="icon-button"
            onClick={() =>
              onConfirm({
                title: labels.clearHistoryTitle,
                detail: labels.clearHistoryDetail,
                action: labels.clearHistoryAction,
                run: () => workspace.dispatch({ type: "message.clearConversation" }),
              })
            }
            aria-label={labels.clearHistory}
            title={labels.clearHistory}
          >
            <Icon name="more" />
          </button>
        </div>
      </header>
      <div
        className={`message-scroll ${!messages.length ? "has-empty-state" : ""}`}
        ref={scroll}
      >
        {messages.length > 0 && (
          <button
            className="load-older"
            onClick={() => workspace.dispatch({ type: "conversation.loadOlder" })}
          >
            {labels.loadOlder}
          </button>
        )}
        {!messages.length && (
          <EmptyState title={labels.noMessages} detail={labels.noMessagesHint} />
        )}
        {messages.map((message) => (
          <article
            className={`message ${message.own ? "sent" : "received"}`}
            key={message.client_message_id || message.id}
            data-od-id={`message-${message.client_message_id || message.id}`}
            data-message-key={
              message.client_message_id || message.message_id || message.id
            }
            data-message-id={message.message_id ?? message.id}
            data-client-message-id={message.client_message_id || undefined}
          >
            <Avatar
              entity={
                message.own
                  ? state.self
                  : { id: message.sender_id, name: message.sender_name }
              }
              labels={labels}
            />
            <div className="message-stack">
              {!message.own && conversation.kind === "group" && (
                <span className="sender-label">
                  {message.sender_name || message.sender_id}
                </span>
              )}
              {message.msg_type === "text" ? (
                <div className="bubble">{message.content}</div>
              ) : (
                <MessageFile
                  message={message}
                  state={state}
                  workspace={workspace}
                  labels={labels}
                />
              )}
              <span className={`message-meta ${message.status === "failed" ? "danger-text" : ""}`}>
                {formatTime(message.timestamp, labels.locale)}
                {statusLabel(message, conversation.kind === "group", labels) && (
                  <i>{statusLabel(message, conversation.kind === "group", labels)}</i>
                )}
                {message.id !== undefined && message.status !== "pending" && (
                  <button
                    className="message-delete"
                    onClick={() =>
                      onConfirm({
                        title: labels.deleteMessageTitle,
                        detail: labels.deleteMessageDetail,
                        action: labels.deleteMessageAction,
                        run: () =>
                          workspace.dispatch({
                            type: "message.deleteLocal",
                            ids: [message.id],
                          }),
                      })
                    }
                    aria-label={labels.deleteLocalMessage}
                  >
                    {labels.delete}
                  </button>
                )}
              </span>
            </div>
          </article>
        ))}
      </div>
      {state.transfers.some((item) => ACTIVE_TRANSFER_STATES.has(item.status)) && (
        <div className="transfer-dock">
          <Icon name="download" />
          <span>
            <b>
              {labels.activeTransfers(
                state.transfers.filter((item) => ACTIVE_TRANSFER_STATES.has(item.status))
                  .length,
              )}
            </b>
            <small>{labels.openFileCenterProgress}</small>
          </span>
        </div>
      )}
      <Composer
        state={state}
        conversation={conversation}
        workspace={workspace}
        labels={labels}
      />
    </main>
  );
}

function HostWorkspace({
  state,
  workspace,
  labels,
  selectedId,
  onBack,
  onRemark,
  onChat,
  onConfirm,
}) {
  const device = state.devices.find((item) => item.id === selectedId);
  if (!device) {
    return (
      <main className="workspace host-workspace no-selection">
        <EmptyState
          icon="hosts"
          title={labels.selectHost}
          detail={labels.selectHostHint}
        />
      </main>
    );
  }
  return (
    <main className="workspace host-workspace" data-od-id="host-workspace">
      <header className="workspace-head">
        <button
          className="mobile-back icon-button"
          onClick={onBack}
          aria-label={labels.backHostList}
        >
          <Icon name="back" />
        </button>
        <div className="workspace-heading">
          <b>{displayName(device, labels)}</b>
          <span>
            {device.is_offline ? labels.offline : labels.online} ·{" "}
            {sourceText(device.discovery_source, labels) || labels.unknownDiscovery}
          </span>
        </div>
      </header>
      <div className="host-detail">
        <section className="host-identity">
          <Avatar entity={device} labels={labels} large />
          <div>
            <h1>{displayName(device, labels)}</h1>
            <p>{device.hostname || device.name || labels.unknownHostname}</p>
            <span className={device.is_offline ? "presence-label offline" : "presence-label"}>
              {device.is_offline ? labels.offline : labels.online}
            </span>
          </div>
          <div className="host-actions">
            <button className="primary-button" onClick={() => onChat(device.id)}>
              {labels.sendMessage}
            </button>
            <button
              className="secondary-button"
              onClick={onRemark}
              disabled={!state.capabilities.deviceMetadata}
            >
              {labels.editRemark}
            </button>
          </div>
        </section>
        <section className="detail-section">
          <h2>{labels.deviceIdentity}</h2>
          <dl className="detail-grid">
            <div><dt>{labels.hostname}</dt><dd>{device.hostname || labels.notProvided}</dd></div>
            <div>
              <dt>{labels.currentAddress}</dt>
              <dd className="numeric">{device.addr || labels.notProvided}</dd>
            </div>
            <div>
              <dt>{device.mac_address ? labels.macAddress : labels.deviceId}</dt>
              <dd className="numeric">{device.mac_address || device.id}</dd>
            </div>
            <div>
              <dt>{labels.discoveryMethod}</dt>
              <dd>{sourceText(device.discovery_source, labels)}</dd>
            </div>
            <div>
              <dt>{labels.lastOnline}</dt>
              <dd>{formatTime(device.last_seen, labels.locale) || labels.unknown}</dd>
            </div>
            <div>
              <dt>{labels.availableMemory}</dt>
              <dd>
                {device.available_memory_mb
                  ? `${device.available_memory_mb} MB`
                  : labels.notProvided}
              </dd>
            </div>
          </dl>
        </section>
        <section className="danger-zone">
          <div>
            <b>{labels.deleteLocalContact}</b>
            <span>{labels.deleteLocalContactHint}</span>
          </div>
          <button
            className="danger-button"
            disabled={!device.is_offline}
            onClick={() =>
              onConfirm({
                title: labels.deleteDeviceTitle(displayName(device, labels)),
                detail: labels.deleteDeviceDetail,
                action: labels.deleteDeviceAction,
                run: () => workspace.dispatch({ type: "device.remove", id: device.id }),
              })
            }
          >
            {labels.deleteDevice}
          </button>
        </section>
      </div>
    </main>
  );
}

function FileActions({
  file,
  state,
  workspace,
  labels,
  onDelete,
  onPreview,
}) {
  const available = localFileAvailable(file);
  const previewable = available && fileKind(file) !== "other";
  const canDelete = available && file.direction === "incoming";
  return (
    <div className="file-actions">
      <button
        className="icon-button"
        disabled={!previewable}
        onClick={() => onPreview(file)}
        aria-label={labels.previewFile}
        title={previewable ? labels.previewFile : labels.localFileUnavailable}
      >
        <Icon name="eye" />
      </button>
      <button
        className="icon-button"
        disabled={!available || !state.capabilities.revealFile}
        onClick={() => workspace.dispatch({ type: "file.reveal", file })}
        aria-label={labels.revealFile}
        title={available ? labels.revealFile : labels.localFileUnavailable}
      >
        <Icon name="folder" />
      </button>
      <button
        className="icon-button danger-text"
        disabled={!canDelete}
        onClick={() => onDelete(file)}
        aria-label={labels.deleteLocalCopy}
        title={canDelete ? labels.deleteLocalCopy : labels.localFileUnavailable}
      >
        <Icon name="trash" />
      </button>
    </div>
  );
}

function FileWorkspace({ state, workspace, labels, query, filter, onBack, onConfirm }) {
  const [kind, setKind] = useState("all");
  const [preview, setPreview] = useState(null);
  const text = query.trim().toLocaleLowerCase();
  const files = state.files.filter((file) => {
    const source = sourceForFile(file, state);
    const matchesText = `${file.file_name || file.name} ${displayName(source, labels)}`
      .toLocaleLowerCase()
      .includes(text);
    return (
      matchesText &&
      fileMatchesSource(file, filter, state) &&
      (kind === "all" || fileKind(file) === kind)
    );
  });
  const selectedSource =
    filter === "all"
      ? null
      : fileSources(state).find((source) => source.id === filter)?.entity;
  const title = selectedSource
    ? displayName(selectedSource, labels)
    : kind === "all"
      ? labels.fileFilters.all
      : `${labels.fileKinds[kind]}${labels.files}`;
  const previewFile = (file) => {
    if (localFileAvailable(file) && fileKind(file) !== "other") setPreview(file);
  };
  return (
    <main className="workspace file-workspace" data-od-id="shared-file-center">
      <header className="workspace-head">
        <button
          className="mobile-back icon-button"
          onClick={onBack}
          aria-label={labels.backFileFilters}
        >
          <Icon name="back" />
        </button>
        <div className="workspace-heading">
          <b>{title}</b>
          <span>
            {files.length} {labels.locale === "en" ? "items" : "个项目"}
            {selectedSource
              ? ` · ${
                  selectedSource.kind === "group"
                    ? labels.sourceGroup
                    : labels.sourceDevice
                }`
              : ""}
          </span>
        </div>
        <button
          className="icon-button"
          onClick={() => workspace.dispatch({ type: "refresh" })}
          aria-label={labels.refresh}
          title={labels.refresh}
        >
          <Icon name="refresh" />
        </button>
      </header>
      <div className="file-kind-bar" aria-label={labels.fileCenter}>
        {FILE_KINDS.map((value) => (
          <button
            className={`kind-chip ${kind === value ? "active" : ""}`}
            type="button"
            key={value}
            onClick={() => setKind(value)}
          >
            {labels.fileKinds[value]}
          </button>
        ))}
      </div>
      <div className={`file-content ${!files.length ? "has-empty-state" : ""}`}>
        {files.length ? (
          <section className="file-table">
            <div className="file-table-head">
              <span>{labels.file}</span>
              <span>{labels.sourceTarget}</span>
              <span>{labels.time}</span>
              <span>{labels.size}</span>
              <span>{labels.actions}</span>
            </div>
            {files.map((file) => {
              const source = sourceForFile(file, state);
              const type = fileKind(file);
              const available = localFileAvailable(file);
              return (
              <div
                className={`file-row ${!available ? "invalid" : ""}`}
                key={file.id || file.message_id}
                data-od-id={`file-${file.id || file.message_id}`}
                onDoubleClick={() =>
                  type === "other"
                    ? available &&
                      state.capabilities.revealFile &&
                      workspace.dispatch({ type: "file.reveal", file })
                    : previewFile(file)
                }
              >
                <span className={`file-type ${type}`}>
                  <Icon name={type === "document" || type === "other" ? "file" : type} />
                </span>
                <div className="file-primary file-cell">
                  <b>{file.file_name || file.name || labels.unnamedFile}</b>
                  <span>
                    {!available
                      ? labels.localFileUnavailable
                      : file.direction === "outgoing"
                        ? labels.sentDirection
                        : labels.receivedDirection}
                  </span>
                </div>
                <span className="file-source file-source-col">
                  <Avatar entity={source} labels={labels} />
                  <span>{displayName(source, labels)}</span>
                </span>
                <time className="file-time-col">
                  {formatTime(file.timestamp || file.updated_at, labels.locale)}
                </time>
                <span className="numeric file-size-col">
                  {formatSize(file.file_size || file.bytes_total)}
                </span>
                <FileActions
                  file={file}
                  state={state}
                  workspace={workspace}
                  labels={labels}
                  onPreview={previewFile}
                  onDelete={(target) =>
                    onConfirm({
                      title: labels.deleteFileTitle(
                        target.file_name || target.name || labels.unnamedFile,
                      ),
                      detail: labels.deleteFileDetail,
                      action: labels.deleteFileAction,
                      run: () => workspace.dispatch({ type: "file.deleteLocalCopy", file: target }),
                    })
                  }
                />
              </div>
              );
            })}
          </section>
        ) : (
          <EmptyState
            icon="files"
            title={
              state.files.length
                ? labels.noMatchingFiles
                : labels.noFiles
            }
            detail={
              state.files.length
                ? labels.noMatchingFilesHint
                : labels.noFilesHint
            }
          />
        )}
      </div>
      {preview && (
        <FilePreviewModal
          file={preview}
          state={state}
          workspace={workspace}
          labels={labels}
          onClose={() => setPreview(null)}
          onDelete={() => {
            setPreview(null);
            onConfirm({
              title: labels.deleteFileTitle(
                preview.file_name || preview.name || labels.unnamedFile,
              ),
              detail: labels.deleteFileDetail,
              action: labels.deleteFileAction,
              run: () =>
                workspace.dispatch({
                  type: "file.deleteLocalCopy",
                  file: preview,
                }),
            });
          }}
        />
      )}
    </main>
  );
}

function FilePreviewModal({
  file,
  state,
  workspace,
  labels,
  onClose,
  onDelete,
}) {
  const kind = fileKind(file);
  const media = useMessageMedia(file, workspace, kind === "image");
  const source = sourceForFile(file, state);
  return (
    <Modal
      title={file.file_name || file.name || labels.unnamedFile}
      onClose={onClose}
      closeLabel={labels.close}
      wide
      actions={
        <>
          <button
            className="secondary-button"
            disabled={!state.capabilities.revealFile}
            onClick={() => workspace.dispatch({ type: "file.reveal", file })}
          >
            {labels.revealFile}
          </button>
          <button
            className="danger-button"
            disabled={file.direction !== "incoming"}
            onClick={onDelete}
          >
            {labels.deleteFileAction}
          </button>
        </>
      }
    >
      <div className={`preview-stage ${kind}`}>
        {kind === "image" && media.url && (
          <img
            src={media.url}
            alt={file.file_name || file.name || labels.imagePreview}
          />
        )}
        {kind === "image" && !media.url && (
          <div className="media-preview">
            <Icon name="image" size={64} />
            <span>
              {media.failed
                ? labels.localFileUnavailable
                : labels.imagePreview}
            </span>
          </div>
        )}
        {kind === "document" && (
          <div className="preview-document">
            <h3>{file.file_name || file.name}</h3>
            <div className="preview-lines">
              {Array.from({ length: 6 }, (_, index) => <i key={index} />)}
            </div>
            <p className="preview-note">{labels.documentPreview}</p>
          </div>
        )}
        {(kind === "audio" || kind === "video" || kind === "other") && (
          <div className="media-preview">
            <span className="media-cover">
              <Icon
                name={kind === "other" ? "file" : kind}
                size={64}
              />
            </span>
            <b>{file.file_name || file.name}</b>
            <span className="preview-note">
              {kind === "audio"
                ? labels.audioPreview
                : kind === "video"
                  ? labels.videoPreview
                  : labels.previewUnavailable}
            </span>
          </div>
        )}
      </div>
      <p className="preview-meta">
        {formatSize(file.file_size)} ·{" "}
        {formatTime(file.timestamp || file.updated_at, labels.locale)} ·{" "}
        {displayName(source, labels)}
      </p>
    </Modal>
  );
}

function SettingRow({ label, detail, children }) {
  return (
    <label className="setting-row">
      <span><b>{label}</b>{detail && <small>{detail}</small>}</span>
      {children}
    </label>
  );
}

function SettingsWorkspace({
  state,
  workspace,
  labels,
  onBack,
  onLanguagePreview,
  onActiveSection,
}) {
  const [form, setForm] = useState(state.settings);
  const [dirty, setDirty] = useState(false);
  const scroll = useRef(null);
  useEffect(() => {
    if (!dirty) setForm(state.settings);
  }, [dirty, state.settings]);
  useEffect(
    () => () => onLanguagePreview(null),
    [onLanguagePreview],
  );
  const syncSection = () => {
    const root = scroll.current;
    if (!root) return;
    const rootTop = root.getBoundingClientRect().top;
    let active = Object.keys(labels.settingsSections)[0];
    for (const id of Object.keys(labels.settingsSections)) {
      const element = document.getElementById(`settings-${id}`);
      if (element && element.getBoundingClientRect().top - rootTop <= 72) {
        active = id;
      }
    }
    onActiveSection(active);
  };
  const change = (key, value) => {
    setDirty(true);
    setForm((current) => ({ ...current, [key]: value }));
  };
  const avatars = ["🐼", "🦊", "🐧", "🐰", "🐯", "🐸", "🐨", "🦁"];
  return (
    <main className="workspace settings-workspace" data-od-id="settings-workspace">
      <header className="workspace-head">
        <button
          className="mobile-back icon-button"
          onClick={onBack}
          aria-label={labels.backSettingsList}
        >
          <Icon name="back" />
        </button>
        <div className="workspace-heading">
          <b>{labels.settings}</b>
          <span>{labels.settingsSubtitle}</span>
        </div>
        <button
          className="primary-button"
          onClick={async () => {
            const patch = Object.fromEntries(
              [
                "name",
                "avatar",
                "theme",
                "language",
                "notifications_enabled",
                "download_path",
                "auto_download",
                "port",
                "db_path",
                "capture_shortcut",
              ]
                .filter((key) => form[key] !== state.settings[key])
                .map((key) => [key, form[key]]),
            );
            const result = await workspace.dispatch({ type: "settings.patch", patch });
            if (result.ok) setDirty(false);
          }}
          disabled={!dirty}
        >
          {labels.saveSettings}
        </button>
      </header>
      <div className="settings-scroll" ref={scroll} onScroll={syncSection}>
        <section className="settings-section" id="settings-identity">
          <h2>{labels.identity}</h2>
          <SettingRow
            label={labels.localName}
            detail={
              state.self.id
                ? labels.deviceIdDetail(state.self.id)
                : labels.deviceIdUnavailable
            }
          >
            <input value={form.name} onChange={(event) => change("name", event.target.value)} />
          </SettingRow>
          <div className="avatar-setting">
            <span>
              <b>{labels.localAvatar}</b>
              <small>{labels.localAvatarHint}</small>
            </span>
            <div className="avatar-picker">
              {avatars.map((avatar) => (
                <button
                  className={form.avatar === avatar ? "selected" : ""}
                  onClick={() => change("avatar", avatar)}
                  key={avatar}
                  type="button"
                  aria-label={labels.chooseAvatar(avatar)}
                >
                  {avatar}
                </button>
              ))}
            </div>
          </div>
        </section>
        <section className="settings-section" id="settings-appearance">
          <h2>{labels.appearance}</h2>
          <SettingRow label={labels.theme}>
            <select value={form.theme} onChange={(event) => change("theme", event.target.value)}>
              <option value="system">{labels.systemTheme}</option>
              <option value="light">{labels.lightTheme}</option>
              <option value="dark">{labels.darkTheme}</option>
            </select>
          </SettingRow>
          <SettingRow label={labels.language}>
            <select
              value={form.language}
              onChange={(event) => {
                change("language", event.target.value);
                onLanguagePreview(event.target.value);
              }}
            >
              <option value="zh-CN">{labels.simplifiedChinese}</option>
              <option value="en">{labels.english}</option>
            </select>
          </SettingRow>
        </section>
        <section className="settings-section" id="settings-notification">
          <h2>{labels.notification}</h2>
          <SettingRow
            label={labels.newMessageNotification}
            detail={
              state.capabilities.notifications
                ? labels.permissionManagedBySystem
                : labels.platformUnavailable
            }
          >
            <input
              type="checkbox"
              checked={form.notifications_enabled}
              disabled={!state.capabilities.notifications}
              onChange={(event) => change("notifications_enabled", event.target.checked)}
            />
          </SettingRow>
        </section>
        <section className="settings-section" id="settings-download">
          <h2>{labels.downloadsAndTransfers}</h2>
          <SettingRow label={labels.downloadPath}>
            <input value={form.download_path} onChange={(event) => change("download_path", event.target.value)} />
          </SettingRow>
          <SettingRow
            label={labels.autoReceiveFiles}
            detail={labels.autoReceiveFilesHint}
          >
            <input type="checkbox" checked={form.auto_download} onChange={(event) => change("auto_download", event.target.checked)} />
          </SettingRow>
        </section>
        <section className="settings-section" id="settings-network">
          <h2>{labels.network}</h2>
          <SettingRow label={labels.serverPort} detail={labels.restartRequired}>
            <input className="numeric" inputMode="numeric" value={form.port} onChange={(event) => change("port", event.target.value)} />
          </SettingRow>
          <SettingRow label={labels.databasePath} detail={labels.restartRequired}>
            <input className="numeric" value={form.db_path} onChange={(event) => change("db_path", event.target.value)} />
          </SettingRow>
        </section>
        <section className="settings-section" id="settings-shortcut">
          <h2>{labels.shortcuts}</h2>
          <SettingRow
            label={labels.captureShortcut}
            detail={
              state.capabilities.captureShortcut
                ? `${labels.captureShortcutFocusedHint} · ${labels.captureShortcutHint}`
                : labels.platformUnavailable
            }
          >
            <input
              className="numeric"
              value={form.capture_shortcut}
              readOnly
              disabled={!state.capabilities.captureShortcut}
              onKeyDown={(event) => {
                const shortcut = shortcutLabelFromEvent(event);
                if (!shortcut) return;
                event.preventDefault();
                change("capture_shortcut", shortcut);
              }}
            />
          </SettingRow>
        </section>
      </div>
    </main>
  );
}

function InfoPanel({ state, conversation, workspace, labels, onRemark, onConfirm }) {
  if (!conversation) return null;
  const peer = conversation.peer;
  const group = conversation.kind === "group";
  return (
    <aside className="info-panel" data-od-id="conversation-information">
      <div className="info-identity">
        <Avatar entity={peer || conversation} labels={labels} large />
        <b>{displayName(conversation, labels)}</b>
        <span>
          {group
            ? labels.groupChat
            : peer?.is_offline
              ? labels.offline
              : labels.online}
        </span>
      </div>
      {group ? (
        <section className="info-section">
          <h2>{labels.groupMembers(conversation.members?.length || 0)}</h2>
          {(conversation.members || []).map((member) => (
            <div className="member-row" key={member.peer_id || member.id}>
              <Avatar
                entity={{
                  id: member.peer_id || member.id,
                  name: member.display_name || member.name,
                }}
                labels={labels}
              />
              <span><b>{member.display_name || member.name}</b><small>{member.peer_id || member.id}</small></span>
            </div>
          ))}
        </section>
      ) : (
        <>
          <dl className="info-section info-kv">
            <div>
              <dt>{labels.hostname}</dt>
              <dd>{peer?.hostname || peer?.name || labels.notProvided}</dd>
            </div>
            <div>
              <dt>{labels.currentAddress}</dt>
              <dd>{peer?.addr || labels.notProvided}</dd>
            </div>
            <div>
              <dt>{peer?.mac_address ? labels.macAddress : labels.deviceId}</dt>
              <dd>{peer?.mac_address || peer?.id}</dd>
            </div>
            <div>
              <dt>{labels.discoveryMethod}</dt>
              <dd>{sourceText(peer?.discovery_source, labels)}</dd>
            </div>
          </dl>
          <div className="info-actions">
            <button onClick={onRemark} disabled={!state.capabilities.deviceMetadata}>
              {labels.editDeviceRemark}
            </button>
            <button
              className="danger-text"
              onClick={() =>
                onConfirm({
                  title: labels.clearHistoryTitle,
                  detail: labels.clearHistoryShortDetail,
                  action: labels.clearHistoryAction,
                  run: () => workspace.dispatch({ type: "message.clearConversation" }),
                })
              }
            >
              {labels.clearHistory}
            </button>
          </div>
        </>
      )}
      <div className="info-actions">
        <button
          disabled={!state.capabilities.conversationState}
          onClick={() =>
            workspace.dispatch({
              type: "conversation.pin",
              id: conversation.id,
              value: !conversation.pinned,
            })
          }
        >
          {conversation.pinned
            ? labels.unpinConversation
            : labels.pinConversation}
        </button>
        <button
          disabled={!state.capabilities.conversationState}
          onClick={() =>
            workspace.dispatch({
              type: "conversation.markUnread",
              id: conversation.id,
              value: !conversation.forced_unread,
            })
          }
        >
          {conversation.forced_unread
            ? labels.unmarkUnread
            : labels.markUnread}
        </button>
      </div>
    </aside>
  );
}

function Modal({ title, children, actions, onClose, closeLabel, wide = false }) {
  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <section className={`modal ${wide ? "modal-wide" : ""}`} role="dialog" aria-modal="true" aria-label={title}>
        <header>
          <b>{title}</b>
          <button className="icon-button" onClick={onClose} aria-label={closeLabel}>
            <Icon name="close" />
          </button>
        </header>
        <div className="modal-body">{children}</div>
        {actions && <footer>{actions}</footer>}
      </section>
    </div>
  );
}

function GroupModal({ state, workspace, labels, onClose }) {
  const [title, setTitle] = useState("");
  const [members, setMembers] = useState([]);
  const create = async () => {
    if (!title.trim() || members.length < 2) return;
    const result = await workspace.dispatch({
      type: "conversation.createGroup",
      title: title.trim(),
      memberIds: members,
    });
    if (result.ok) onClose();
  };
  return (
    <Modal
      title={labels.newGroup}
      onClose={onClose}
      closeLabel={labels.close}
      actions={
        <>
          <button className="secondary-button" onClick={onClose}>
            {labels.cancelAction}
          </button>
          <button
            className="primary-button"
            disabled={!title.trim() || members.length < 2}
            onClick={create}
          >
            {labels.createGroup}
          </button>
        </>
      }
    >
      <label className="field">
        <span>{labels.groupName}</span>
        <input
          value={title}
          onChange={(event) => setTitle(event.target.value)}
          autoFocus
        />
      </label>
      <p className="helper">{labels.groupHelper}</p>
      <div className="member-picker">
        {state.devices.map((device) => (
          <label className="member-row selectable" key={device.id}>
            <input
              type="checkbox"
              checked={members.includes(device.id)}
              onChange={(event) =>
                setMembers((current) =>
                  event.target.checked
                    ? [...current, device.id]
                    : current.filter((id) => id !== device.id),
                )
              }
            />
            <Avatar entity={device} labels={labels} />
            <span>
              <b>{displayName(device, labels)}</b>
              <small>{device.addr || device.id}</small>
            </span>
          </label>
        ))}
      </div>
    </Modal>
  );
}

function EndpointModal({ state, workspace, labels, onClose }) {
  const [endpoint, setEndpoint] = useState("");
  const valid = /^([a-zA-Z0-9.-]+)(:\d{1,5})?$/.test(endpoint.trim());
  return (
    <Modal
      title={labels.addDevice}
      onClose={onClose}
      closeLabel={labels.close}
      actions={
        <>
          <button className="secondary-button" onClick={onClose}>
            {labels.cancelAction}
          </button>
          <button
            className="primary-button"
            disabled={!valid}
            onClick={async () => {
              const result = await workspace.dispatch({
                type: "device.saveEndpoint",
                endpoint: endpoint.trim(),
              });
              if (result.ok) onClose();
            }}
          >
            {labels.addDevice}
          </button>
        </>
      }
    >
      <label className="field">
        <span>{labels.deviceAddress}</span>
        <input
          className="numeric"
          value={endpoint}
          onChange={(event) => setEndpoint(event.target.value)}
          placeholder={labels.endpointPlaceholder}
          autoFocus
        />
      </label>
      <p className="helper">{labels.endpointHelper}</p>
      {state.settings.custom_peers?.length > 0 && (
        <div className="endpoint-list">
          {state.settings.custom_peers.map((peer) => (
            <div key={peer}>
              <span className="numeric">{peer}</span>
              <button
                className="text-action danger-text"
                onClick={() =>
                  workspace.dispatch({
                    type: "device.removeEndpoint",
                    endpoint: peer,
                  })
                }
              >
                {labels.delete}
              </button>
            </div>
          ))}
        </div>
      )}
    </Modal>
  );
}

function RemarkModal({ device, workspace, labels, onClose }) {
  const [remark, setRemark] = useState(device?.remark || "");
  if (!device) return null;
  return (
    <Modal
      title={labels.editDeviceRemark}
      onClose={onClose}
      closeLabel={labels.close}
      actions={
        <>
          <button className="secondary-button" onClick={onClose}>
            {labels.cancelAction}
          </button>
          <button
            className="primary-button"
            disabled={!remark.trim()}
            onClick={async () => {
              const result = await workspace.dispatch({
                type: "device.saveRemark",
                id: device.id,
                remark: remark.trim(),
              });
              if (result.ok) onClose();
            }}
          >
            {labels.saveRemark}
          </button>
        </>
      }
    >
      <label className="field"><span>{device.hostname || device.name} · {device.mac_address || device.id}</span><input value={remark} onChange={(event) => setRemark(event.target.value)} autoFocus /></label>
      <p className="helper">{labels.remarkHelper}</p>
    </Modal>
  );
}

function ConfirmModal({ confirm, labels, onClose }) {
  return (
    <Modal
      title={confirm.title}
      onClose={onClose}
      closeLabel={labels.close}
      actions={
        <>
          <button className="secondary-button" onClick={onClose}>
            {labels.cancelAction}
          </button>
          <button
            className="danger-button"
            onClick={async () => {
              await confirm.run();
              onClose();
            }}
          >
            {confirm.action}
          </button>
        </>
      }
    >
      <p>{confirm.detail}</p>
    </Modal>
  );
}

export default function App({ workspace }) {
  const requestedView = new URLSearchParams(globalThis.location?.search || "").get("view");
  if (requestedView === "capture-editor" || requestedView === "capture-pin") {
    return <CaptureEditor workspace={workspace} mode={requestedView === "capture-pin" ? "pin" : "editor"} />;
  }
  const state = useSyncExternalStore(workspace.subscribe, workspace.getSnapshot);
  const savedLanguage = state.settings.language === "en" ? "en" : "zh-CN";
  const [languagePreview, setLanguagePreview] = useState(null);
  const language = languagePreview || savedLanguage;
  const labels = copy[language];
  const [query, setQuery] = useState("");
  const [selectedDeviceId, setSelectedDeviceId] = useState("");
  const [fileFilter, setFileFilter] = useState("all");
  const [settingsSection, setSettingsSection] = useState("identity");
  const [infoOpen, setInfoOpen] = useState(false);
  const [mobileList, setMobileList] = useState(false);
  const [modal, setModal] = useState(null);
  const [confirm, setConfirm] = useState(null);

  useTheme(state.settings.theme);

  useEffect(() => {
    if (languagePreview === savedLanguage) setLanguagePreview(null);
  }, [languagePreview, savedLanguage]);

  useEffect(() => {
    document.documentElement.lang = language;
  }, [language]);

  useEffect(() => {
    if (!state.capabilities.captureShortcut) return;
    const onKeyDown = (event) => {
      if (
        event.target instanceof HTMLInputElement ||
        event.target instanceof HTMLTextAreaElement ||
        event.target instanceof HTMLSelectElement
      ) {
        return;
      }
      if (matchesShortcut(event, state.settings.capture_shortcut)) {
        event.preventDefault();
        workspace.dispatch({ type: "capture.start" });
      }
    };
    addEventListener("keydown", onKeyDown);
    return () => removeEventListener("keydown", onKeyDown);
  }, [state.capabilities.captureShortcut, state.settings.capture_shortcut, workspace]);

  useEffect(() => {
    if (!globalThis.BroadcastChannel) return;
    const channel = new BroadcastChannel("xchat-capture");
    channel.onmessage = ({ data }) => {
      if (data?.type === "capture-ready" && data.attachment) {
        workspace.dispatch({
          type: "draft.addManaged",
          conversationId: data.attachment.conversation_id,
          attachment: data.attachment,
        });
      }
    };
    return () => channel.close();
  }, [workspace]);

  useEffect(() => {
    workspace.dispatch({ type: "bootstrap" });
  }, [workspace]);

  useEffect(() => {
    if (!selectedDeviceId && state.devices[0]) setSelectedDeviceId(state.devices[0].id);
  }, [selectedDeviceId, state.devices]);

  useEffect(() => {
    if (
      fileFilter !== "all" &&
      !fileSources(state).some((source) => source.id === fileFilter)
    ) {
      setFileFilter("all");
    }
  }, [fileFilter, state.files, state.conversations, state.devices]);

  useEffect(() => {
    setInfoOpen(false);
  }, [state.activeConversationId]);

  useEffect(() => {
    if (state.activeSection !== "chat" || !state.capabilities.messageSearch) return;
    const timer = setTimeout(
      () => workspace.dispatch({ type: "message.search", query }),
      query.trim().length >= 2 ? 250 : 0,
    );
    return () => clearTimeout(timer);
  }, [query, state.activeSection, state.capabilities.messageSearch, workspace]);

  useEffect(() => {
    const notice = state.notices.at(-1);
    if (!notice) return;
    const timer = setTimeout(
      () => workspace.dispatch({ type: "notice.dismiss", id: notice.id }),
      3500,
    );
    return () => clearTimeout(timer);
  }, [state.notices, workspace]);

  useEffect(() => {
    if (!modal && !confirm) return;
    const close = (event) => {
      if (event.key === "Escape") {
        setModal(null);
        setConfirm(null);
      }
    };
    addEventListener("keydown", close);
    return () => removeEventListener("keydown", close);
  }, [confirm, modal]);

  const conversation = state.conversations.find(
    (item) => item.id === state.activeConversationId,
  );
  const selectedDevice = state.devices.find((item) => item.id === selectedDeviceId);
  const shellClass = [
    "app-shell",
    state.activeSection === "chat" && infoOpen && conversation ? "has-info" : "",
    mobileList ? "mobile-list" : "",
  ].join(" ");

  const openSection = (section) => {
    setQuery("");
    setMobileList(true);
    workspace.dispatch({ type: "navigation.open", section });
  };

  const openConversation = (id, target = {}) => {
    if (!id) return;
    setMobileList(false);
    setInfoOpen(false);
    workspace.dispatch({ type: "conversation.open", id, ...target });
  };

  const openSettingsSection = (id) => {
    setSettingsSection(id);
    setMobileList(false);
    requestAnimationFrame(() =>
      document
        .getElementById(`settings-${id}`)
        ?.scrollIntoView({ behavior: "smooth", block: "start" }),
    );
  };

  const chatWithDevice = (deviceId) => {
    const direct = state.conversations.find(
      (item) => item.kind === "direct" && item.peer_id === deviceId,
    );
    if (direct) openConversation(direct.id);
  };

  return (
    <section className={shellClass} data-od-id="xchat-desktop-app">
      <Rail state={state} labels={labels} onOpen={openSection} />
      <ListPane
        state={state}
        labels={labels}
        query={query}
        setQuery={setQuery}
        selectedDeviceId={selectedDeviceId}
        fileFilter={fileFilter}
        onConversation={openConversation}
        onDevice={(id) => { setSelectedDeviceId(id); setMobileList(false); }}
        onAdd={() => setModal(state.activeSection === "chat" ? "group" : "endpoint")}
        onFileFilter={(value) => { setFileFilter(value); setMobileList(false); }}
        onCloseMobile={() => setMobileList(false)}
        settingsSection={settingsSection}
        onSettingsSection={openSettingsSection}
      />
      {state.activeSection === "chat" && (
        <ChatWorkspace
          state={state}
          workspace={workspace}
          labels={labels}
          onBack={() => setMobileList(true)}
          infoOpen={infoOpen}
          onToggleInfo={() => setInfoOpen((value) => !value)}
          onConfirm={setConfirm}
        />
      )}
      {state.activeSection === "hosts" && (
        <HostWorkspace
          state={state}
          workspace={workspace}
          labels={labels}
          selectedId={selectedDeviceId}
          onBack={() => setMobileList(true)}
          onRemark={() => setModal("remark")}
          onChat={chatWithDevice}
          onConfirm={setConfirm}
        />
      )}
      {state.activeSection === "files" && (
        <FileWorkspace
          state={state}
          workspace={workspace}
          labels={labels}
          query={query}
          filter={fileFilter}
          onBack={() => setMobileList(true)}
          onConfirm={setConfirm}
        />
      )}
      {state.activeSection === "settings" && (
        <SettingsWorkspace
          state={state}
          workspace={workspace}
          labels={labels}
          onBack={() => setMobileList(true)}
          onLanguagePreview={setLanguagePreview}
          onActiveSection={setSettingsSection}
        />
      )}
      {state.activeSection === "chat" && infoOpen && (
        <InfoPanel
          state={state}
          conversation={conversation}
          workspace={workspace}
          labels={labels}
          onRemark={() => {
            if (conversation?.peer_id) {
              setSelectedDeviceId(conversation.peer_id);
              setModal("remark");
            }
          }}
          onConfirm={setConfirm}
        />
      )}
      {state.phase !== "ready" && (
        <div className={`connection-banner ${state.phase}`}>
          <span>
            {state.phase === "booting"
              ? labels.connecting
              : state.phase === "offline"
                ? labels.reconnecting
                : labels.connectionFailed}
          </span>
          {state.phase !== "booting" && (
            <button onClick={() => workspace.dispatch({ type: "refresh" })}>
              {labels.retry}
            </button>
          )}
        </div>
      )}
      {state.notices.at(-1) && (
        <div className={`toast ${state.notices.at(-1).kind}`} role="status">
          <i />
          {state.notices.at(-1).message}
        </div>
      )}
      {modal === "group" && (
        <GroupModal
          state={state}
          workspace={workspace}
          labels={labels}
          onClose={() => setModal(null)}
        />
      )}
      {modal === "endpoint" && (
        <EndpointModal
          state={state}
          workspace={workspace}
          labels={labels}
          onClose={() => setModal(null)}
        />
      )}
      {modal === "remark" && (
        <RemarkModal
          device={selectedDevice}
          workspace={workspace}
          labels={labels}
          onClose={() => setModal(null)}
        />
      )}
      {confirm && (
        <ConfirmModal
          confirm={confirm}
          labels={labels}
          onClose={() => setConfirm(null)}
        />
      )}
    </section>
  );
}
