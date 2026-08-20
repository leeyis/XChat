import {
  Fragment,
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import {
  ACTIVE_TRANSFER_STATES,
  EMOJI_SET,
  avatarText,
  canSaveVerifiedEndpoint,
  fileKind,
  fileStatus,
  groupMentionCandidates,
  groupAvatarRows,
  insertTextAtSelection,
  isAppActive,
  isCopyableMessage,
  isImageFile,
  isPhysicalPointInsideRect,
  localFileAvailable,
  messageDeliveryStatus,
  matchesShortcut,
  mentionQueryAtCaret,
  mentionToken,
  nativeClipboardPaths,
  nativeCaptureShortcutAvailable,
  nativeDragDropTarget,
  discoveryInterfaceState,
  discoverySummary,
  formatMessageTime,
  messageTimeDividerIndices,
  recommendedDiscoverySettings,
  retainedMentionIds,
  settingsFormDirty,
  settingsPatch,
  shortcutLabelFromEvent,
  validServerPort,
  withDiscoveryInterfaceSelection,
} from "./xchat.js";
import CaptureEditor from "./CaptureEditor.jsx";

const FILE_KINDS = ["all", "image", "document", "audio", "video", "other"];
const RECENT_EMOJI_KEY = "xchat.recentEmoji";

function readRecentEmoji() {
  try {
    const value = JSON.parse(globalThis.localStorage?.getItem(RECENT_EMOJI_KEY) || "[]");
    return Array.isArray(value) ? value.filter((emoji) => EMOJI_SET.includes(emoji)).slice(0, 6) : [];
  } catch {
    return [];
  }
}

function rememberEmoji(emoji, current = []) {
  const next = [emoji, ...current.filter((item) => item !== emoji)].slice(0, 6);
  globalThis.localStorage?.setItem(RECENT_EMOJI_KEY, JSON.stringify(next));
  return next;
}

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
      identity: { label: "身份", icon: "user" },
      appearance: { label: "外观", icon: "appearance" },
      notification: { label: "通知", icon: "bell" },
      download: { label: "下载与传输", icon: "download" },
      network: { label: "网络", icon: "network" },
      shortcut: { label: "快捷键", icon: "keyboard" },
      about: { label: "关于", icon: "info" },
    },
    aboutTitle: "关于 Xchat",
    aboutDescription: "局域网聊天与文件传输，数据保留在你的设备之间。",
    version: "版本",
    attachment: "附件",
    emoji: "表情",
    dropFilesHere: "松开即可添加到发送草稿",
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
    openMenu: "打开选项",
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
    macAddress: "网卡地址（辅助）",
    deviceId: "设备 ID",
    identityVerification: "身份核验",
    identityVerifiedCurrentAddress: "当前地址已确认属于此设备",
    identityOfflineStopped: "离线，未向旧地址发送",
    discoveryMethod: "发现方式",
    lastOnline: "最后在线",
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
    localIp: "本机 IP",
    refreshIp: "刷新 IP",
    selectIp: "选择 IP",
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
    chooseFolder: "选择文件夹",
    autoReceiveFiles: "自动接收文件",
    autoReceiveFilesHint: "关闭后文件停在待接收状态",
    maxParallelChannels: "最大并行通道",
    maxParallelChannelsAria: "文件传输最大并行通道",
    maxParallelChannelsDefault: "4（默认）",
    maxParallelChannelsHint: (channels) =>
      ({
        4: "兼顾兼容性与资源占用。保存后对新开始的传输生效；旧版设备会自动使用 4 个通道。",
        8: "同一时间传输更多数据块。保存后对新开始的传输生效；旧版设备会自动使用 4 个通道。",
        16: "适合高速网络与高性能存储，会增加 CPU 和磁盘占用。保存后对新开始的传输生效；旧版设备仍自动使用 4 个通道。",
      })[channels] ??
      "兼顾兼容性与资源占用。保存后对新开始的传输生效；旧版设备会自动使用 4 个通道。",
    network: "网络",
    serverPort: "服务端口",
    invalidServerPort: "请输入 1–65535 之间的整数端口",
    restartRequired: "重启后生效",
    databasePath: "数据库路径",
    deviceDiscovery: "设备发现",
    deviceDiscoverySubtitle: "选择 Xchat 用哪些网络自动发现和探测设备",
    discoveryNote: "这里只控制主动发现和在线探测，不会修改系统路由或代理设置。主动发现仅发送到已勾选的网络接口，不会沿默认路由或连续扫描地址段。代理 TUN 与虚拟网卡默认排除；已知设备消息仍按系统路由通信。",
    localDiscovery: "本地局域网发现",
    localDiscoveryHint: "通过 Wi-Fi 和有线网络发现同网段设备",
    vpnDiscovery: "异地组网 VPN 发现",
    vpnDiscoveryHint: "通过 WireGuard 等组网网络发现远端设备",
    vpnFixedHelper: "部分 VPN 不转发广播，可用固定地址完成首次连接。目标网段由 VPN 接管时，消息会按系统路由进入 VPN。",
    manageFixedPeers: "管理固定地址",
    discoveryNetworks: "参与设备发现的网络",
    discoverySummary: (enabled, paused, excluded) =>
      `已启用 ${enabled} 个${paused ? `，已暂停 ${paused} 个` : ""}，已排除 ${excluded} 个`,
    expand: "展开",
    collapse: "收起",
    adapterManagerHint: "物理网络与组网 VPN 默认开启，代理和虚拟接口默认排除。",
    refreshNetworkList: "刷新网络列表",
    refreshingNetworkList: "正在刷新网络列表",
    networkListUpdatedNow: "网络列表刚刚更新",
    networkListNotRefreshed: "网络列表来自当前设置快照",
    restoreRecommended: "恢复推荐设置",
    discoveryAllOff: "自动发现已暂停。已添加设备仍可通信；新设备需要手动添加。",
    noNetworkInterfaces: "暂未发现可用于设备发现的网络接口。",
    interfaceConnected: "接口已连接",
    interfaceDisconnected: "接口未连接",
    interfaceNoAddress: "暂无 IPv4 地址",
    discoveryCategories: {
      physical_lan: "物理局域网",
      mesh_vpn: "组网 VPN",
      proxy_tun: "代理 TUN",
      virtual_machine: "虚拟网卡",
      unknown: "未知接口",
    },
    recommended: "推荐",
    defaultExcluded: "默认排除",
    enableDiscoveryOn: (name) => `使用 ${name} 进行设备发现`,
    tunRiskTitle: (name) => `在 ${name} 上启用设备发现？`,
    tunRiskText: "这可能让发现流量进入代理网络。只有确认该接口用于设备互联时才开启。",
    keepOff: "保持关闭",
    enableAnyway: "仍然开启",
    shortcuts: "快捷键",
    captureShortcut: "截屏快捷键",
    captureShortcutHint: "点击输入框后按下字母或数字组合键",
    captureShortcutFocusedHint: "仅在 Xchat 窗口聚焦时生效",
    captureShortcutGlobalHint: "启动 Xchat 后全局生效，包括隐藏到托盘时",
    deviceInformation: "设备信息",
    conversationManagement: "会话管理",
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
    endpointHelper: "测试只核对设备身份，不会发送聊天内容。适用于跨 VLAN、WireGuard 或无法自动发现的设备。",
    endpointSubtitle: "先确认地址对应哪台 Xchat 设备，再保存",
    testConnection: "测试连接",
    testingConnection: "正在核对设备身份…",
    endpointTestSuccess: "身份确认成功，可以安全保存。",
    endpointTestMismatch: "此地址上的设备身份与已保存记录不一致，已停止使用该地址。",
    endpointTestFailed: "无法确认此地址上的 Xchat 设备。",
    savedFixedAddresses: "已保存的固定地址",
    fixedAddressSafety: "地址变化或身份不一致时会停止发送",
    identityConfirmed: "身份已确认",
    identityNeedsTest: "需要重新测试",
    retest: "重新测试",
    saveDevice: "保存设备",
    offlineSafetyTitle: "对方已离线",
    offlineSafetyText: "消息暂不发送，对方上线后自动发送。",
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
      sent: "已发出",
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
      identity: { label: "Identity", icon: "user" },
      appearance: { label: "Appearance", icon: "appearance" },
      notification: { label: "Notifications", icon: "bell" },
      download: { label: "Downloads & transfers", icon: "download" },
      network: { label: "Network", icon: "network" },
      shortcut: { label: "Shortcuts", icon: "keyboard" },
      about: { label: "About", icon: "info" },
    },
    aboutTitle: "About Xchat",
    aboutDescription: "LAN chat and file transfer that keeps your data between your devices.",
    version: "Version",
    attachment: "Attachment",
    emoji: "Emoji",
    dropFilesHere: "Drop to add files to the draft",
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
    openMenu: "Open options",
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
    macAddress: "Network address (auxiliary)",
    deviceId: "Device ID",
    identityVerification: "Identity verification",
    identityVerifiedCurrentAddress: "The current address is verified for this device",
    identityOfflineStopped: "Offline; nothing was sent to the old address",
    discoveryMethod: "Discovery method",
    lastOnline: "Last online",
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
    localIp: "Local IP",
    refreshIp: "Refresh IP",
    selectIp: "Select IP",
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
    chooseFolder: "Choose folder",
    autoReceiveFiles: "Automatically receive files",
    autoReceiveFilesHint: "When off, files wait for manual acceptance",
    maxParallelChannels: "Maximum parallel channels",
    maxParallelChannelsAria: "Maximum parallel file-transfer channels",
    maxParallelChannelsDefault: "4 (default)",
    maxParallelChannelsHint: (channels) =>
      ({
        4: "Balances compatibility and resource use. Applies to newly started transfers after saving; older devices automatically use 4 channels.",
        8: "Transfers more data chunks concurrently. Applies to newly started transfers after saving; older devices automatically use 4 channels.",
        16: "For fast networks and high-performance storage; uses more CPU and disk resources. Applies to newly started transfers after saving; older devices still use 4 channels.",
      })[channels] ??
      "Balances compatibility and resource use. Applies to newly started transfers after saving; older devices automatically use 4 channels.",
    network: "Network",
    serverPort: "Server port",
    invalidServerPort: "Enter an integer port from 1 to 65535",
    restartRequired: "Takes effect after restart",
    databasePath: "Database path",
    deviceDiscovery: "Device discovery",
    deviceDiscoverySubtitle: "Choose which networks Xchat uses to discover and probe devices",
    discoveryNote: "These controls affect only active discovery and presence probes; they do not change system routes or proxy settings. Discovery traffic is sent only through selected interfaces, never through a default route or a sequential address scan. Proxy TUN and virtual adapters are excluded by default; messages to known devices still follow system routing.",
    localDiscovery: "Local network discovery",
    localDiscoveryHint: "Discover devices on the same Wi-Fi or wired network",
    vpnDiscovery: "Mesh VPN discovery",
    vpnDiscoveryHint: "Discover remote devices through mesh networks such as WireGuard",
    vpnFixedHelper: "Some VPNs do not forward broadcasts. Use a fixed address for the first connection; messages follow system routing when the VPN owns the target network.",
    manageFixedPeers: "Manage fixed addresses",
    discoveryNetworks: "Networks used for discovery",
    discoverySummary: (enabled, paused, excluded) =>
      `${enabled} enabled${paused ? `, ${paused} paused` : ""}, ${excluded} excluded`,
    expand: "Expand",
    collapse: "Collapse",
    adapterManagerHint: "Physical networks and mesh VPNs are enabled by default; proxy and virtual adapters are excluded.",
    refreshNetworkList: "Refresh network list",
    refreshingNetworkList: "Refreshing network list",
    networkListUpdatedNow: "Network list updated just now",
    networkListNotRefreshed: "Network list is from the current settings snapshot",
    restoreRecommended: "Restore recommended settings",
    discoveryAllOff: "Automatic discovery is paused. Added devices can still communicate; add new devices manually.",
    noNetworkInterfaces: "No network interfaces are currently available for device discovery.",
    interfaceConnected: "Interface connected",
    interfaceDisconnected: "Interface disconnected",
    interfaceNoAddress: "No IPv4 address",
    discoveryCategories: {
      physical_lan: "Physical LAN",
      mesh_vpn: "Mesh VPN",
      proxy_tun: "Proxy TUN",
      virtual_machine: "Virtual adapter",
      unknown: "Unknown interface",
    },
    recommended: "Recommended",
    defaultExcluded: "Excluded by default",
    enableDiscoveryOn: (name) => `Use ${name} for device discovery`,
    tunRiskTitle: (name) => `Enable discovery on ${name}?`,
    tunRiskText: "Discovery traffic may enter the proxy network. Enable this only when the interface is intended for device-to-device connectivity.",
    keepOff: "Keep off",
    enableAnyway: "Enable anyway",
    shortcuts: "Shortcuts",
    captureShortcut: "Capture shortcut",
    captureShortcutHint: "Focus this field, then press a letter or number shortcut",
    captureShortcutFocusedHint: "Works only while the Xchat window is focused",
    captureShortcutGlobalHint: "Works globally while Xchat is running, including from the tray",
    deviceInformation: "Device information",
    conversationManagement: "Conversation management",
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
    endpointHelper: "The test verifies device identity only and sends no chat content. Use it across VLANs, WireGuard, or when automatic discovery is unavailable.",
    endpointSubtitle: "Verify which Xchat device is at this address before saving",
    testConnection: "Test connection",
    testingConnection: "Verifying device identity…",
    endpointTestSuccess: "Identity verified. This address is safe to save.",
    endpointTestMismatch: "The device at this address no longer matches the saved identity. This address has been disabled.",
    endpointTestFailed: "Could not verify an Xchat device at this address.",
    savedFixedAddresses: "Saved fixed addresses",
    fixedAddressSafety: "Sending stops if the address changes or identity does not match",
    identityConfirmed: "Identity verified",
    identityNeedsTest: "Needs another test",
    retest: "Test again",
    saveDevice: "Save device",
    offlineSafetyTitle: "Peer is offline",
    offlineSafetyText: "Messages will wait and send automatically when the peer is back online.",
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

function Icon({ name, size = 20, spin = false }) {
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
    case "user":
      body = <><circle cx="12" cy="8" r="3" /><path d="M5 20c.8-3.2 3.1-5 7-5s6.2 1.8 7 5" /></>;
      break;
    case "appearance":
      body = <><circle cx="12" cy="12" r="4" /><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" /></>;
      break;
    case "bell":
      body = <><path d="M6 17h12l-1.2-2V10a4.8 4.8 0 0 0-9.6 0v5Z" /><path d="M10 20h4" /></>;
      break;
    case "network":
      body = <><circle cx="12" cy="5" r="2" /><circle cx="5" cy="18" r="2" /><circle cx="19" cy="18" r="2" /><path d="m10.8 6.8-4.6 9.4M13.2 6.8l4.6 9.4M7 18h10" /></>;
      break;
    case "keyboard":
      body = <><rect x="3" y="6" width="18" height="12" rx="2" /><path d="M6 10h.01M9 10h.01M12 10h.01M15 10h.01M18 10h.01M7 14h10" /></>;
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
          <circle cx="7" cy="7" r="3" />
          <circle cx="7" cy="17" r="3" />
          <path d="m9.5 8.5 10.5 3.5-10.5 3.5M7 10v4" />
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
    case "copy":
      body = <><rect x="8" y="8" width="11" height="11" rx="2" /><path d="M16 8V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h3" /></>;
      break;
    case "forward":
      body = <><path d="m14 6 6 6-6 6" /><path d="M20 12H9a5 5 0 0 0-5 5v1" /></>;
      break;
    case "quote":
      body = <><path d="M4 5h16v12H8l-4 3Z" /><path d="M8 9h8M8 13h5" /></>;
      break;
    case "recall":
      body = <><path d="m9 7-5 5 5 5" /><path d="M5 12h8a6 6 0 0 1 6 6" /></>;
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
    case "edit":
      body = (
        <>
          <path d="M4 20h4L19 9l-4-4L4 16Z" />
          <path d="m13 7 4 4M4 20h16" />
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
    case "chevron-down":
      body = <path d="m6 9 6 6 6-6" />;
      break;
    default:
      body = <circle cx="12" cy="12" r="8" />;
  }
  return (
    <svg
      aria-hidden="true"
      className={`icon ${spin ? "icon-spin" : ""}`}
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

function groupPermissions(conversation, selfId) {
  const selfMember = (conversation?.members || []).find(
    (member) => member.peer_id === selfId,
  );
  const owner = selfMember?.role === "owner" || conversation?.created_by === selfId;
  return {
    selfMember,
    owner,
    manager: owner || selfMember?.role === "admin",
  };
}

function quoteReferenceText(source = {}, labels = copy["zh-CN"]) {
  const author = String(source.sender_name || source.sender_id || labels.unnamedDevice).trim();
  const fallback = isImageFile(source)
    ? labels.locale === "en" ? "[Image]" : "[图片]"
    : labels.attachment;
  const content = String(source.content || source.file_name || fallback)
    .replace(/\s+/g, " ")
    .trim();
  return `${author}: ${content || fallback}`;
}

function groupedReactions(reactions = []) {
  const groups = new Map();
  for (const reaction of reactions) {
    const group = groups.get(reaction.emoji) || { emoji: reaction.emoji, count: 0 };
    group.count += 1;
    groups.set(reaction.emoji, group);
  }
  return [...groups.values()];
}

function StrongReminderCard({ payload, embedded = false, onOpen, onDismiss }) {
  const name = payload.from_name || payload.from_id || "Xchat";
  return (
    <article className={`strong-reminder-card ${embedded ? "embedded" : ""}`} onClick={onOpen}>
      <span className="strong-reminder-avatar">{name.trim().slice(0, 1) || "我"}</span>
      <span className="strong-reminder-copy">
        <b>{name} 提醒你查看消息</b>
        <span>{payload.summary || "点击查看这条消息"}</span>
      </span>
      <footer>
        <span>点击查看消息</span>
        <button type="button" onClick={(event) => { event.stopPropagation(); onDismiss(); }}>忽略</button>
      </footer>
    </article>
  );
}

function StrongReminderWindow() {
  const params = new URLSearchParams(globalThis.location?.search || "");
  const payload = {
    conversation_id: params.get("conversation_id") || "",
    client_message_id: params.get("client_message_id") || "",
    from_name: params.get("from_name") || "Xchat",
    summary: params.get("summary") || "",
  };
  const invoke = globalThis.window?.__TAURI__?.core?.invoke;
  return (
    <main className="strong-reminder-window">
      <StrongReminderCard
        payload={payload}
        onOpen={() => invoke?.("open_strong_reminder", {
          conversationId: payload.conversation_id,
          clientMessageId: payload.client_message_id,
        })}
        onDismiss={() => invoke?.("dismiss_strong_reminder")}
      />
    </main>
  );
}

function Avatar({ entity = {}, labels, large = false, self = false }) {
  const groupRows = entity.kind === "group" ? groupAvatarRows(entity.members) : [];
  if (groupRows.length) {
    return (
      <span
        aria-hidden="true"
        className={`avatar group-avatar group-avatar-${Math.max(...groupRows.map((row) => row.length))}${large ? " avatar-large" : ""}`}
      >
        {groupRows.map((row, rowIndex) => (
          <span className="group-avatar-row" key={rowIndex}>
            {row.map((label, cellIndex) => (
              <i
                className="group-avatar-cell"
                key={`${rowIndex}:${cellIndex}`}
                style={{ "--avatar": hashColor(`${entity.id}:${rowIndex}:${cellIndex}`) }}
              >
                {label}
              </i>
            ))}
          </span>
        ))}
      </span>
    );
  }
  const label =
    entity.avatar ||
    (entity.kind === "group"
      ? labels.groupAvatar
      : avatarText(displayName(entity, labels)));
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

function appVersion() {
  return typeof globalThis.__XCHAT_VERSION__ === "string" && globalThis.__XCHAT_VERSION__
    ? globalThis.__XCHAT_VERSION__
    : "0.1.6";
}

function formatSize(bytes) {
  const value = Number(bytes || 0);
  if (!value) return "—";
  if (value < 1024) return `${value} B`;
  if (value < 1024 ** 2) return `${(value / 1024).toFixed(1)} KB`;
  if (value < 1024 ** 3) return `${(value / 1024 ** 2).toFixed(1)} MB`;
  return `${(value / 1024 ** 3).toFixed(1)} GB`;
}

function formatRate(bytesPerSecond) {
  return `${Number(bytesPerSecond) > 0 ? formatSize(bytesPerSecond) : "0 B"}/s`;
}

function formatProgressSize(bytes) {
  return Number(bytes) > 0 ? formatSize(bytes) : "0 B";
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
  const available = localFileAvailable(message);
  const status = fileStatus(message);
  useEffect(() => {
    if (!enabled || messageId == null || !available) {
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
  }, [available, enabled, message.file_path, messageId, status, workspace]);
  return media;
}

function statusText(status, labels) {
  return labels.status[status] || status;
}

function sourceText(source, labels) {
  return labels.sources[source] || source || labels.unknown;
}

function statusLabel(message, group, labels, peerOffline = false) {
  if (!message.own) return "";
  if (message.status === "failed") return labels.sendFailed;
  const deliveryStatus = messageDeliveryStatus(message, !group && peerOffline);
  if (deliveryStatus === "waiting_peer") {
    return labels.status.waiting_peer;
  }
  if (group && message.recipient_count) {
    return `${labels.deliveredCount(
      message.delivered_count || 0,
      message.recipient_count,
    )} · ${labels.readCount(message.read_count || 0, message.recipient_count)}`;
  }
  return statusText(deliveryStatus, labels);
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
  const offline = Boolean(conversation.peer?.is_offline);
  return (
    <button
      className={`conversation-row ${selected ? "selected" : ""}`}
      onClick={onOpen}
      data-od-id={`conversation-${conversation.id}`}
    >
      <span className="conversation-avatar">
        <Avatar entity={conversation.peer || conversation} labels={labels} />
        {conversation.kind !== "group" && (
          <span
            className={`conversation-presence ${offline ? "offline" : "online"}`}
            role="img"
            aria-label={offline ? labels.offline : labels.online}
            title={offline ? labels.offline : labels.online}
          />
        )}
      </span>
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
          Object.entries(labels.settingsSections).map(([id, section]) => (
            <button
              className={`settings-nav-row ${
                settingsSection === id ? "selected" : ""
              }`}
              key={id}
              onClick={() => onSettingsSection(id)}
              aria-current={settingsSection === id ? "location" : undefined}
            >
              <Icon name={section.icon} size={17} />
              <span>{section.label}</span>
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

function FileOpenMenu({ file, workspace, labels, canReveal }) {
  const [open, setOpen] = useState(false);
  const root = useRef(null);

  useEffect(() => setOpen(false), [file.conversation_id]);

  useEffect(() => {
    if (!open) return;
    const close = (event) => {
      if (event.key === "Escape") setOpen(false);
      if (event.type === "pointerdown" && !root.current?.contains(event.target)) {
        setOpen(false);
      }
    };
    addEventListener("keydown", close);
    addEventListener("pointerdown", close);
    return () => {
      removeEventListener("keydown", close);
      removeEventListener("pointerdown", close);
    };
  }, [open]);

  const run = (type) => {
    setOpen(false);
    workspace.dispatch({ type, file });
  };

  return (
    <span className="file-open-menu" ref={root}>
      <button
        type="button"
        className="file-open-trigger"
        onClick={() => setOpen((value) => !value)}
        aria-label={labels.openMenu}
        aria-haspopup="menu"
        aria-expanded={open}
      >
        {labels.open} <span aria-hidden="true">⌄</span>
      </button>
      {open && (
        <span className="file-open-popover" role="menu">
          <button type="button" role="menuitem" onClick={() => run("file.open")}>
            <Icon name="file" size={17} />
            {labels.openFile}
          </button>
          {canReveal && (
            <button type="button" role="menuitem" onClick={() => run("file.reveal")}>
              <Icon name="folder" size={17} />
              {labels.revealFile}
            </button>
          )}
        </span>
      )}
    </span>
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
  const bytesTotal = Number(activeTransfer?.bytes_total || message.file_size || 0);
  const bytesTransferred = Number(activeTransfer?.bytes_transferred || 0);
  const percent = activeTransfer?.progress_percent || 0;
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
        {activeTransfer && (
          <>
            <span className="message-transfer-meta">
              {percent}% · {formatProgressSize(bytesTransferred)} /{" "}
              {formatProgressSize(bytesTotal)}
              {" · "}
              {formatRate(activeTransfer.speed_bps)}
            </span>
            <span className="progress-track" aria-label={labels.progress(percent)}>
              <i style={{ width: `${percent}%` }} />
            </span>
          </>
        )}
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
        <FileOpenMenu
          file={message}
          workspace={workspace}
          labels={labels}
          canReveal={state.capabilities.revealFile}
        />
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

function Composer({ state, conversation, workspace, labels, quote, onClearQuote }) {
  const [text, setText] = useState(conversation?.draft || "");
  const [emojiOpen, setEmojiOpen] = useState(false);
  const [recentEmoji, setRecentEmoji] = useState(readRecentEmoji);
  const [mention, setMention] = useState(null);
  const [mentionIndex, setMentionIndex] = useState(0);
  const [mentionTargets, setMentionTargets] = useState([]);
  const [dragActive, setDragActive] = useState(false);
  const [sending, setSending] = useState(false);
  const composer = useRef(null);
  const textarea = useRef(null);
  const input = useRef(null);
  const emojiPanel = useRef(null);
  const nativeDragInside = useRef(false);
  const attachments = state.draftAttachments?.[conversation.id] || [];
  const mentionCandidates = useMemo(
    () =>
      groupMentionCandidates(
        conversation,
        state.devices,
        state.self.id,
      ),
    [conversation, state.devices, state.self.id],
  );
  const mentionOptions = useMemo(
    () =>
      mention
        ? groupMentionCandidates(
            conversation,
            state.devices,
            state.self.id,
            mention.query,
          )
        : [],
    [conversation, mention, state.devices, state.self.id],
  );

  useEffect(() => {
    setText(conversation?.draft || "");
    setEmojiOpen(false);
    setMention(null);
    setMentionIndex(0);
  }, [conversation?.id]);

  useEffect(() => {
    if (quote) requestAnimationFrame(() => textarea.current?.focus());
  }, [quote?.client_message_id, quote?.id]);

  useEffect(() => {
    const element = textarea.current;
    if (!element) return;
    element.style.height = "auto";
    element.style.height = `${Math.min(element.scrollHeight, 200)}px`;
  }, [text]);

  useEffect(() => {
    if (mentionOptions.length) {
      document
        .getElementById(`mention-option-${mentionIndex}`)
        ?.scrollIntoView({ block: "nearest" });
    }
  }, [mentionIndex, mentionOptions.length]);

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

  useEffect(() => {
    const currentWebview = nativeDragDropTarget();
    if (!currentWebview) return;
    let disposed = false;
    let unlisten = () => {};
    currentWebview
      .onDragDropEvent((event) => {
        const payload = event.payload ?? event;
        if (payload.type === "leave") {
          nativeDragInside.current = false;
          setDragActive(false);
          return;
        }
        const inside = isPhysicalPointInsideRect(
          payload.position,
          composer.current?.getBoundingClientRect(),
          globalThis.devicePixelRatio || 1,
        );
        if (payload.type === "over") {
          nativeDragInside.current = inside;
          setDragActive(inside);
          return;
        }
        if (payload.type === "drop") {
          const shouldAttach = inside || nativeDragInside.current;
          nativeDragInside.current = false;
          setDragActive(false);
          if (shouldAttach && payload.paths?.length) {
            workspace.dispatch({
              type: "draft.addPaths",
              conversationId: conversation.id,
              paths: payload.paths,
            });
          }
        }
      })
      .then((stop) => {
        if (typeof stop !== "function") return;
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten();
    };
  }, [conversation.id, workspace]);

  const send = async () => {
    const conversationId = conversation.id;
    const content = text.trim();
    if ((!content && !attachments.length) || sending) return;
    setSending(true);
    try {
      if (content) {
        const result = await workspace.dispatch({
          type: "message.sendText",
          conversationId,
          content,
          msgType: quote ? "quote" : "text",
          quote,
          mentionIds: retainedMentionIds(
            content,
            mentionTargets,
            conversationId,
          ),
        });
        if (!result.ok) return;
        if (quote) onClearQuote();

        const latest = workspace.getSnapshot();
        const currentDraft = latest.conversations.find(
          (item) => item.id === conversationId,
        )?.draft;
        const activeSame = latest.activeConversationId === conversationId;
        const unchanged = activeSame
          ? textarea.current?.value.trim() === content
          : !currentDraft || currentDraft.trim() === content;
        if (unchanged) {
          if (activeSame) {
            setText("");
            setMention(null);
          }
          setMentionTargets((current) =>
            current.filter((target) => target.conversation_id !== conversationId),
          );
          if (state.capabilities.conversationState) {
            workspace.dispatch({
              type: "conversation.saveDraft",
              id: conversationId,
              draft: "",
            });
          }
        }
      }
      for (const attachment of attachments) {
        const result = await workspace.dispatch({
          type: "message.sendFiles",
          conversationId,
          files: [attachment],
        });
        if (result.ok) {
          await workspace.dispatch({
            type: "draft.sent",
            conversationId,
            id: attachment.id,
          });
        }
      }
    } finally {
      setSending(false);
    }
  };

  const selectMention = (candidate) => {
    if (!mention) return;
    const element = textarea.current;
    const end = element?.selectionStart ?? text.length;
    const token = mentionToken(
      candidate,
      mentionCandidates,
      labels.unnamedDevice,
    );
    const next = `${text.slice(0, mention.start)}${token} ${text.slice(end)}`;
    const caret = mention.start + token.length + 1;
    setText(next);
    setMention(null);
    setMentionTargets((current) => [
      ...current.filter(
        (target) =>
          target.conversation_id !== conversation.id ||
          target.peer_id !== candidate.peer_id,
      ),
      {
        conversation_id: conversation.id,
        peer_id: candidate.peer_id,
        token,
      },
    ]);
    requestAnimationFrame(() => {
      element?.focus();
      element?.setSelectionRange(caret, caret);
    });
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
    const next = insertTextAtSelection(text, emoji, start, end);
    setText(next.value);
    setRecentEmoji((current) => rememberEmoji(emoji, current));
    setEmojiOpen(false);
    requestAnimationFrame(() => {
      element?.focus();
      element?.setSelectionRange(next.caret, next.caret);
    });
  };

  return (
    <footer
      className={`composer ${dragActive ? "drag-active" : ""}`}
      ref={composer}
      data-od-id="message-composer"
      onDragEnter={(event) => {
        if ([...event.dataTransfer.items].some((item) => item.kind === "file")) {
          event.preventDefault();
          setDragActive(true);
        }
      }}
      onDragOver={(event) => {
        if ([...event.dataTransfer.types].includes("Files")) {
          event.preventDefault();
          setDragActive(true);
        }
      }}
      onDragLeave={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget)) setDragActive(false);
      }}
      onDrop={(event) => {
        event.preventDefault();
        event.stopPropagation();
        setDragActive(false);
        const files = [];
        const rejectedNames = [];
        for (const item of event.dataTransfer.items) {
          if (item.kind !== "file") continue;
          const entry = item.webkitGetAsEntry?.();
          if (entry?.isDirectory) rejectedNames.push(entry.name);
          else {
            const file = item.getAsFile();
            if (file) files.push(file);
          }
        }
        if (!event.dataTransfer.items.length) {
          files.push(...event.dataTransfer.files);
        }
        if (files.length || rejectedNames.length) {
          workspace.dispatch({
            type: "draft.addFiles",
            conversationId: conversation.id,
            files,
            rejectedNames,
          });
        }
      }}
    >
      <div className="compose-box" data-drop-label={labels.dropFilesHere}>
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
          onChange={(event) => {
            const next = event.target.value;
            setText(next);
            setMentionTargets((current) =>
              current.filter(
                (target) =>
                  target.conversation_id !== conversation.id ||
                  next.includes(target.token),
              ),
            );
            setMention(
              conversation.kind === "group"
                ? mentionQueryAtCaret(next, event.target.selectionStart)
                : null,
            );
            setMentionIndex(0);
            setEmojiOpen(false);
          }}
          onBlur={() => {
            setMention(null);
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
            if (event.nativeEvent.isComposing) return;
            if (mention && event.key === "Escape") {
              event.preventDefault();
              setMention(null);
              return;
            }
            if (mentionOptions.length && event.key === "ArrowDown") {
              event.preventDefault();
              setMentionIndex((index) => (index + 1) % mentionOptions.length);
              return;
            }
            if (mentionOptions.length && event.key === "ArrowUp") {
              event.preventDefault();
              setMentionIndex(
                (index) => (index - 1 + mentionOptions.length) % mentionOptions.length,
              );
              return;
            }
            if (mentionOptions.length && event.key === "Enter") {
              event.preventDefault();
              selectMention(mentionOptions[mentionIndex]);
              return;
            }
            if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
              event.preventDefault();
              send();
            }
          }}
          onPaste={(event) => {
            const files = [...event.clipboardData.files];
            const paths = state.capabilities.nativeFilePicker
              ? nativeClipboardPaths(
                  event.clipboardData.getData("text/uri-list") ||
                    event.clipboardData.getData("text/plain"),
                )
              : [];
            if (files.length || paths.length) {
              event.preventDefault();
              if (
                files.length &&
                (!state.capabilities.nativeFilePicker || !paths.length)
              ) {
                workspace.dispatch({
                  type: "draft.addFiles",
                  conversationId: conversation.id,
                  files,
                  fromClipboard: state.capabilities.nativeFilePicker,
                });
              }
              if (
                paths.length &&
                (state.capabilities.nativeFilePicker || !files.length)
              ) {
                workspace.dispatch({
                  type: "draft.addPaths",
                  conversationId: conversation.id,
                  paths,
                });
              }
            }
          }}
          rows="2"
          placeholder={labels.messagePlaceholder}
          aria-label={labels.message}
          aria-expanded={Boolean(mention && mentionOptions.length)}
          aria-controls={mentionOptions.length ? "mention-options" : undefined}
          aria-activedescendant={
            mentionOptions.length ? `mention-option-${mentionIndex}` : undefined
          }
        />
        {mention && mentionOptions.length > 0 && (
          <div
            className="mention-panel"
            id="mention-options"
            role="listbox"
            aria-label={labels.groupMembers(mentionOptions.length)}
          >
            {mentionOptions.map((candidate, index) => (
              <button
                type="button"
                id={`mention-option-${index}`}
                className={index === mentionIndex ? "active" : ""}
                role="option"
                aria-selected={index === mentionIndex}
                key={candidate.peer_id}
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => selectMention(candidate)}
              >
                <b>{candidate.display_name || labels.unnamedDevice}</b>
                <small>{candidate.addr || labels.unknownAddress}</small>
              </button>
            ))}
          </div>
        )}
        {quote && (
          <div className="quote-preview" role="note">
            <span className="quote-preview-text" title={quoteReferenceText(quote, labels)}>
              {quoteReferenceText(quote, labels)}
            </span>
            <button
              className="quote-preview-close"
              type="button"
              onClick={onClearQuote}
              aria-label={labels.close}
              title={labels.close}
            >
              <Icon name="close" size={12} />
            </button>
          </div>
        )}
        <div className="compose-toolbar">
          <div className="compose-tools">
            <button
              className={`icon-button composer-tool ${emojiOpen ? "active" : ""}`}
              type="button"
              data-emoji-toggle
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => {
                setMention(null);
                setEmojiOpen((value) => !value);
              }}
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
            <div className="emoji-recent" aria-label={labels.locale === "en" ? "Recently used" : "最近使用"}>
              <small>{labels.locale === "en" ? "Recent" : "最近使用"}</small>
              <span>
                {(recentEmoji.length ? recentEmoji : EMOJI_SET.slice(0, 6)).map((emoji) => (
                  <button type="button" key={emoji} onMouseDown={(event) => event.preventDefault()} onClick={() => insertEmoji(emoji)}>{emoji}</button>
                ))}
              </span>
            </div>
          </div>
        )}
      </div>
    </footer>
  );
}

function messageSummary(message, labels) {
  if (message.msg_type === "file") return message.file_name || message.content || labels.attachment;
  if (message.msg_type === "announcement") return message.content;
  return message.content || labels.message;
}

function ForwardModal({ message, state, workspace, labels, onClose }) {
  const [query, setQuery] = useState("");
  const [kind, setKind] = useState("all");
  const [selected, setSelected] = useState([]);
  const [note, setNote] = useState("");
  const [sending, setSending] = useState(false);
  const targets = state.conversations.filter((conversation) => {
    if (kind !== "all" && (kind === "group") !== (conversation.kind === "group")) return false;
    return displayName(conversation, labels).toLocaleLowerCase().includes(query.trim().toLocaleLowerCase());
  });
  const chosen = selected
    .map((id) => state.conversations.find((conversation) => conversation.id === id))
    .filter(Boolean);
  const toggle = (id) => setSelected((current) => current.includes(id)
    ? current.filter((item) => item !== id)
    : [...current, id]);
  const submit = async () => {
    if (!selected.length || sending) return;
    setSending(true);
    try {
      const result = await workspace.dispatch({
        type: "message.forward",
        messageId: message.message_id ?? message.id,
        conversationIds: selected,
        note,
      });
      if (result.ok) onClose();
    } finally {
      setSending(false);
    }
  };
  return (
    <Modal title={labels.locale === "en" ? "Forward message" : "转发消息"} closeLabel={labels.close} onClose={onClose} wide>
      <div className="forward-shell">
        <section className="forward-main">
          <div className="forward-search-head">
            <label className="modal-search">
              <Icon name="search" size={16} />
              <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder={labels.locale === "en" ? "Search contacts and groups" : "搜索联系人和群聊"} autoFocus />
            </label>
            <div className="forward-tabs">
              {[["all", "全部"], ["direct", "联系人"], ["group", "群聊"]].map(([value, text]) => (
                <button key={value} className={kind === value ? "active" : ""} onClick={() => setKind(value)}>{labels.locale === "en" ? ({ all: "All", direct: "Contacts", group: "Groups" })[value] : text}</button>
              ))}
            </div>
          </div>
          <div className="forward-list">
            {targets.map((target) => (
              <button key={target.id} className={`forward-row ${selected.includes(target.id) ? "selected" : ""}`} onClick={() => toggle(target.id)}>
                <span className="forward-check">✓</span>
                <Avatar entity={target.peer || target} labels={labels} />
                <span><b>{displayName(target, labels)}</b><small>{target.kind === "group" ? labels.memberCount(target.members?.length || 0) : target.peer?.hostname || target.peer?.addr}</small></span>
              </button>
            ))}
          </div>
        </section>
        <aside className="forward-side">
          <h3>{labels.locale === "en" ? "Send to" : "发送给"}</h3>
          <div className="selected-chips">
            {chosen.length ? chosen.map((target) => (
              <span className="selected-chip" key={target.id}>
                <Avatar entity={target.peer || target} labels={labels} />
                <span>{displayName(target, labels)}</span>
                <button onClick={() => toggle(target.id)}>×</button>
              </span>
            )) : <span className="helper">{labels.locale === "en" ? "Select contacts or groups on the left" : "从左侧选择联系人或群聊"}</span>}
          </div>
          <div className="forward-compose">
            <div className="forward-preview-card">
              <span className="forward-preview-icon"><Icon name={message.msg_type === "file" ? "file" : "chat"} /></span>
              <span className="forward-preview-copy"><b>{message.msg_type === "file" ? (labels.locale === "en" ? "File" : "文件") : (labels.locale === "en" ? "Message" : "消息内容")}</b><span>{messageSummary(message, labels)}</span></span>
            </div>
            <textarea className="forward-note" value={note} maxLength={1000} onChange={(event) => setNote(event.target.value)} placeholder={labels.locale === "en" ? "Add a message (optional)" : "给朋友留言（可选）"} />
            <div className="forward-foot">
              <button onClick={onClose}>{labels.cancel}</button>
              <button className="primary" disabled={!selected.length || sending} onClick={submit}>{sending ? (labels.locale === "en" ? "Sending…" : "发送中…") : labels.send}</button>
            </div>
          </div>
        </aside>
      </div>
    </Modal>
  );
}

function HistoryModal({ conversation, messages, state, workspace, labels, onJump, onClose }) {
  const [query, setQuery] = useState("");
  const [kind, setKind] = useState("all");
  useEffect(() => {
    const timer = setTimeout(() => workspace.dispatch({ type: "message.search", query }), 180);
    return () => clearTimeout(timer);
  }, [query, workspace]);
  const source = query.trim()
    ? state.searchResults.filter((message) => message.conversation_id === conversation.id)
    : messages;
  const results = source.filter((message) => {
    const type = message.msg_type === "file" ? "file" : "text";
    return (kind === "all" || kind === type) && messageSummary(message, labels).toLocaleLowerCase().includes(query.trim().toLocaleLowerCase());
  });
  return (
    <Modal title={labels.locale === "en" ? "Search chat history" : "查找聊天记录"} closeLabel={labels.close} onClose={onClose} wide>
      <div className="history-shell">
        <section className="history-side">
          <div className="history-search-head">
            <label className="modal-search"><Icon name="search" size={16} /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder={labels.locale === "en" ? "Search this conversation" : `在“${displayName(conversation, labels)}”中查找`} autoFocus /></label>
            <div className="history-filters">
              {[["all", "全部"], ["text", "文本"], ["file", "文件"]].map(([value, text]) => <button key={value} className={kind === value ? "active" : ""} onClick={() => setKind(value)}>{labels.locale === "en" ? ({ all: "All", text: "Text", file: "Files" })[value] : text}</button>)}
            </div>
          </div>
          <div className="history-results">
            {results.map((message) => <button className="history-result" key={message.client_message_id || message.id} onClick={() => { onClose(); onJump(message.client_message_id || message.message_id || message.id); }}><span className="history-result-top"><span>{message.sender_name || message.sender_id}</span><span>{formatTime(message.timestamp, labels.locale)}</span></span><p>{messageSummary(message, labels)}</p></button>)}
            {!results.length && <div className="history-empty">{labels.locale === "en" ? "No matching messages" : "没有找到相关记录"}</div>}
          </div>
        </section>
        <div className="history-preview"><Icon name="search" size={52} /><b>{labels.locale === "en" ? "Select a result to jump to it" : "点击搜索结果即可定位到原消息"}</b></div>
      </div>
    </Modal>
  );
}

function ChatWorkspace({ state, workspace, labels, onBack, onToggleInfo, infoOpen, onConfirm }) {
  const conversation = state.conversations.find(
    (item) => item.id === state.activeConversationId,
  );
  const messages = state.messagesByConversation[state.activeConversationId] || [];
  const scroll = useRef(null);
  const [menu, setMenu] = useState(null);
  const [reactionPicker, setReactionPicker] = useState(null);
  const [recentEmoji, setRecentEmoji] = useState(readRecentEmoji);
  const [quote, setQuote] = useState(null);
  const [forward, setForward] = useState(null);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [announcementOpen, setAnnouncementOpen] = useState(false);
  const [dismissedAnnouncement, setDismissedAnnouncement] = useState("");

  useEffect(() => {
    setQuote(null);
    setMenu(null);
    setReactionPicker(null);
  }, [conversation?.id]);

  useEffect(() => {
    const close = () => {
      setMenu(null);
      setReactionPicker(null);
    };
    addEventListener("pointerdown", close);
    addEventListener("resize", close);
    return () => {
      removeEventListener("pointerdown", close);
      removeEventListener("resize", close);
    };
  }, []);

  useEffect(() => {
    const openHistory = () => setHistoryOpen(true);
    addEventListener("xchat:open-history", openHistory);
    return () => removeEventListener("xchat:open-history", openHistory);
  }, []);

  useEffect(() => {
    const viewport = scroll.current;
    if (!viewport || state.focusedMessageId != null) return undefined;
    let animationFrame = 0;
    const scheduleBottomScroll = () => {
      cancelAnimationFrame(animationFrame);
      animationFrame = requestAnimationFrame(() => {
        viewport.scrollTop = viewport.scrollHeight;
      });
    };
    const handleResourceLoad = (event) => {
      if (event.target instanceof HTMLImageElement) scheduleBottomScroll();
    };

    scheduleBottomScroll();
    viewport.addEventListener("load", handleResourceLoad, true);
    return () => {
      cancelAnimationFrame(animationFrame);
      viewport.removeEventListener("load", handleResourceLoad, true);
    };
  }, [conversation?.id, messages.length, state.focusedMessageId]);

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
    if (
      conversation &&
      isAppActive(document.visibilityState, document.hasFocus())
    ) {
      workspace.dispatch({ type: "message.markRead", conversationId: conversation.id });
    }
  }, [conversation?.id, messages.length, workspace]);

  useEffect(() => {
    if (!conversation) return;
    const markReadWhenVisible = () => {
      if (isAppActive(document.visibilityState, document.hasFocus())) {
        workspace.dispatch({
          type: "message.markRead",
          conversationId: conversation.id,
        });
      }
    };
    addEventListener("focus", markReadWhenVisible);
    document.addEventListener("visibilitychange", markReadWhenVisible);
    return () => {
      removeEventListener("focus", markReadWhenVisible);
      document.removeEventListener("visibilitychange", markReadWhenVisible);
    };
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
  const announcement = [...messages].reverse().find((message) => message.msg_type === "announcement");
  const announcementKey = announcement && `${conversation.id}:${announcement.client_message_id || announcement.id}`;
  const announcementHidden = !announcement || dismissedAnnouncement === announcementKey || globalThis.localStorage?.getItem(`xchat:announcement:${announcementKey}`) === "dismissed";
  const visibleMessages = messages.filter(
    (message) => message.msg_type !== "announcement" && message.status !== "recalled",
  );
  const messageDividerIndices = new Set(messageTimeDividerIndices(visibleMessages));
  const jumpToMessage = (messageId) => workspace.dispatch({
    type: "conversation.open",
    id: conversation.id,
    targetClientMessageId: messageId,
  });
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
      onDrop={(event) => event.preventDefault()}
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
            <Icon name="more" />
          </button>
        </div>
      </header>
      {!announcementHidden && (
        <button className="announcement-banner" onClick={() => setAnnouncementOpen(true)}>
          <span className="announcement-mark">📢</span>
          <span className="announcement-copy"><b>{labels.locale === "en" ? "Group announcement" : "群公告"}</b><small>{announcement.content}</small></span>
          <span className="announcement-arrow">›</span>
        </button>
      )}
      {conversation.kind !== "group" && peer?.is_offline && (
        <div className="peer-offline-safety-banner" role="status" aria-live="polite">
          <span className="peer-offline-safety-mark" aria-hidden="true">!</span>
          <span>
            <b>{labels.offlineSafetyTitle}</b>
            <small>{labels.offlineSafetyText}</small>
          </span>
        </div>
      )}
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
        {visibleMessages.map((message, index) => (
          <Fragment key={message.client_message_id || message.id}>
            {messageDividerIndices.has(index) && (
              <div className="message-time-divider">
                {formatMessageTime(message.timestamp, labels.locale)}
              </div>
            )}
            <article
            className={`message ${message.own ? "sent" : "received"}`}
            data-od-id={`message-${message.client_message_id || message.id}`}
            data-message-key={
              message.client_message_id || message.message_id || message.id
            }
            data-message-id={message.message_id ?? message.id}
            data-client-message-id={message.client_message_id || undefined}
            onContextMenu={(event) => {
              event.preventDefault();
              event.stopPropagation();
              setMenu({
                x: Math.min(event.clientX, globalThis.innerWidth - 202),
                y: Math.min(event.clientY, globalThis.innerHeight - 250),
                message,
              });
            }}
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
              <div className="message-body-line">
                <div className="message-body-content">
                  {message.msg_type === "text" || message.msg_type === "quote" ? (
                    <>
                      <div className="bubble">
                        <span>{message.content}</span>
                      </div>
                      {message.quote && (
                        <button
                          type="button"
                          className="quoted-block"
                          onClick={() => jumpToMessage(message.quote.client_message_id || message.quote.message_id)}
                          title={labels.locale === "en" ? "Jump to original message" : "点击定位到原消息"}
                          aria-label={labels.locale === "en" ? "Jump to original message" : "点击定位到原消息"}
                        >
                          {quoteReferenceText(message.quote, labels)}
                        </button>
                      )}
                    </>
                  ) : (
                    <MessageFile
                      message={message}
                      state={state}
                      workspace={workspace}
                      labels={labels}
                    />
                  )}
                </div>
                <div className="message-quick-actions" aria-label={labels.locale === "en" ? "Message actions" : "消息操作"}>
                  <button
                    type="button"
                    aria-label={labels.emoji}
                    title={labels.emoji}
                    disabled={!message.client_message_id}
                    onClick={(event) => {
                      event.stopPropagation();
                      const rect = event.currentTarget.getBoundingClientRect();
                      setMenu(null);
                      setReactionPicker({
                        message,
                        x: Math.max(10, Math.min(rect.left - 130, globalThis.innerWidth - 326)),
                        y: Math.max(10, Math.min(rect.bottom + 7, globalThis.innerHeight - 356)),
                      });
                    }}
                  ><Icon name="emoji" size={16} /></button>
                  <button
                    type="button"
                    aria-label={labels.locale === "en" ? "Quote" : "引用"}
                    title={labels.locale === "en" ? "Quote" : "引用"}
                    onClick={() => { setQuote(message); setMenu(null); setReactionPicker(null); }}
                  ><Icon name="quote" size={16} /></button>
                  <button
                    type="button"
                    aria-label={labels.locale === "en" ? "More" : "更多"}
                    title={labels.locale === "en" ? "More" : "更多"}
                    onClick={(event) => {
                      event.stopPropagation();
                      const rect = event.currentTarget.getBoundingClientRect();
                      setReactionPicker(null);
                      setMenu({
                        x: Math.max(10, Math.min(rect.right - 188, globalThis.innerWidth - 198)),
                        y: Math.max(10, Math.min(rect.bottom + 7, globalThis.innerHeight - 250)),
                        message,
                      });
                    }}
                  ><Icon name="more" size={16} /></button>
                </div>
              </div>
              {statusLabel(message, conversation.kind === "group", labels, peer?.is_offline) && (
                <span className={`message-meta ${message.status === "failed" ? "danger-text" : ""}`}>
                  <i>{statusLabel(message, conversation.kind === "group", labels, peer?.is_offline)}</i>
                </span>
              )}
            </div>
            </article>
            {groupedReactions(message.reactions).length > 0 && (
              <div className={`message-reactions ${message.own ? "sent" : "received"}`}>
                {groupedReactions(message.reactions).map((reaction) => (
                  <button
                    type="button"
                    key={reaction.emoji}
                    onClick={() => workspace.dispatch({
                      type: "message.react",
                      conversationId: conversation.id,
                      clientMessageId: message.client_message_id,
                      emoji: reaction.emoji,
                    })}
                  >{reaction.emoji}<span>{reaction.count}</span></button>
                ))}
              </div>
            )}
          </Fragment>
        ))}
      </div>
      <Composer
        state={state}
        conversation={conversation}
        workspace={workspace}
        labels={labels}
        quote={quote}
        onClearQuote={() => setQuote(null)}
      />
      {reactionPicker && (
        <div
          className="message-reaction-picker"
          style={{ left: reactionPicker.x, top: reactionPicker.y }}
          onPointerDown={(event) => event.stopPropagation()}
        >
          <div className="message-reaction-grid">
            {EMOJI_SET.map((emoji) => (
              <button
                type="button"
                key={emoji}
                onClick={() => {
                  workspace.dispatch({
                    type: "message.react",
                    conversationId: conversation.id,
                    clientMessageId: reactionPicker.message.client_message_id,
                    emoji,
                  });
                  setRecentEmoji((current) => rememberEmoji(emoji, current));
                  setReactionPicker(null);
                }}
              >{emoji}</button>
            ))}
          </div>
          <div className="message-reaction-recent">
            <small>{labels.locale === "en" ? "Recent" : "最近使用"}</small>
            <span>{(recentEmoji.length ? recentEmoji : EMOJI_SET.slice(0, 6)).map((emoji) => <button type="button" key={emoji} onClick={() => {
              workspace.dispatch({ type: "message.react", conversationId: conversation.id, clientMessageId: reactionPicker.message.client_message_id, emoji });
              setRecentEmoji((current) => rememberEmoji(emoji, current));
              setReactionPicker(null);
            }}>{emoji}</button>)}</span>
          </div>
        </div>
      )}
      {menu && (
        <div
          className="message-context-menu"
          style={{ left: menu.x, top: menu.y }}
          role="menu"
          onPointerDown={(event) => event.stopPropagation()}
        >
          {menu.message.own && menu.message.client_message_id && (
            <button onClick={() => {
              workspace.dispatch({
                type: "message.strongReminder",
                conversationId: conversation.id,
                clientMessageId: menu.message.client_message_id,
              });
              setMenu(null);
            }}>
              <Icon name="bell" size={16} />{labels.locale === "en" ? "Remind" : "提醒"}
            </button>
          )}
          {isCopyableMessage(menu.message) && (
            <button onClick={async () => {
              if (!["text", "quote"].includes(menu.message.msg_type)) {
                await workspace.dispatch({ type: "message.copyFile", file: menu.message });
              } else {
                await navigator.clipboard?.writeText(messageSummary(menu.message, labels));
              }
              setMenu(null);
            }}>
              <Icon name="copy" size={16} />{labels.locale === "en" ? "Copy" : "复制"}
            </button>
          )}
          <button onClick={() => { setForward(menu.message); setMenu(null); }}>
            <Icon name="forward" size={16} />{labels.locale === "en" ? "Forward" : "转发"}
          </button>
          <button onClick={() => { setQuote(menu.message); setMenu(null); }}>
            <Icon name="quote" size={16} />{labels.locale === "en" ? "Quote" : "引用"}
          </button>
          {menu.message.msg_type === "file" && localFileAvailable(menu.message) && (
            <button onClick={() => { workspace.dispatch({ type: "file.saveAs", file: menu.message }); setMenu(null); }}>
              <Icon name="download" size={16} />{labels.locale === "en" ? "Save As…" : "另存为"}
            </button>
          )}
          <i className="context-separator" />
          {menu.message.own && menu.message.client_message_id && (
            <button onClick={() => { workspace.dispatch({ type: "message.recall", clientMessageId: menu.message.client_message_id }); setMenu(null); }}>
              <Icon name="recall" size={16} />{labels.locale === "en" ? "Recall" : "撤回"}
            </button>
          )}
          {menu.message.id !== undefined && (
            <button className="danger-text" onClick={() => { const messageId = menu.message.id; setMenu(null); onConfirm({ title: labels.deleteMessageTitle, detail: labels.deleteMessageDetail, action: labels.deleteMessageAction, run: () => workspace.dispatch({ type: "message.deleteLocal", ids: [messageId] }) }); }}>
              <Icon name="trash" size={16} />{labels.locale === "en" ? "Delete locally" : "删除"}
            </button>
          )}
        </div>
      )}
      {forward && (
        <ForwardModal message={forward} state={state} workspace={workspace} labels={labels} onClose={() => setForward(null)} />
      )}
      {historyOpen && <HistoryModal conversation={conversation} messages={visibleMessages} state={state} workspace={workspace} labels={labels} onJump={jumpToMessage} onClose={() => setHistoryOpen(false)} />}
      {announcementOpen && announcement && (
        <Modal title={labels.locale === "en" ? "Group announcement" : "群公告"} closeLabel={labels.close} onClose={() => setAnnouncementOpen(false)} actions={<button className="primary" onClick={() => { globalThis.localStorage?.setItem(`xchat:announcement:${announcementKey}`, "dismissed"); setDismissedAnnouncement(announcementKey); setAnnouncementOpen(false); }}>{labels.locale === "en" ? "Got it" : "我知道了"}</button>}>
          <div className="announcement-detail"><div className="announcement-detail-head"><span className="announcement-detail-icon">📢</span><span><b>{labels.locale === "en" ? "Group announcement" : "群公告"}</b><small>{announcement.sender_name || announcement.sender_id} · {formatTime(announcement.timestamp, labels.locale)}</small></span></div><div className="announcement-content">{announcement.content}</div></div>
        </Modal>
      )}
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
              <dt>{labels.deviceId}</dt>
              <dd className="numeric">{device.id}</dd>
            </div>
            <div>
              <dt>{labels.currentAddress}</dt>
              <dd className="numeric">{device.addr || labels.notProvided}</dd>
            </div>
            <div>
              <dt>{labels.macAddress}</dt>
              <dd className="numeric">{device.mac_address || labels.notProvided}</dd>
            </div>
            <div>
              <dt>{labels.identityVerification}</dt>
              <dd>
                {device.is_offline
                  ? labels.identityOfflineStopped
                  : labels.identityVerifiedCurrentAddress}
              </dd>
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
              <dt>{labels.version}</dt>
              <dd>{device.app_version || labels.notProvided}</dd>
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
  onManageFixedPeers,
}) {
  const [form, setForm] = useState(state.settings);
  const [dirty, setDirty] = useState(false);
  const [ipLoading, setIpLoading] = useState(false);
  const [availableIps, setAvailableIps] = useState([]);
  const [ipDropdownOpen, setIpDropdownOpen] = useState(false);
  const [adaptersExpanded, setAdaptersExpanded] = useState(false);
  const [networkRefreshing, setNetworkRefreshing] = useState(false);
  const [networkUpdated, setNetworkUpdated] = useState(false);
  const [pendingRiskInterfaceId, setPendingRiskInterfaceId] = useState(null);
  const scroll = useRef(null);
  // 进入设置页就读取一次网卡列表，多网卡时下拉框才会直接可见
  useEffect(() => {
    const invoke = globalThis.window?.__TAURI__?.core?.invoke;
    if (!invoke) return;
    let cancelled = false;
    Promise.resolve(invoke("get_all_local_ips"))
      .then((ips) => {
        if (!cancelled && Array.isArray(ips)) setAvailableIps(ips);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);
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
    setForm((current) => {
      const next = { ...current, [key]: value };
      setDirty(settingsFormDirty(next, state.settings));
      return next;
    });
  };
  const discoverySettings = {
    ...recommendedDiscoverySettings(),
    ...(form.discovery_settings || {}),
    interface_overrides: {
      ...(form.discovery_settings?.interface_overrides || {}),
    },
  };
  const networkInterfaces = state.settings.network_interfaces || [];
  const adapterSummary = discoverySummary(networkInterfaces, discoverySettings);
  const portValid = validServerPort(form.port);
  const applyInterfaceSelection = (interfaceId, enabled) => {
    const networkInterface = networkInterfaces.find(({ id }) => id === interfaceId);
    if (!networkInterface) return;
    change(
      "discovery_settings",
      withDiscoveryInterfaceSelection(discoverySettings, networkInterface, enabled),
    );
    setPendingRiskInterfaceId(null);
  };
  const selectDiscoveryInterface = (networkInterface, enabled) => {
    if (enabled && networkInterface.category === "proxy_tun") {
      setPendingRiskInterfaceId(networkInterface.id);
      return;
    }
    applyInterfaceSelection(networkInterface.id, enabled);
  };
  const refreshNetworkInterfaces = async () => {
    setNetworkRefreshing(true);
    try {
      const result = await workspace.dispatch({ type: "refresh" });
      if (result.ok) setNetworkUpdated(true);
    } finally {
      setNetworkRefreshing(false);
    }
  };
  const choosePath = async (key) => {
    const result = await workspace.dispatch({
      type: "settings.pickPath",
      title: labels.chooseFolder,
    });
    if (result.ok && result.data) change(key, Array.isArray(result.data) ? result.data[0] : result.data);
  };
  const refreshIp = async () => {
    const tauri = globalThis.window?.__TAURI__;
    if (!tauri?.core?.invoke) return;
    setIpLoading(true);
    try {
      const ips = await tauri.core.invoke("refresh_local_ips");
      if (Array.isArray(ips)) setAvailableIps(ips);
      // 立刻重读快照，否则展示的 IP 要等下一轮轮询（最多 5 秒）才更新
      await workspace.dispatch({ type: "refresh" });
    } catch {
      // 探测失败保持原值，不打断设置页
    } finally {
      setIpLoading(false);
    }
  };
  const selectIp = async (ip) => {
    const tauri = globalThis.window?.__TAURI__;
    if (!tauri?.core?.invoke) return;
    setIpDropdownOpen(false);
    try {
      await tauri.core.invoke("set_local_ip", { ip });
      await workspace.dispatch({ type: "refresh" });
    } catch {
      // 目标地址已失效时保持原值
    }
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
            const patch = settingsPatch(form, state.settings);
            const result = await workspace.dispatch({ type: "settings.patch", patch });
            if (result.ok) setDirty(false);
          }}
          disabled={!dirty || !portValid}
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
          <SettingRow label={labels.localIp}>
            <div className="ip-selector">
              <output className="setting-readonly">{state.self.addr || labels.notProvided}</output>
              <button
                type="button"
                className="icon-button ip-refresh-button"
                onClick={refreshIp}
                disabled={ipLoading}
                aria-label={labels.refreshIp || "刷新IP"}
                title={labels.refreshIp || "刷新IP地址"}
              >
                <Icon name="refresh" size={15} spin={ipLoading} />
              </button>
              {availableIps.length > 1 && (
                <div className="ip-dropdown-container">
                  <button
                    type="button"
                    className="icon-button ip-dropdown-button"
                    onClick={() => setIpDropdownOpen(!ipDropdownOpen)}
                    aria-label={labels.selectIp || "选择IP"}
                    title={labels.selectIp || "选择IP地址"}
                  >
                    <Icon name="chevron-down" size={15} />
                  </button>
                  {ipDropdownOpen && (
                    <div className="ip-dropdown-menu">
                      {availableIps.map((ip) => (
                        <button
                          key={ip}
                          type="button"
                          className={`ip-option ${ip === state.self.addr ? "selected" : ""}`}
                          onClick={() => selectIp(ip)}
                        >
                          {ip}
                        </button>
                      ))}
                    </div>
                  )}
                </div>
              )}
            </div>
          </SettingRow>
          <SettingRow label={labels.macAddress}>
            <output className="setting-readonly">
              {state.self.mac_address || labels.notProvided}
            </output>
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
            <div className="path-picker-field">
              <input value={form.download_path} onChange={(event) => change("download_path", event.target.value)} />
              <button type="button" className="path-picker-button" onClick={() => choosePath("download_path")} aria-label={labels.chooseFolder} title={labels.chooseFolder}><Icon name="folder" size={17} /></button>
            </div>
          </SettingRow>
          <SettingRow
            label={labels.autoReceiveFiles}
            detail={labels.autoReceiveFilesHint}
          >
            <input type="checkbox" checked={form.auto_download} onChange={(event) => change("auto_download", event.target.checked)} />
          </SettingRow>
          <SettingRow
            label={labels.maxParallelChannels}
            detail={labels.maxParallelChannelsHint(form.max_parallel_channels)}
          >
            <select
              value={form.max_parallel_channels}
              aria-label={labels.maxParallelChannelsAria}
              onChange={(event) => change("max_parallel_channels", Number(event.target.value))}
            >
              <option value={4}>{labels.maxParallelChannelsDefault}</option>
              <option value={8}>8</option>
              <option value={16}>16</option>
            </select>
          </SettingRow>
        </section>
        <section className="settings-section" id="settings-network">
          <h2>{labels.network}</h2>
          <SettingRow label={labels.serverPort} detail={labels.restartRequired}>
            <span className="setting-field-stack">
              <input
                className="numeric"
                type="number"
                min="1"
                max="65535"
                step="1"
                inputMode="numeric"
                value={form.port}
                aria-invalid={!portValid}
                aria-describedby={!portValid ? "server-port-error" : undefined}
                onChange={(event) => change("port", event.target.value)}
              />
              {!portValid && (
                <small id="server-port-error" className="setting-field-error" role="alert">
                  {labels.invalidServerPort}
                </small>
              )}
            </span>
          </SettingRow>
          <SettingRow label={labels.databasePath} detail={labels.restartRequired}>
            <div className="path-picker-field">
              <input value={form.db_path} onChange={(event) => change("db_path", event.target.value)} />
              <button type="button" className="path-picker-button" onClick={() => choosePath("db_path")} aria-label={labels.chooseFolder} title={labels.chooseFolder}><Icon name="folder" size={17} /></button>
            </div>
          </SettingRow>
          <div className="settings-subtitle">
            <b>{labels.deviceDiscovery}</b>
            <span>{labels.deviceDiscoverySubtitle}</span>
          </div>
          <div className="settings-note">{labels.discoveryNote}</div>
          <SettingRow label={labels.localDiscovery} detail={labels.localDiscoveryHint}>
            <input
              type="checkbox"
              checked={discoverySettings.local_discovery}
              onChange={(event) =>
                change("discovery_settings", {
                  ...discoverySettings,
                  local_discovery: event.target.checked,
                })
              }
            />
          </SettingRow>
          <SettingRow label={labels.vpnDiscovery} detail={labels.vpnDiscoveryHint}>
            <input
              type="checkbox"
              checked={discoverySettings.vpn_discovery}
              onChange={(event) =>
                change("discovery_settings", {
                  ...discoverySettings,
                  vpn_discovery: event.target.checked,
                })
              }
            />
          </SettingRow>
          <div className="vpn-helper">
            <span>{labels.vpnFixedHelper}</span>
            <button type="button" className="text-button" onClick={onManageFixedPeers}>
              {labels.manageFixedPeers}
            </button>
          </div>
          <div className="setting-row adapter-summary-row">
            <span>
              <b>{labels.discoveryNetworks}</b>
              <small aria-live="polite">
                {labels.discoverySummary(
                  adapterSummary.enabled,
                  adapterSummary.paused,
                  adapterSummary.excluded,
                )}
              </small>
            </span>
            <button
              type="button"
              className="secondary-button disclosure-button"
              aria-expanded={adaptersExpanded}
              aria-controls="discovery-adapter-manager"
              onClick={() => setAdaptersExpanded((expanded) => !expanded)}
            >
              {adaptersExpanded ? labels.collapse : labels.expand}
            </button>
          </div>
          {adaptersExpanded && (
            <div className="adapter-manager" id="discovery-adapter-manager">
              <div className="adapter-manager-head">
                <span>{labels.adapterManagerHint}</span>
                <button
                  type="button"
                  className="text-button"
                  disabled={networkRefreshing}
                  onClick={refreshNetworkInterfaces}
                >
                  <Icon name="refresh" size={14} spin={networkRefreshing} />
                  {networkRefreshing
                    ? labels.refreshingNetworkList
                    : labels.refreshNetworkList}
                </button>
              </div>
              <div
                className="adapter-list"
                role="group"
                aria-label={labels.discoveryNetworks}
              >
                {networkInterfaces.length === 0 && (
                  <div className="adapter-empty">{labels.noNetworkInterfaces}</div>
                )}
                {networkInterfaces.map((networkInterface) => {
                  const adapterState = discoveryInterfaceState(
                    networkInterface,
                    discoverySettings,
                  );
                  const interfaceName = networkInterface.name || networkInterface.id;
                  const categoryLabel =
                    labels.discoveryCategories[networkInterface.category] ||
                    labels.discoveryCategories.unknown;
                  const addressLabel = networkInterface.addresses.length
                    ? networkInterface.addresses
                        .map((address) =>
                          address.prefix_length === null
                            ? address.ipv4
                            : `${address.ipv4}/${address.prefix_length}`,
                        )
                        .join(", ")
                    : labels.interfaceNoAddress;
                  const riskPending = pendingRiskInterfaceId === networkInterface.id;
                  return (
                    <Fragment key={networkInterface.id}>
                      <div
                        className={`adapter-row ${
                          adapterState.category_disabled ? "category-disabled" : ""
                        }`}
                      >
                        <span
                          className={`adapter-dot ${networkInterface.is_up ? "available" : ""}`}
                          aria-hidden="true"
                        />
                        <span className="adapter-main">
                          <b>{interfaceName}</b>
                          <small>
                            {networkInterface.is_up
                              ? labels.interfaceConnected
                              : labels.interfaceDisconnected}
                            {" · "}
                            <span className="numeric">{addressLabel}</span>
                          </small>
                        </span>
                        <span className="adapter-labels">
                          <span className="adapter-tag">{categoryLabel}</span>
                          <span
                            className={`adapter-tag ${
                              networkInterface.default_enabled ? "recommended" : "excluded"
                            }`}
                          >
                            {networkInterface.default_enabled
                              ? labels.recommended
                              : labels.defaultExcluded}
                          </span>
                        </span>
                        <label className="discovery-switch">
                          <input
                            type="checkbox"
                            checked={adapterState.selected}
                            disabled={adapterState.category_disabled}
                            aria-label={labels.enableDiscoveryOn(interfaceName)}
                            onChange={(event) =>
                              selectDiscoveryInterface(networkInterface, event.target.checked)
                            }
                          />
                          <span aria-hidden="true" />
                        </label>
                      </div>
                      {riskPending && (
                        <div
                          className="adapter-risk"
                          role="group"
                          aria-label={labels.tunRiskTitle(interfaceName)}
                        >
                          <span>
                            <b>{labels.tunRiskTitle(interfaceName)}</b>
                            <small>{labels.tunRiskText}</small>
                          </span>
                          <span className="adapter-risk-actions">
                            <button
                              type="button"
                              className="secondary-button"
                              onClick={() => setPendingRiskInterfaceId(null)}
                            >
                              {labels.keepOff}
                            </button>
                            <button
                              type="button"
                              className="primary-button"
                              onClick={() =>
                                applyInterfaceSelection(networkInterface.id, true)
                              }
                            >
                              {labels.enableAnyway}
                            </button>
                          </span>
                        </div>
                      )}
                    </Fragment>
                  );
                })}
              </div>
              <div className="adapter-manager-foot">
                <span>
                  {networkUpdated
                    ? labels.networkListUpdatedNow
                    : labels.networkListNotRefreshed}
                </span>
                <button
                  type="button"
                  className="text-button"
                  onClick={() => {
                    change("discovery_settings", recommendedDiscoverySettings());
                    setPendingRiskInterfaceId(null);
                  }}
                >
                  {labels.restoreRecommended}
                </button>
              </div>
            </div>
          )}
          {adapterSummary.all_off && (
            <div className="settings-warning" role="alert">
              {labels.discoveryAllOff}
            </div>
          )}
        </section>
        <section className="settings-section" id="settings-shortcut">
          <h2>{labels.shortcuts}</h2>
          <SettingRow
            label={labels.captureShortcut}
            detail={
              state.capabilities.captureShortcut
                ? `${globalThis.window?.__TAURI__ ? labels.captureShortcutGlobalHint : labels.captureShortcutFocusedHint} · ${labels.captureShortcutHint}`
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
        <section className="settings-section settings-about" id="settings-about">
          <h2>{labels.aboutTitle}</h2>
          <div className="about-card">
            <img className="about-logo" src="/app-icon.png" alt="Xchat" />
            <div>
              <b>Xchat</b>
              <p>{labels.aboutDescription}</p>
              <small>{labels.version} {appVersion()}</small>
            </div>
          </div>
        </section>
      </div>
    </main>
  );
}

function LegacyInfoPanel({ state, conversation, workspace, labels, onRemark, onConfirm }) {
  const [newMember, setNewMember] = useState("");
  if (!conversation) return null;
  const peer = conversation.peer;
  const group = conversation.kind === "group";
  const { owner, manager } = groupPermissions(conversation, state.self.id);
  const runGroupAction = (operation, value = null, memberIds = []) =>
    workspace.dispatch({ type: "conversation.updateGroup", operation, value, memberIds });
  const availableMembers = state.devices.filter(
    (device) => !(conversation.members || []).some((member) => member.peer_id === device.id),
  );
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
              <span>
                <b>{member.display_name || member.name}</b>
                <small>{member.role === "owner" ? (labels.locale === "en" ? "Owner" : "群主") : member.role === "admin" ? (labels.locale === "en" ? "Admin" : "管理员") : member.peer_id || member.id}</small>
              </span>
              {owner && member.role !== "owner" && (
                <button onClick={() => runGroupAction(member.role === "admin" ? "remove_admin" : "set_admin", null, [member.peer_id])}>
                  {member.role === "admin" ? (labels.locale === "en" ? "Remove admin" : "取消管理员") : (labels.locale === "en" ? "Make admin" : "设为管理员")}
                </button>
              )}
              {manager && member.role === "member" && member.peer_id !== state.self.id && (
                <button className="danger-text" onClick={() => runGroupAction("remove_members", null, [member.peer_id])}>
                  {labels.locale === "en" ? "Remove" : "移出"}
                </button>
              )}
            </div>
          ))}
          {manager && availableMembers.length > 0 && (
            <div className="group-member-add">
              <select value={newMember} onChange={(event) => setNewMember(event.target.value)}>
                <option value="">{labels.locale === "en" ? "Select a device" : "选择要添加的成员"}</option>
                {availableMembers.map((device) => <option key={device.id} value={device.id}>{displayName(device, labels)}</option>)}
              </select>
              <button disabled={!newMember} onClick={async () => { const result = await runGroupAction("add_members", null, [newMember]); if (result.ok) setNewMember(""); }}>
                {labels.locale === "en" ? "Add" : "添加"}
              </button>
            </div>
          )}
        </section>
      ) : (
        <>
          <div className="drawer-section-label">{labels.deviceInformation}</div>
          <dl className="info-section info-kv">
            <div>
              <dt>{labels.hostname}</dt>
              <dd>{peer?.hostname || peer?.name || labels.notProvided}</dd>
            </div>
            <div>
              <dt>{labels.deviceId}</dt>
              <dd>{peer?.id || labels.notProvided}</dd>
            </div>
            <div>
              <dt>{labels.currentAddress}</dt>
              <dd>{peer?.addr || labels.notProvided}</dd>
            </div>
            <div>
              <dt>{labels.macAddress}</dt>
              <dd>{peer?.mac_address || labels.notProvided}</dd>
            </div>
            <div>
              <dt>{labels.identityVerification}</dt>
              <dd>
                {peer?.is_offline
                  ? labels.identityOfflineStopped
                  : labels.identityVerifiedCurrentAddress}
              </dd>
            </div>
            <div>
              <dt>{labels.discoveryMethod}</dt>
              <dd>{sourceText(peer?.discovery_source, labels)}</dd>
            </div>
          </dl>
          <div className="drawer-section-label">{labels.conversationManagement}</div>
          <div className="info-actions direct-info-actions drawer-setting-list">
            <button onClick={onRemark} disabled={!state.capabilities.deviceMetadata}>
              <Icon name="edit" size={17} />{labels.editDeviceRemark}
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
              <Icon name="trash" size={17} />{labels.clearHistory}
            </button>
          </div>
        </>
      )}
      <div className="info-actions conversation-state-actions">
        {group && owner && (
          <button onClick={() => { const value = prompt(labels.locale === "en" ? "New group name" : "新的群名称", conversation.title); if (value) runGroupAction("rename", value); }}>
            {labels.locale === "en" ? "Rename group" : "修改群名称"}
          </button>
        )}
        {group && manager && (
          <button onClick={() => { const value = prompt(labels.locale === "en" ? "Group announcement" : "群公告"); if (value) runGroupAction("announcement", value); }}>
            {labels.locale === "en" ? "Post announcement" : "发布群公告"}
          </button>
        )}
        {group && (
          <button onClick={() => document.querySelector(".search-box input")?.focus()}>
            {labels.locale === "en" ? "Search messages" : "查找聊天记录"}
          </button>
        )}
        {group && (
          <button className="danger-text" onClick={() => onConfirm({ title: labels.clearHistoryTitle, detail: labels.clearHistoryDetail, action: labels.clearHistoryAction, run: () => workspace.dispatch({ type: "message.clearConversation" }) })}>
            {labels.clearHistory}
          </button>
        )}
        {group && owner && (
          <button className="danger-text" onClick={() => onConfirm({ title: labels.locale === "en" ? "Disband this group?" : "解散这个群聊？", detail: labels.locale === "en" ? "All members will lose this conversation." : "所有成员都将移除此群聊。", action: labels.locale === "en" ? "Disband" : "解散群聊", run: () => runGroupAction("disband") })}>
            {labels.locale === "en" ? "Disband group" : "解散群聊"}
          </button>
        )}
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

function GroupManageModal({ state, conversation, workspace, labels, initialView, onClose, onConfirm }) {
  const [view, setView] = useState(initialView || "members");
  const [query, setQuery] = useState("");
  const [adding, setAdding] = useState(false);
  const [selected, setSelected] = useState([]);
  const [announcement, setAnnouncement] = useState("");
  const [name, setName] = useState(conversation.title || "");
  const members = conversation.members || [];
  const { owner, manager } = groupPermissions(conversation, state.self.id);
  const run = (operation, value = null, memberIds = []) => workspace.dispatch({ type: "conversation.updateGroup", operation, value, memberIds });
  useEffect(() => setName(conversation.title || ""), [conversation.id, conversation.title]);
  const memberRows = members.filter((member) => `${member.display_name || member.name} ${member.peer_id}`.toLocaleLowerCase().includes(query.toLocaleLowerCase()));
  const candidates = state.devices.filter((device) => !members.some((member) => member.peer_id === device.id) && `${displayName(device, labels)} ${device.hostname || ""} ${device.mac_address || ""}`.toLocaleLowerCase().includes(query.toLocaleLowerCase()));
  const toggle = (id) => setSelected((current) => current.includes(id) ? current.filter((item) => item !== id) : [...current, id]);
  return (
    <Modal title={labels.locale === "en" ? "Group management" : "群聊管理"} closeLabel={labels.close} onClose={onClose} wide>
      <div className="group-manage-layout">
        <nav className="group-manage-nav">
          <button className={view === "members" ? "active" : ""} onClick={() => { setView("members"); setAdding(false); setQuery(""); }}><Icon name="user" />{labels.locale === "en" ? "Members" : "群成员"}</button>
          <button className={view === "announcement" ? "active" : ""} onClick={() => { setView("announcement"); setQuery(""); }}><Icon name="chat" />{labels.locale === "en" ? "Announcement" : "群公告"}</button>
          <button className={view === "settings" ? "active" : ""} onClick={() => { setView("settings"); setQuery(""); }}><Icon name="settings" />{labels.locale === "en" ? "Settings" : "群聊设置"}</button>
        </nav>
        <section className="group-manage-main">
          {view === "members" && (
            <>
              <div className="section-head"><span><h3>{adding ? (labels.locale === "en" ? "Add members" : "添加群成员") : `${labels.locale === "en" ? "Members" : "群成员"} · ${members.length}`}</h3><small>{manager ? (labels.locale === "en" ? "Owners and admins can manage members" : "群主和管理员可以增减群成员") : (labels.locale === "en" ? "Only owners and admins can manage members" : "仅群主和管理员可管理成员")}</small></span>{manager && <button className="primary" onClick={() => { setAdding((value) => !value); setSelected([]); setQuery(""); }}>{adding ? labels.cancel : (labels.locale === "en" ? "Add members" : "添加成员")}</button>}</div>
              <label className="modal-search"><Icon name="search" size={16} /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder={adding ? (labels.locale === "en" ? "Search available contacts" : "搜索可添加的联系人") : (labels.locale === "en" ? "Search members" : "搜索群成员")} /></label>
              <div className="member-manage-list">
                {(adding ? candidates : memberRows).map((member) => {
                  const id = adding ? member.id : member.peer_id;
                  const role = adding ? "member" : member.role;
                  const entity = adding ? member : { id, name: member.display_name || member.name };
                  return adding ? (
                    <button className={`candidate-row ${selected.includes(id) ? "selected" : ""}`} key={id} onClick={() => toggle(id)}><span className="forward-check">✓</span><Avatar entity={entity} labels={labels} /><span><b>{displayName(entity, labels)}</b><small>{entity.hostname || entity.addr || id}</small></span></button>
                  ) : (
                    <div className="manage-member-row" key={id}><Avatar entity={entity} labels={labels} /><span><b>{displayName(entity, labels)}</b>{role === "owner" && <i className="role-tag owner">{labels.locale === "en" ? "Owner" : "群主"}</i>}{role === "admin" && <i className="role-tag">{labels.locale === "en" ? "Admin" : "管理员"}</i>}<small>{id === state.self.id ? (labels.locale === "en" ? "This device" : "本机设备") : id}</small></span><span className="member-actions">{owner && role !== "owner" && <button onClick={() => run(role === "admin" ? "remove_admin" : "set_admin", null, [id])}>{role === "admin" ? (labels.locale === "en" ? "Remove admin" : "取消管理员") : (labels.locale === "en" ? "Make admin" : "设为管理员")}</button>}{manager && role === "member" && id !== state.self.id && <button className="danger-text" onClick={() => onConfirm({ title: labels.locale === "en" ? "Remove this member?" : "移出群成员？", detail: labels.locale === "en" ? "They will no longer receive messages from this group." : `确定将“${displayName(entity, labels)}”移出群聊吗？`, action: labels.locale === "en" ? "Remove" : "移出群聊", run: () => run("remove_members", null, [id]) })}>{labels.locale === "en" ? "Remove" : "移出"}</button>}</span></div>
                  );
                })}
                {!(adding ? candidates : memberRows).length && <div className="history-empty">{labels.locale === "en" ? "No matching members" : "没有符合条件的成员"}</div>}
              </div>
              {adding && <div className="manage-footer"><span>{labels.locale === "en" ? `${selected.length} selected` : `已选择 ${selected.length} 人`}</span><button className="primary" disabled={!selected.length} onClick={async () => { const result = await run("add_members", null, selected); if (result.ok) { setAdding(false); setSelected([]); setQuery(""); } }}>{labels.locale === "en" ? "Add" : "添加"}</button></div>}
            </>
          )}
          {view === "announcement" && (
            <><div className="section-head"><span><h3>{labels.locale === "en" ? "Group announcement" : "群公告"}</h3><small>{labels.locale === "en" ? "Published announcements stay pinned above the conversation" : "发布后会固定显示在群聊顶部"}</small></span></div><textarea className="announcement-editor" value={announcement} maxLength={2000} disabled={!manager} onChange={(event) => setAnnouncement(event.target.value)} placeholder={labels.locale === "en" ? "Write an announcement…" : "输入群公告内容…"} /><div className="announcement-note"><span>{manager ? (labels.locale === "en" ? "All members will see this banner" : "所有成员都会看到公告横幅") : (labels.locale === "en" ? "Only owners and admins can publish" : "仅群主和管理员可发布")}</span><span>{announcement.length} / 2000</span></div>{manager && <div className="manage-footer"><span /><button className="primary" disabled={!announcement.trim()} onClick={async () => { const result = await run("announcement", announcement); if (result.ok) onClose(); }}>{labels.locale === "en" ? "Publish" : "发布公告"}</button></div>}</>
          )}
          {view === "settings" && (
            <><div className="section-head"><span><h3>{labels.locale === "en" ? "Group settings" : "群聊设置"}</h3><small>{labels.locale === "en" ? "Only the owner can rename or disband the group" : "仅群主可以修改群名称和解散群聊"}</small></span></div><div className="group-setting-row"><span><b>{labels.locale === "en" ? "Group name" : "群聊名称"}</b><small>{conversation.title}</small></span><input value={name} maxLength={80} disabled={!owner} onChange={(event) => setName(event.target.value)} /><button disabled={!owner || !name.trim() || name.trim() === conversation.title} onClick={async () => { const nextName = name.trim(); const result = await run("rename", nextName); if (result.ok) setName(nextName); }}>{labels.locale === "en" ? "Save" : "保存"}</button></div><div className="group-setting-row"><span><b>{labels.clearHistory}</b><small>{labels.locale === "en" ? "Only removes records on this device" : "仅删除本机记录，不影响其他成员"}</small></span><button className="danger-text" onClick={() => onConfirm({ title: labels.clearHistoryTitle, detail: labels.clearHistoryDetail, action: labels.clearHistoryAction, run: () => workspace.dispatch({ type: "message.clearConversation" }) })}>{labels.locale === "en" ? "Clear" : "清空"}</button></div>{owner && <div className="group-setting-row danger-zone"><span><b>{labels.locale === "en" ? "Disband group" : "解散群聊"}</b><small>{labels.locale === "en" ? "All members will be removed permanently" : "所有成员都将退出且无法恢复"}</small></span><button className="danger" onClick={() => onConfirm({ title: labels.locale === "en" ? "Disband this group?" : "解散这个群聊？", detail: labels.locale === "en" ? "This cannot be undone." : "解散后无法恢复。", action: labels.locale === "en" ? "Disband" : "解散群聊", run: async () => { await run("disband"); onClose(); } })}>{labels.locale === "en" ? "Disband" : "解散"}</button></div>}</>
          )}
        </section>
      </div>
    </Modal>
  );
}

function InfoPanel({ state, conversation, workspace, labels, onRemark, onConfirm }) {
  const [manageView, setManageView] = useState("");
  if (!conversation) return null;
  const group = conversation.kind === "group";
  const members = conversation.members || [];
  const { manager } = groupPermissions(conversation, state.self.id);
  if (!group) return <LegacyInfoPanel state={state} conversation={conversation} workspace={workspace} labels={labels} onRemark={onRemark} onConfirm={onConfirm} />;
  const open = (view) => setManageView(view);
  return (
    <aside className="info-panel group-drawer" data-od-id="conversation-information">
      <section className="group-info-card"><Avatar entity={conversation} labels={labels} large /><h3>{displayName(conversation, labels)}</h3><p>{labels.memberCount(members.length)}</p></section>
      <div className="group-member-peek">
        {members.slice(0, 7).map((member) => <button className="member-tile" key={member.peer_id} onClick={() => open("members")}><Avatar entity={{ id: member.peer_id, name: member.display_name || member.name }} labels={labels} /><span>{member.display_name || member.name}</span></button>)}
        {manager && <button className="member-tile" onClick={() => open("members")}><span className="member-add-tile"><Icon name="plus" /></span><span>{labels.locale === "en" ? "Add" : "添加"}</span></button>}
      </div>
      <div className="group-quick-actions drawer-setting-list">
        <button onClick={() => open("members")}><Icon name="user" />{labels.locale === "en" ? "View members" : "查看群成员"}</button>
        <button onClick={() => dispatchEvent(new Event("xchat:open-history"))}><Icon name="search" />{labels.locale === "en" ? "Search history" : "查找聊天记录"}</button>
        {manager && <button onClick={() => open("announcement")}><Icon name="chat" />{labels.locale === "en" ? "Announcement" : "发布群公告"}</button>}
        <button onClick={() => open("settings")}><Icon name="settings" />{labels.locale === "en" ? "Group settings" : "群聊设置"}</button>
      </div>
      <div className="info-actions conversation-state-actions"><button disabled={!state.capabilities.conversationState} onClick={() => workspace.dispatch({ type: "conversation.pin", id: conversation.id, value: !conversation.pinned })}>{conversation.pinned ? labels.unpinConversation : labels.pinConversation}</button><button disabled={!state.capabilities.conversationState} onClick={() => workspace.dispatch({ type: "conversation.markUnread", id: conversation.id, value: !conversation.forced_unread })}>{conversation.forced_unread ? labels.unmarkUnread : labels.markUnread}</button></div>
      {manageView && <GroupManageModal state={state} conversation={conversation} workspace={workspace} labels={labels} initialView={manageView} onClose={() => setManageView("")} onConfirm={onConfirm} />}
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
  const [testedInput, setTestedInput] = useState("");
  const [testResult, setTestResult] = useState(null);
  const [testError, setTestError] = useState("");
  const [testing, setTesting] = useState(false);
  const [saving, setSaving] = useState(false);
  const candidate = endpoint.trim();
  const valid = Boolean(candidate && !/[\s/?#]/.test(candidate));
  const canSave = canSaveVerifiedEndpoint(endpoint, testedInput, testResult);
  const shortId = (deviceId) => {
    if (!deviceId) return labels.notProvided;
    return deviceId.length > 12
      ? `${deviceId.slice(0, 4)}…${deviceId.slice(-4)}`
      : deviceId;
  };
  const test = async (record = null) => {
    const value = String(record?.endpoint ?? endpoint).trim();
    if (!value || /[\s/?#]/.test(value) || testing) return;
    if (record) setEndpoint(value);
    setTestedInput(value);
    setTestResult(null);
    setTestError("");
    setTesting(true);
    try {
      const result = await workspace.dispatch({
        type: "device.testEndpoint",
        endpoint: value,
        expectedDeviceId: record?.device_id ?? null,
      });
      if (result.ok) setTestResult(result.data);
      else setTestError(result.error?.message || labels.endpointTestFailed);
    } finally {
      setTesting(false);
    }
  };
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
            disabled={!canSave || saving}
            onClick={async () => {
              setSaving(true);
              const result = await workspace.dispatch({
                type: "device.saveEndpoint",
                endpoint: testResult.endpoint || endpoint.trim(),
                expectedDeviceId: testResult.identity.device_id,
              });
              if (result.ok) {
                onClose();
              } else {
                setTestError(result.error?.message || labels.endpointTestFailed);
                setTestResult(null);
              }
              setSaving(false);
            }}
          >
            {labels.saveDevice}
          </button>
        </>
      }
    >
      <p className="endpoint-subtitle">{labels.endpointSubtitle}</p>
      <label className="field">
        <span>{labels.deviceAddress}</span>
        <span className="endpoint-test-row">
          <input
            className="numeric"
            value={endpoint}
            onChange={(event) => {
              setEndpoint(event.target.value);
              setTestResult(null);
              setTestError("");
            }}
            placeholder={labels.endpointPlaceholder}
            autoFocus
          />
          <button
            type="button"
            className="secondary-button"
            disabled={!valid || testing}
            onClick={() => test()}
          >
            {testing ? labels.testingConnection : labels.testConnection}
          </button>
        </span>
      </label>
      <p className="helper">{labels.endpointHelper}</p>
      {(testing || testResult || testError) && (
        <div
          className={`endpoint-test-result ${
            testResult?.identity_matches ? "success" : testResult || testError ? "error" : "testing"
          }`}
          role="status"
          aria-live="polite"
        >
          {testing && <b>{labels.testingConnection}</b>}
          {testError && <><b>{labels.endpointTestFailed}</b><small>{testError}</small></>}
          {testResult?.identity_matches && (
            <>
              <b>{testResult.identity.name || testResult.identity.hostname || labels.identityConfirmed}</b>
              <span>{labels.endpointTestSuccess}</span>
              <small className="numeric">
                {labels.deviceId} {testResult.identity.device_id} · {labels.currentAddress}{" "}
                {testResult.address || testResult.endpoint}
              </small>
            </>
          )}
          {testResult && !testResult.identity_matches && (
            <>
              <b>{labels.endpointTestMismatch}</b>
              <small className="numeric">
                {labels.deviceId} {testResult.identity?.device_id || labels.notProvided}
              </small>
            </>
          )}
        </div>
      )}
      {state.settings.custom_peers?.length > 0 && (
        <section className="endpoint-list">
          <header>
            <b>{labels.savedFixedAddresses}</b>
            <small>{labels.fixedAddressSafety}</small>
          </header>
          {state.settings.custom_peers.map((peer) => (
            <div className="endpoint-record" key={peer.endpoint}>
              <span className="endpoint-record-main">
                <span>
                  <b>{peer.name || peer.hostname || peer.endpoint}</b>
                  <i className={peer.verified ? "verified" : "unverified"}>
                    {peer.verified ? labels.identityConfirmed : labels.identityNeedsTest}
                  </i>
                </span>
                <small className="numeric">
                  {peer.endpoint}
                  {peer.device_id ? ` · ${labels.deviceId} ${shortId(peer.device_id)}` : ""}
                </small>
              </span>
              <span className="endpoint-record-actions">
                <button className="text-action" type="button" onClick={() => test(peer)}>
                  {labels.retest}
                </button>
                <button
                  className="text-action danger-text"
                  onClick={() =>
                    workspace.dispatch({
                      type: "device.removeEndpoint",
                      endpoint: peer.endpoint,
                    })
                  }
                >
                  {labels.delete}
                </button>
              </span>
            </div>
          ))}
        </section>
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
  if (requestedView === "strong-reminder") return <StrongReminderWindow />;
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
    const clearAttention = () => {
      if (isAppActive(document.visibilityState, document.hasFocus())) {
        workspace.dispatch({ type: "attention.clear" });
      }
    };
    addEventListener("focus", clearAttention);
    document.addEventListener("visibilitychange", clearAttention);
    clearAttention();
    return () => {
      removeEventListener("focus", clearAttention);
      document.removeEventListener("visibilitychange", clearAttention);
    };
  }, [workspace]);

  useEffect(() => {
    if (
      !state.capabilities.captureShortcut ||
      nativeCaptureShortcutAvailable()
    ) {
      return;
    }
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
        ?.scrollIntoView({ behavior: "auto", block: "start" }),
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
          onManageFixedPeers={() => setModal("endpoint")}
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
      {state.strongReminder && !globalThis.window?.__TAURI__ && (
        <div className="strong-reminder-fallback">
          <StrongReminderCard
            embedded
            payload={state.strongReminder}
            onOpen={() => workspace.dispatch({
              type: "strongReminder.open",
              conversationId: state.strongReminder.conversation_id,
              clientMessageId: state.strongReminder.client_message_id,
            })}
            onDismiss={() => workspace.dispatch({ type: "strongReminder.dismiss" })}
          />
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
