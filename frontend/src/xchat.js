const MESSAGE_STATUS = ["pending", "sent", "delivered", "read"];
const EVENT_NAMES = [
  "workspace-changed",
  "device-changed",
  "conversation-changed",
  "message-changed",
  "receipt-changed",
  "transfer-changed",
  "settings-changed",
  "new-peer",
  "new-message",
  "messages-resent",
  "upload_progress",
  "file_status_update",
  "file_download_progress",
  "notifications-changed",
  "device.changed",
  "conversation.changed",
  "message.changed",
  "receipt.changed",
  "transfer.changed",
  "settings.changed",
  "capture-finished",
  "capture.finished",
  "capture-ready",
];

const IMAGE_EXTENSIONS = new Set([
  "avif",
  "bmp",
  "gif",
  "jpeg",
  "jpg",
  "png",
  "webp",
]);
const DOCUMENT_EXTENSIONS = new Set([
  "csv",
  "doc",
  "docx",
  "json",
  "md",
  "pdf",
  "ppt",
  "pptx",
  "rtf",
  "txt",
  "xls",
  "xlsx",
]);
const AUDIO_EXTENSIONS = new Set(["aac", "flac", "m4a", "mp3", "ogg", "wav"]);
const VIDEO_EXTENSIONS = new Set(["avi", "mkv", "mov", "mp4", "webm"]);

export const EMOJI_SET = [
  "😀", "😃", "😄", "😁", "😆", "😅", "😂", "🤣", "😊",
  "😇", "🙂", "🙃", "😉", "😌", "😍", "🥰", "😘", "😋",
  "😛", "😝", "😜", "🤪", "🤨", "🧐", "🤓", "😎", "🤩",
  "🥳", "😏", "😒", "😞", "😔", "😟", "😕", "🙁", "😣",
  "😖", "😫", "😩", "🥺", "😢", "😭", "😤", "😠", "😡",
  "🤬", "🤯", "😳", "🥵", "🥶", "😱", "😨", "😰", "😥",
  "🤔", "🤭", "🤫", "🤥", "😶", "😐", "😑", "😬", "🙄",
  "😯", "😮", "😲", "🥱", "😴", "🤤", "😵", "🤐", "🤢",
  "👍", "👎", "👌", "🤌", "✌️", "🤞", "🤟", "🤘", "🤙",
  "👈", "👉", "👆", "👇", "☝️", "✋", "👋", "🤝", "👏",
  "🙏", "💪", "❤️", "🧡", "💛", "💚", "💙", "💜", "💔",
  "✅", "❌", "🔥", "⭐", "✨", "🎉", "🎁", "📎", "🚀",
];

export const ACTIVE_TRANSFER_STATES = new Set([
  "queued",
  "waiting_peer",
  "offering",
  "awaiting_acceptance",
  "transferring",
  "receiving",
  "uploading",
  "downloading",
  "cancelling",
]);

class TransportError extends Error {
  constructor(message, code = "transport_error", status = 0, retryable = true) {
    super(message);
    this.code = code;
    this.status = status;
    this.retryable = retryable;
  }
}

const storage = {
  get(key) {
    try {
      return globalThis.localStorage?.getItem(key) ?? null;
    } catch {
      return null;
    }
  },
  set(key, value) {
    try {
      globalThis.localStorage?.setItem(key, value);
    } catch {
      // Local preferences are optional; transport-backed settings still work.
    }
  },
};

function uiCopy(zh, en) {
  const language =
    globalThis.document?.documentElement?.lang || storage.get("xchat.language") || "zh-CN";
  return language.toLowerCase().startsWith("en") ? en : zh;
}

function errorText(error) {
  if (typeof error === "string") return error;
  return error?.message || uiCopy("操作失败", "Operation failed");
}

function unavailable(error) {
  return error?.status === 404 || /not found|unknown command|not allowed/i.test(errorText(error));
}

function valueOf(result, fallback) {
  return result.status === "fulfilled" ? result.value : fallback;
}

function toSeconds(value) {
  const number = Number(value || 0);
  return number > 10_000_000_000 ? Math.floor(number / 1000) : number;
}

function humanPlatform() {
  const agent = globalThis.navigator?.userAgent || "";
  if (/Android/i.test(agent)) return "android";
  if (/Macintosh|Mac OS X/i.test(agent)) return "macos";
  if (/Windows/i.test(agent)) return "windows";
  if (/Linux/i.test(agent)) return "linux";
  return "unknown";
}

export function runtimeCapabilities(runtime, supplied = {}, legacy = false) {
  const platform = humanPlatform();
  const webCapture = Boolean(globalThis.navigator?.mediaDevices?.getDisplayMedia);
  const defaults = {
    capture: runtime === "web" ? webCapture : platform === "macos",
    captureShortcut: runtime === "web" ? webCapture : platform === "macos",
    revealFile: runtime === "tauri" && platform !== "android",
    openOutgoingFile: runtime === "tauri",
    notifications:
      runtime === "web"
        ? "Notification" in globalThis
        : platform === "android" || platform === "windows" || platform === "linux",
    groupChat: !legacy,
    readReceipts: !legacy,
    conversationState: !legacy,
    messageSearch: !legacy,
    fileCenter: !legacy,
    transferCancel: !legacy,
    deviceMetadata: !legacy,
    nativeFilePicker: runtime === "tauri",
  };
  const capabilities = { ...defaults, ...supplied };
  if (runtime === "web") {
    capabilities.capture = webCapture;
    capabilities.captureShortcut = webCapture;
    capabilities.notifications = "Notification" in globalThis;
    capabilities.revealFile = false;
    capabilities.openOutgoingFile = false;
    capabilities.nativeFilePicker = false;
  }
  return capabilities;
}

function normalizeCapabilities(raw = {}) {
  const mapped = {
    groupChat: raw.groupChat ?? raw.groups,
    readReceipts: raw.readReceipts ?? raw.receipts,
    fileCenter: raw.fileCenter ?? raw.file_center,
    transferCancel: raw.transferCancel ?? raw.transfer_cancel,
    deviceMetadata: raw.deviceMetadata ?? raw.device_metadata,
    conversationState: raw.conversationState ?? raw.conversation_state,
    messageSearch: raw.messageSearch ?? raw.message_search,
    captureShortcut: raw.captureShortcut ?? raw.capture_shortcut,
    revealFile: raw.revealFile ?? raw.reveal_file,
    nativeFilePicker: raw.nativeFilePicker ?? raw.native_file_picker,
  };
  return {
    ...raw,
    ...Object.fromEntries(Object.entries(mapped).filter(([, value]) => value !== undefined)),
  };
}

export function directConversationId(selfId, peerId) {
  const ids = [selfId, peerId].filter(Boolean).sort();
  return ids.length === 2 ? `direct:${ids[0]}:${ids[1]}` : `direct:${peerId}`;
}

export function matchesShortcut(event, label = "") {
  const value = label.toLocaleLowerCase().replaceAll(" ", "");
  const eitherPrimary =
    value.includes("ctrl/⌘") ||
    value.includes("⌘/ctrl") ||
    value.includes("commandorcontrol") ||
    value.includes("cmdorctrl");
  const wantsMeta = !eitherPrimary && /⌘|cmd|command/.test(value);
  const wantsControl = !eitherPrimary && /ctrl|⌃|control/.test(value);
  const wantsShift = /⇧|shift/.test(value);
  const wantsAlt = /⌥|alt|option/.test(value);
  const key = value.match(/([a-z0-9])$/)?.[1];
  return Boolean(
    key &&
      event.key?.toLocaleLowerCase() === key &&
      (eitherPrimary ? event.metaKey || event.ctrlKey : event.metaKey === wantsMeta) &&
      (eitherPrimary || event.ctrlKey === wantsControl) &&
      event.shiftKey === wantsShift &&
      event.altKey === wantsAlt,
  );
}

export function shortcutLabelFromEvent(event) {
  if (!/^[a-z0-9]$/i.test(event.key || "")) return "";
  return [
    event.metaKey && "⌘",
    event.ctrlKey && "⌃",
    event.altKey && "⌥",
    event.shiftKey && "⇧",
    event.key.toLocaleUpperCase(),
  ]
    .filter(Boolean)
    .join(" ");
}

export function insertTextAtSelection(value, inserted, selectionStart, selectionEnd) {
  const text = String(value ?? "");
  const start = Math.max(0, Math.min(Number(selectionStart) || 0, text.length));
  const end = Math.max(start, Math.min(Number(selectionEnd) || start, text.length));
  const addition = String(inserted ?? "");
  return {
    value: `${text.slice(0, start)}${addition}${text.slice(end)}`,
    caret: start + addition.length,
  };
}

export function isPhysicalPointInsideRect(position, rect, scale = 1) {
  if (!position || !rect) return false;
  const ratio = Number(scale) > 0 ? Number(scale) : 1;
  const x = Number(position.x) / ratio;
  const y = Number(position.y) / ratio;
  return (
    Number.isFinite(x) &&
    Number.isFinite(y) &&
    x >= rect.left &&
    x <= rect.right &&
    y >= rect.top &&
    y <= rect.bottom
  );
}

export function measureTransfers(previous = [], current = [], elapsedMs = 0) {
  const before = new Map(previous.map((transfer) => [transfer.id, transfer]));
  const elapsedSeconds = Number(elapsedMs) > 0 ? Number(elapsedMs) / 1000 : 0;
  return current.map((transfer) => {
    const bytesTotal = Math.max(0, Number(transfer.bytes_total ?? 0));
    const bytesTransferred = Math.max(
      0,
      Math.min(Number(transfer.bytes_transferred ?? 0), bytesTotal || Infinity),
    );
    const prior = before.get(transfer.id);
    const speed =
      prior && elapsedSeconds > 0 && ACTIVE_TRANSFER_STATES.has(transfer.status)
        ? Math.max(0, bytesTransferred - Number(prior.bytes_transferred ?? 0)) /
          elapsedSeconds
        : 0;
    return {
      ...transfer,
      bytes_total: bytesTotal,
      bytes_transferred: bytesTransferred,
      progress_percent: bytesTotal
        ? Math.min(100, Math.round((bytesTransferred / bytesTotal) * 100))
        : 0,
      speed_bps: Math.round(speed),
    };
  });
}

export function normalizeMessage(raw = {}, selfId = "", conversationId = "") {
  const senderId = raw.sender_id ?? raw.from_id ?? "";
  const own = Boolean(raw.own ?? raw.is_self ?? senderId === "me" ?? false) ||
    (Boolean(selfId) && senderId === selfId);
  const clientMessageId =
    raw.client_message_id ?? raw.clientMessageId ?? raw.sender_msg_id ?? null;
  const id = raw.id ?? clientMessageId ?? `${senderId}:${raw.timestamp ?? 0}:${raw.content ?? ""}`;
  const msgType = raw.msg_type ?? raw.type ?? "text";
  return {
    ...raw,
    id,
    client_message_id: clientMessageId,
    conversation_id: raw.conversation_id ?? conversationId,
    sender_id: senderId,
    sender_name: raw.sender_name ?? raw.from_name ?? "",
    content: raw.content ?? "",
    msg_type: msgType,
    timestamp: toSeconds(raw.timestamp ?? raw.created_at),
    status: raw.status || (own ? "sent" : "received"),
    own,
    file_name: raw.file_name ?? (msgType === "file" ? raw.content : ""),
    file_path: raw.file_path ?? "",
    file_size: Number(raw.file_size ?? raw.bytes_total ?? 0),
    file_status: raw.file_status ?? "",
    mime_type: raw.mime_type ?? raw.content_type ?? "",
    local_available:
      raw.local_available ?? raw.local_exists ?? raw.file_available ?? undefined,
    delivered_count: Number(raw.delivered_count ?? 0),
    read_count: Number(raw.read_count ?? 0),
    recipient_count: Number(raw.recipient_count ?? 0),
  };
}

export function fileStatus(file = {}) {
  return file.file_status || file.status || "";
}

function fileExtension(file = {}) {
  const name = file.file_name ?? file.name ?? file.content ?? "";
  return String(name).split(".").pop()?.toLocaleLowerCase() || "";
}

export function fileKind(file = {}) {
  const mime = String(file.mime_type ?? file.type ?? "").toLocaleLowerCase();
  if (mime.startsWith("image/") || IMAGE_EXTENSIONS.has(fileExtension(file))) return "image";
  if (mime.startsWith("audio/") || AUDIO_EXTENSIONS.has(fileExtension(file))) return "audio";
  if (mime.startsWith("video/") || VIDEO_EXTENSIONS.has(fileExtension(file))) return "video";
  if (DOCUMENT_EXTENSIONS.has(fileExtension(file))) return "document";
  return "other";
}

export function isImageFile(file = {}) {
  return fileKind(file) === "image";
}

export function localFileAvailable(file = {}) {
  const explicit =
    file.local_available ?? file.local_exists ?? file.file_available;
  if (explicit !== undefined && explicit !== null) return Boolean(explicit);
  return !["invalid", "removed"].includes(fileStatus(file));
}

function draftId() {
  return globalThis.crypto?.randomUUID?.() ?? `${Date.now()}:${Math.random()}`;
}

function draftName(source, fallback = "attachment") {
  if (typeof source === "string") return source.split(/[\\/]/).pop() || fallback;
  return source?.file_name ?? source?.name ?? fallback;
}

function normalizeDraftAttachment(source = {}, extra = {}) {
  const path = typeof source === "string" ? source : source.path ?? source.file_path ?? "";
  const file = typeof File !== "undefined" && source instanceof File ? source : source.file;
  const name = extra.file_name ?? extra.name ?? draftName(source);
  return {
    ...source,
    ...extra,
    id: extra.id ?? source.id ?? draftId(),
    file_name: name,
    name,
    file_size: Number(extra.file_size ?? source.file_size ?? source.size ?? file?.size ?? 0),
    mime_type: extra.mime_type ?? source.mime_type ?? source.type ?? file?.type ?? "",
    path: extra.path ?? extra.file_path ?? path,
    file_path: extra.file_path ?? extra.path ?? path,
    file: file ?? (typeof source === "object" && source?.arrayBuffer ? source : undefined),
  };
}

function dataUrlFromFile(file) {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result);
    reader.onerror = () => reject(reader.error || new Error("Unable to read file"));
    reader.readAsDataURL(file);
  });
}

async function pngDataUrlFromFile(file) {
  if (file.type === "image/png") return dataUrlFromFile(file);
  const bitmap = await createImageBitmap(file);
  const canvas = document.createElement("canvas");
  canvas.width = bitmap.width;
  canvas.height = bitmap.height;
  canvas.getContext("2d").drawImage(bitmap, 0, 0);
  bitmap.close();
  return canvas.toDataURL("image/png");
}

function bytesDataUrl(bytes, mimeType) {
  let binary = "";
  const value = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  for (let index = 0; index < value.length; index += 0x8000) {
    binary += String.fromCharCode(...value.subarray(index, index + 0x8000));
  }
  return `data:${mimeType};base64,${btoa(binary)}`;
}

function imageMime(file) {
  const extension = fileExtension(file);
  if (extension === "jpg" || extension === "jpeg") return "image/jpeg";
  if (extension === "gif") return "image/gif";
  if (extension === "webp") return "image/webp";
  if (extension === "bmp") return "image/bmp";
  if (extension === "avif") return "image/avif";
  return "image/png";
}

function messageKey(message) {
  return message.client_message_id
    ? `client:${message.client_message_id}`
    : `id:${message.id}`;
}

function monotonicStatus(previous, next) {
  const before = MESSAGE_STATUS.indexOf(previous);
  const after = MESSAGE_STATUS.indexOf(next);
  if (before >= 0 && after >= 0) return MESSAGE_STATUS[Math.max(before, after)];
  return next || previous;
}

export function mergeMessages(existing = [], incoming = []) {
  const merged = new Map(existing.map((message) => [messageKey(message), message]));
  for (const message of incoming) {
    const key = messageKey(message);
    const previous = merged.get(key);
    merged.set(
      key,
      previous
        ? {
            ...previous,
            ...message,
            status: monotonicStatus(previous.status, message.status),
            delivered_count: Math.max(previous.delivered_count || 0, message.delivered_count || 0),
            read_count: Math.max(previous.read_count || 0, message.read_count || 0),
          }
        : message,
    );
  }
  return [...merged.values()].sort(
    (a, b) => (a.timestamp || 0) - (b.timestamp || 0) || String(a.id).localeCompare(String(b.id)),
  );
}

function normalizeDevice(raw = {}) {
  return {
    ...raw,
    id: raw.id ?? "",
    name: raw.name ?? raw.hostname ?? "",
    remark: raw.remark ?? "",
    hostname: raw.hostname ?? raw.name ?? "",
    addr: raw.addr ?? raw.address ?? "",
    mac_address: raw.mac_address ?? raw.mac ?? "",
    discovery_source: raw.discovery_source ?? (raw.manual ? "manual" : "udp"),
    is_offline: Boolean(raw.is_offline ?? raw.offline ?? false),
    last_seen: Number(raw.last_seen ?? 0),
    available_memory_mb: Number(raw.available_memory_mb ?? 0),
  };
}

function normalizeConversation(raw = {}, devices = []) {
  const peer = devices.find((device) => device.id === raw.peer_id);
  return {
    ...raw,
    id: raw.id ?? "",
    kind: raw.kind ?? (raw.members?.length ? "group" : "direct"),
    peer_id: raw.peer_id ?? null,
    title: raw.title ?? raw.name ?? peer?.remark ?? peer?.name ?? "",
    pinned: Boolean(raw.pinned),
    forced_unread: Boolean(raw.forced_unread),
    draft: raw.draft ?? "",
    unread_count: Number(raw.unread_count ?? 0),
    last_message: raw.last_message ?? raw.preview ?? "",
    last_message_at: Number(raw.last_message_at ?? raw.updated_at ?? 0),
    members: raw.members ?? [],
  };
}

function normalizeSettings(raw = {}) {
  return {
    name: raw.name ?? "",
    avatar: raw.avatar ?? storage.get("xchat.avatar") ?? "",
    theme: raw.theme ?? raw.current_theme ?? storage.get("xchat.theme") ?? "system",
    language: raw.language ?? storage.get("xchat.language") ?? "zh-CN",
    notifications_enabled: Boolean(raw.notifications_enabled ?? true),
    download_path: raw.download_path ?? "",
    auto_download: Boolean(raw.auto_download ?? false),
    port: String(raw.port ?? ""),
    db_path: raw.db_path ?? "",
    capture_shortcut:
      raw.capture_shortcut ??
      storage.get("xchat.captureShortcut") ??
      (humanPlatform() === "macos" ? "⌘ ⇧ A" : "Ctrl/⌘ ⇧ A"),
    custom_peers: raw.custom_peers ?? [],
  };
}

function normalizeWorkspace(raw, previous, runtime) {
  const source = raw?.snapshot ?? raw ?? {};
  const sampledAt = globalThis.performance?.now?.() ?? Date.now();
  const self = {
    id: source.self?.id ?? source.self_id ?? "",
    name: source.self?.name ?? source.self_name ?? source.settings?.name ?? "",
    hostname: source.self?.hostname ?? "",
    mac_address: source.self?.mac_address ?? "",
    addr: source.self?.addr ?? "",
    avatar: source.self?.avatar ?? "",
  };
  const devices = (source.devices ?? source.peers ?? []).map(normalizeDevice);
  const conversations = (source.conversations ?? []).map((item) =>
    normalizeConversation(item, devices),
  );
  const settings = normalizeSettings({ ...source.settings, name: source.settings?.name ?? self.name });
  return {
    ...previous,
    phase: "ready",
    self: { ...self, avatar: self.avatar || settings.avatar },
    conversations,
    devices,
    files: source.files ?? [],
    transfers: measureTransfers(
      previous.transfers,
      source.transfers ?? [],
      previous.transfer_sample_at ? sampledAt - previous.transfer_sample_at : 0,
    ),
    transfer_sample_at: sampledAt,
    settings,
    capabilities: runtimeCapabilities(
      runtime,
      normalizeCapabilities(source.capabilities),
      Boolean(source.legacy),
    ),
    activeConversationId: conversations.some((item) => item.id === previous.activeConversationId)
      ? previous.activeConversationId
      : conversations[0]?.id ?? null,
  };
}

async function parseResponse(response) {
  const text = await response.text();
  let data = null;
  if (text) {
    try {
      data = JSON.parse(text);
    } catch {
      data = text;
    }
  }
  if (!response.ok) {
    throw new TransportError(
      data?.error ||
        data?.message ||
        uiCopy(`请求失败 (${response.status})`, `Request failed (${response.status})`),
      data?.code || `http_${response.status}`,
      response.status,
      response.status >= 500,
    );
  }
  return data;
}

class TauriAdapter {
  constructor(tauri) {
    this.tauri = tauri;
    this.runtime = "tauri";
  }

  invoke(command, payload) {
    return this.tauri.core.invoke(command, payload);
  }

  async getSnapshot() {
    try {
      return await this.invoke("get_workspace_snapshot");
    } catch (error) {
      if (!unavailable(error)) throw error;
      return this.getLegacySnapshot();
    }
  }

  async getLegacySnapshot() {
    const [id, name, peers, settings, customPeers, files, transfers] = await Promise.allSettled([
      this.invoke("get_my_id"),
      this.invoke("get_my_name"),
      this.invoke("get_peers"),
      this.invoke("get_settings"),
      this.invoke("get_custom_peers"),
      this.getFiles(),
      this.getTransfers(),
    ]);
    if ([id, name, peers, settings].every((item) => item.status === "rejected")) {
      throw id.reason;
    }
    const selfId = valueOf(id, "");
    const devices = valueOf(peers, []).map(normalizeDevice);
    return {
      legacy: true,
      self: { id: selfId, name: valueOf(name, "") },
      devices,
      conversations: devices.map((peer) => ({
        id: directConversationId(selfId, peer.id),
        kind: "direct",
        peer_id: peer.id,
        title: peer.remark || peer.name,
      })),
      files: valueOf(files, []),
      transfers: valueOf(transfers, []),
      capabilities: {
        fileCenter: files.status === "fulfilled",
        transferCancel: transfers.status === "fulfilled",
      },
      settings: {
        ...valueOf(settings, {}),
        name: valueOf(name, ""),
        custom_peers: valueOf(customPeers, []),
      },
    };
  }

  async getMessages(conversation, limit, offset) {
    try {
      return await this.invoke("get_conversation_messages", {
        conversationId: conversation.id,
        limit,
        offset,
      });
    } catch (error) {
      if (!unavailable(error) || conversation.kind !== "direct") throw error;
      return offset
        ? this.invoke("get_chat_history_with_offset", {
            peerId: conversation.peer_id,
            limit,
            offset,
          })
        : this.invoke("get_chat_history", { peerId: conversation.peer_id });
    }
  }

  getFiles() {
    return this.invoke("get_file_center");
  }

  getTransfers() {
    return this.invoke("get_transfers");
  }

  createGroup(title, memberIds) {
    return this.invoke("create_group", { title, memberIds });
  }

  async sendMessage(conversation, clientMessageId, content, msgType = "text") {
    try {
      return await this.invoke("send_conversation_message", {
        conversationId: conversation.id,
        clientMessageId,
        content,
        msgType,
      });
    } catch (error) {
      if (!unavailable(error) || conversation.kind !== "direct") throw error;
      const peer = conversation.peer;
      return this.invoke("send_message", {
        peerId: conversation.peer_id,
        peerAddr: peer?.addr || "",
        content,
      });
    }
  }

  async sendFiles(conversation, files = []) {
    let paths = files
      .map((file) =>
        typeof file === "string" ? file : file.path ?? file.file_path ?? "",
      )
      .filter(Boolean);
    if (!paths.length) {
      if (!this.tauri.dialog?.open) {
        throw new TransportError(
          uiCopy("系统文件选择器不可用", "The system file picker is unavailable"),
          "file_picker_unavailable",
          0,
          false,
        );
      }
      const selected = await this.tauri.dialog?.open({
        multiple: true,
        title: uiCopy("选择要发送的文件", "Choose files to send"),
      });
      if (!selected) return [];
      paths = Array.isArray(selected) ? selected : [selected];
    }
    const results = [];
    for (const filePath of paths) {
      results.push(
        await this.invoke("send_conversation_file", {
          conversationId: conversation.id,
          filePath,
        }),
      );
    }
    return results;
  }

  async pickFiles() {
    if (!this.tauri.dialog?.open) {
      throw new TransportError(
        uiCopy("系统文件选择器不可用", "The system file picker is unavailable"),
        "file_picker_unavailable",
        0,
        false,
      );
    }
    const selected = await this.tauri.dialog.open({
      multiple: true,
      title: uiCopy("选择要发送的文件", "Choose files to send"),
    });
    if (!selected) return [];
    return this.attachmentsFromPaths(Array.isArray(selected) ? selected : [selected]);
  }

  async attachmentsFromPaths(paths) {
    const attachments = [];
    for (const path of paths) {
      const attachment = normalizeDraftAttachment(path);
      if (isImageFile(attachment) && this.tauri.fs?.readFile) {
        try {
          attachment.preview_url = bytesDataUrl(
            await this.tauri.fs.readFile(path),
            imageMime(attachment),
          );
        } catch {
          // Preview is optional; sending still uses the user-selected path.
        }
      }
      attachments.push(attachment);
    }
    return attachments;
  }

  async validateDroppedPaths(paths = []) {
    const files = [];
    const errors = [];
    for (const path of paths) {
      try {
        const metadata = await this.tauri.fs?.stat?.(path);
        if (metadata?.isDirectory) {
          errors.push(
            uiCopy(
              `不能拖入文件夹：${draftName(path)}`,
              `Folders cannot be attached: ${draftName(path)}`,
            ),
          );
        } else {
          files.push(path);
        }
      } catch {
        errors.push(
          uiCopy(
            `文件不可读或已不存在：${draftName(path)}`,
            `The file is unreadable or no longer exists: ${draftName(path)}`,
          ),
        );
      }
    }
    return { files: await this.attachmentsFromPaths(files), errors };
  }

  async stageImage(file) {
    const dataUrl = await pngDataUrlFromFile(file);
    const result = await this.invoke("stage_image_attachment", {
      dataUrl,
      fileName: `${(file.name || `Xchat-${Date.now()}`).replace(/\.[^.]+$/, "")}.png`,
    });
    return normalizeDraftAttachment(result, {
      file_name: result?.file_name ?? file.name,
      file_size: result?.file_size ?? file.size,
      mime_type: result?.mime_type ?? file.type,
      preview_url:
        result?.data_url ||
        result?.preview_url ||
        dataUrl,
    });
  }

  startCapture(conversationId) {
    return this.invoke("start_capture_editor", { conversationId });
  }

  pendingCapture() {
    return this.invoke("get_pending_capture");
  }

  finishCapture(dataUrl) {
    return this.invoke("finish_capture_editor", { dataUrl });
  }

  cancelCapture() {
    return this.invoke("cancel_capture_editor");
  }

  pinCapture(dataUrl) {
    return this.invoke("pin_capture", { dataUrl });
  }

  saveCapture(dataUrl) {
    return this.invoke("save_capture_editor", { dataUrl });
  }

  readMessageMedia(messageId) {
    return this.invoke("read_workspace_media", { messageId });
  }

  discardStagedAttachment(filePath) {
    return this.invoke("discard_staged_attachment", { filePath });
  }

  markRead(conversationId, messageIds) {
    return this.invoke("mark_messages_read", { conversationId, messageIds });
  }

  search(query, limit = 100) {
    return this.invoke("search_workspace_messages", { query, limit });
  }

  updateConversation(conversationId, patch) {
    return this.invoke("update_conversation_state", {
      conversationId,
      pinned: patch.pinned,
      forcedUnread: patch.forced_unread,
      draft: patch.draft,
    });
  }

  updateDevice(deviceId, patch) {
    return this.invoke("update_device_metadata", { deviceId, remark: patch.remark });
  }

  addEndpoint(peer) {
    return this.invoke("add_custom_peer", { peer });
  }

  removeEndpoint(peer) {
    return this.invoke("remove_custom_peer", { peer });
  }

  removeDevice(peerId) {
    return this.invoke("delete_user_complete", { peerId });
  }

  clearConversation(conversationId) {
    return this.invoke("clear_conversation_history", { conversationId });
  }

  deleteMessages(msgIds) {
    return this.invoke("delete_messages", { msgIds });
  }

  acceptFile(file) {
    return this.invoke("request_file", {
      messageId: file.message_id ?? file.id,
      senderMsgId: file.sender_msg_id,
    });
  }

  retryFile(file) {
    return this.invoke("retry_conversation_file", {
      messageId: file.message_id ?? file.id,
    });
  }

  cancelTransfer(transferId) {
    return this.invoke("cancel_transfer", { transferId });
  }

  deleteLocalFile(messageId) {
    return this.invoke("delete_local_file", { messageId });
  }

  openFile(file) {
    return this.invoke("open_workspace_file", {
      messageId: file.message_id ?? file.id,
    });
  }

  revealFile(file) {
    return this.invoke("reveal_workspace_file", {
      messageId: file.message_id ?? file.id,
    });
  }

  async patchSettings(patch, current) {
    if (patch.name !== undefined) {
      await this.invoke("update_my_name", { newName: patch.name });
    }
    if (
      ["download_path", "port", "db_path", "auto_download"].some((key) => patch[key] !== undefined)
    ) {
      const next = { ...current, ...patch };
      await this.invoke("update_settings", {
        downloadPath: next.download_path,
        port: String(next.port),
        dbPath: next.db_path,
        autoDownload: next.auto_download,
      });
    }
    if (patch.theme !== undefined) {
      await this.invoke("save_current_theme", { themeName: patch.theme });
      storage.set("xchat.theme", patch.theme);
    }
    if (patch.language !== undefined) {
      await this.invoke("set_language", { lang: patch.language }).catch((error) => {
        if (!unavailable(error)) throw error;
      });
      storage.set("xchat.language", patch.language);
    }
    if (patch.notifications_enabled !== undefined) {
      await this.invoke("set_notifications_enabled", {
        enabled: patch.notifications_enabled,
      });
    }
    if (patch.avatar !== undefined) {
      await this.invoke("update_workspace_preference", {
        key: "avatar",
        value: patch.avatar,
      });
      storage.set("xchat.avatar", patch.avatar);
    }
    if (patch.capture_shortcut !== undefined) {
      await this.invoke("update_workspace_preference", {
        key: "capture_shortcut",
        value: patch.capture_shortcut,
      });
      storage.set("xchat.captureShortcut", patch.capture_shortcut);
    }
  }

  subscribe(emit) {
    let disposed = false;
    const unlisteners = [];
    for (const name of EVENT_NAMES) {
      this.tauri.event
        ?.listen(name, (event) => emit({ type: name, payload: event.payload }))
        .then((unlisten) => (disposed ? unlisten() : unlisteners.push(unlisten)))
        .catch(() => {});
    }
    return () => {
      disposed = true;
      unlisteners.splice(0).forEach((unlisten) => unlisten());
    };
  }
}

class HttpWsAdapter {
  constructor() {
    this.runtime = "web";
    this.pendingUploads = new Map();
    this.aborters = new Map();
  }

  request(path, options = {}) {
    return fetch(path, options).then(parseResponse);
  }

  json(path, method, body) {
    return this.request(path, {
      method,
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
  }

  async getSnapshot() {
    try {
      return await this.request("/api/workspace");
    } catch (error) {
      if (!unavailable(error)) throw error;
      return this.getLegacySnapshot();
    }
  }

  async getLegacySnapshot() {
    const [id, name, peers, settings, customPeers, files, transfers] = await Promise.allSettled([
      this.request("/api/get_my_id"),
      this.request("/api/get_my_name"),
      this.request("/api/get_peers"),
      this.request("/api/get_settings"),
      this.request("/api/get_custom_peers"),
      this.getFiles(),
      this.getTransfers(),
    ]);
    if ([id, name, peers, settings].every((item) => item.status === "rejected")) {
      throw id.reason;
    }
    const selfId = valueOf(id, {})?.id ?? "";
    const devices = (valueOf(peers, []) ?? []).map(normalizeDevice);
    return {
      legacy: true,
      self: { id: selfId, name: valueOf(name, {})?.name ?? "" },
      devices,
      conversations: devices.map((peer) => ({
        id: directConversationId(selfId, peer.id),
        kind: "direct",
        peer_id: peer.id,
        title: peer.remark || peer.name,
      })),
      files: valueOf(files, {})?.files ?? valueOf(files, []),
      transfers: valueOf(transfers, {})?.transfers ?? valueOf(transfers, []),
      capabilities: {
        fileCenter: files.status === "fulfilled",
        transferCancel: transfers.status === "fulfilled",
      },
      settings: {
        ...valueOf(settings, {}),
        custom_peers: valueOf(customPeers, {})?.peers ?? [],
      },
    };
  }

  async getMessages(conversation, limit, offset) {
    try {
      const result = await this.request(
        `/api/conversations/${encodeURIComponent(conversation.id)}/messages?limit=${limit}&offset=${offset}`,
      );
      return result?.messages ?? result ?? [];
    } catch (error) {
      if (!unavailable(error) || conversation.kind !== "direct") throw error;
      const result = await this.request(
        `/api/chat_history/${encodeURIComponent(conversation.peer_id)}?limit=${limit}&offset=${offset}`,
      );
      return result?.messages ?? [];
    }
  }

  getFiles() {
    return this.request("/api/files");
  }

  getTransfers() {
    return this.request("/api/transfers");
  }

  createGroup(title, memberIds) {
    return this.json("/api/groups", "POST", { title, member_ids: memberIds });
  }

  async sendMessage(conversation, clientMessageId, content, msgType = "text") {
    try {
      return await this.json(
        `/api/conversations/${encodeURIComponent(conversation.id)}/messages`,
        "POST",
        { client_message_id: clientMessageId, content, msg_type: msgType },
      );
    } catch (error) {
      if (!unavailable(error) || conversation.kind !== "direct") throw error;
      return this.json("/api/send_message", "POST", {
        peer_id: conversation.peer_id,
        peer_addr: conversation.peer?.addr || "",
        content,
      });
    }
  }

  async sendFiles(conversation, files = []) {
    const results = [];
    for (const attachment of files) {
      const file = attachment?.file ?? attachment;
      if (!(file instanceof Blob)) {
        throw new TransportError(
          uiCopy("浏览器无法读取这个本地文件", "The browser cannot read this local file"),
          "file_unavailable",
          0,
          false,
        );
      }
      const form = new FormData();
      form.append("file", file, attachment?.file_name ?? file.name);
      results.push(
        await this.request(
          `/api/conversations/${encodeURIComponent(conversation.id)}/files`,
          { method: "POST", body: form },
        ),
      );
    }
    return results;
  }

  pickFiles() {
    return [];
  }

  async stageImage(file) {
    return normalizeDraftAttachment(file, {
      preview_url: URL.createObjectURL(file),
    });
  }

  markRead(conversationId, messageIds) {
    return this.json("/api/receipts/read", "POST", {
      conversation_id: conversationId,
      message_ids: messageIds,
    });
  }

  search(query, limit = 100) {
    return this.request(
      `/api/messages/search?q=${encodeURIComponent(query)}&limit=${limit}`,
    );
  }

  updateConversation(conversationId, patch) {
    return this.json(
      `/api/conversations/${encodeURIComponent(conversationId)}/state`,
      "POST",
      patch,
    );
  }

  updateDevice(deviceId, patch) {
    return this.json(`/api/devices/${encodeURIComponent(deviceId)}`, "POST", patch);
  }

  addEndpoint(peer) {
    return this.json("/api/add_custom_peer", "POST", { peer });
  }

  removeEndpoint(peer) {
    return this.json("/api/remove_custom_peer", "POST", { peer });
  }

  removeDevice(peerId) {
    return this.json("/api/delete_user", "POST", { peer_id: peerId });
  }

  clearConversation(conversationId) {
    return this.request(
      `/api/conversations/${encodeURIComponent(conversationId)}/clear`,
      { method: "POST" },
    );
  }

  deleteMessages(msgIds) {
    return this.json("/api/delete_messages", "POST", { msg_ids: msgIds });
  }

  acceptFile(file) {
    return this.json("/api/request_file", "POST", {
      message_id: file.message_id ?? file.id,
      sender_msg_id: file.sender_msg_id,
    });
  }

  retryFile(file) {
    return this.request(
      `/api/files/${encodeURIComponent(file.message_id ?? file.id)}/retry`,
      { method: "POST" },
    );
  }

  cancelTransfer(transferId) {
    return this.request(`/api/transfers/${encodeURIComponent(transferId)}/cancel`, {
      method: "POST",
    });
  }

  deleteLocalFile(messageId) {
    return this.request(`/api/files/${encodeURIComponent(messageId)}/delete`, {
      method: "POST",
    });
  }

  openFile(file) {
    globalThis.open(`/api/download/${encodeURIComponent(file.message_id ?? file.id)}`, "_blank");
  }

  revealFile() {
    throw new TransportError(
      uiCopy("浏览器不能打开系统文件夹", "Browsers cannot open system folders"),
      "unsupported",
      0,
      false,
    );
  }

  async capture() {
    if (!globalThis.navigator?.mediaDevices?.getDisplayMedia) {
      throw new TransportError(
        uiCopy("当前浏览器不支持截屏", "This browser does not support screen capture"),
        "capture_unsupported",
        0,
        false,
      );
    }
    let stream;
    try {
      stream = await navigator.mediaDevices.getDisplayMedia({ video: true, audio: false });
      const video = document.createElement("video");
      video.srcObject = stream;
      await video.play();
      const canvas = document.createElement("canvas");
      canvas.width = video.videoWidth;
      canvas.height = video.videoHeight;
      canvas.getContext("2d").drawImage(video, 0, 0);
      const blob = await new Promise((resolve, reject) =>
        canvas.toBlob(
          (value) =>
            value
              ? resolve(value)
              : reject(new Error(uiCopy("PNG 编码失败", "PNG encoding failed"))),
          "image/png",
        ),
      );
      return new File([blob], `Xchat-${Date.now()}.png`, { type: "image/png" });
    } catch (error) {
      if (error?.name === "NotAllowedError") {
        throw new TransportError(uiCopy("已取消截屏", "Capture cancelled"), "cancelled", 0, false);
      }
      throw error;
    } finally {
      stream?.getTracks().forEach((track) => track.stop());
    }
  }

  async startCapture(conversationId) {
    const file = await this.capture();
    const dataUrl = await dataUrlFromFile(file);
    storage.set(
      "xchat.capture.pending",
      JSON.stringify({
        conversation_id: conversationId,
        data_url: dataUrl,
        file_name: file.name,
        mime_type: file.type,
      }),
    );
    globalThis.open(
      `${location.pathname}?view=capture-editor`,
      "xchat-capture-editor",
      "popup,width=1100,height=760",
    );
    return { pending: true };
  }

  pendingCapture() {
    try {
      return JSON.parse(storage.get("xchat.capture.pending") || "null");
    } catch {
      return null;
    }
  }

  async finishCapture(dataUrl) {
    const pending = await this.pendingCapture();
    const blob = await fetch(dataUrl).then((response) => response.blob());
    const file = new File(
      [blob],
      pending?.file_name || `Xchat-${Date.now()}.png`,
      { type: "image/png" },
    );
    storage.set("xchat.capture.pending", "");
    return normalizeDraftAttachment(file, {
      conversation_id: pending?.conversation_id,
      preview_url: dataUrl,
    });
  }

  cancelCapture() {
    storage.set("xchat.capture.pending", "");
  }

  pinCapture(dataUrl) {
    globalThis.open(dataUrl, "_blank", "popup");
  }

  saveCapture(dataUrl) {
    const link = document.createElement("a");
    link.href = dataUrl;
    link.download = `Xchat-${Date.now()}.png`;
    link.click();
  }

  async readMessageMedia(messageId) {
    const response = await fetch(`/api/download/${encodeURIComponent(messageId)}`);
    if (!response.ok) throw new TransportError(uiCopy("图片不可用", "Image unavailable"));
    return {
      blob: await response.blob(),
      mime_type: response.headers.get("content-type") || "",
    };
  }

  discardStagedAttachment() {}

  async patchSettings(patch, current) {
    if (patch.name !== undefined) {
      await this.json("/api/update_my_name", "POST", { name: patch.name });
    }
    if (
      ["download_path", "port", "db_path", "auto_download"].some((key) => patch[key] !== undefined)
    ) {
      const next = { ...current, ...patch };
      await this.json("/api/update_settings", "POST", {
        download_path: next.download_path,
        port: Number(next.port),
        db_path: next.db_path,
        auto_download: next.auto_download,
      });
    }
    if (patch.theme !== undefined) {
      await this.json("/api/save_current_theme", "POST", { theme_name: patch.theme });
      storage.set("xchat.theme", patch.theme);
    }
    if (patch.language !== undefined) {
      await this.json("/api/set_language", "POST", { language: patch.language }).catch((error) => {
        if (!unavailable(error)) throw error;
      });
      storage.set("xchat.language", patch.language);
    }
    if (patch.notifications_enabled !== undefined) {
      if (patch.notifications_enabled) {
        if (!("Notification" in globalThis)) {
          throw new TransportError(
            uiCopy("浏览器不支持通知", "This browser does not support notifications"),
            "unsupported",
            0,
            false,
          );
        }
        const permission = await Notification.requestPermission();
        if (permission !== "granted") {
          throw new TransportError(
            uiCopy("通知权限未授予", "Notification permission was not granted"),
            "permission_denied",
            0,
            false,
          );
        }
      }
      await this.json("/api/set_notifications_enabled", "POST", {
        enabled: patch.notifications_enabled,
      }).catch((error) => {
        if (!unavailable(error)) throw error;
      });
    }
    if (patch.avatar !== undefined) {
      await this.json("/api/settings/preference", "POST", {
        key: "avatar",
        value: patch.avatar,
      });
      storage.set("xchat.avatar", patch.avatar);
    }
    if (patch.capture_shortcut !== undefined) {
      await this.json("/api/settings/preference", "POST", {
        key: "capture_shortcut",
        value: patch.capture_shortcut,
      });
      storage.set("xchat.captureShortcut", patch.capture_shortcut);
    }
  }

  subscribe(emit) {
    let stopped = false;
    let socket;
    let retryTimer;
    let delay = 1000;
    const connect = () => {
      if (stopped) return;
      const protocol = location.protocol === "https:" ? "wss:" : "ws:";
      socket = new WebSocket(`${protocol}//${location.host}/ws`);
      socket.onopen = () => {
        delay = 1000;
        emit({ type: "transport.connected" });
      };
      socket.onmessage = ({ data }) => {
        try {
          const payload = JSON.parse(data);
          emit({ type: payload.type || payload.msg_type || "message.changed", payload });
        } catch {
          // Ignore malformed frames; the next snapshot refresh remains authoritative.
        }
      };
      socket.onclose = () => {
        if (stopped) return;
        emit({ type: "transport.disconnected" });
        retryTimer = setTimeout(connect, delay);
        delay = Math.min(delay * 2, 5000);
      };
      socket.onerror = () => socket.close();
    };
    connect();
    return () => {
      stopped = true;
      clearTimeout(retryTimer);
      socket?.close();
    };
  }
}

function makeInitialSnapshot(runtime) {
  return {
    phase: "booting",
    self: { id: "", name: "", hostname: "", mac_address: "", addr: "", avatar: "" },
    activeSection: "chat",
    activeConversationId: null,
    focusedMessageId: null,
    conversations: [],
    messagesByConversation: {},
    devices: [],
    files: [],
    transfers: [],
    transfer_sample_at: 0,
    draftAttachments: {},
    searchResults: [],
    settings: normalizeSettings(),
    capabilities: runtimeCapabilities(runtime, {}, true),
    notices: [],
  };
}

function outcomeError(error) {
  return {
    code: error?.code || "operation_failed",
    message: errorText(error),
    retryable: error?.retryable !== false,
  };
}

export function createXChatModule() {
  const tauri = globalThis.window?.__TAURI__;
  const adapter = tauri ? new TauriAdapter(tauri) : new HttpWsAdapter();
  let snapshot = makeInitialSnapshot(adapter.runtime);
  let started = false;
  let stopEvents = () => {};
  let pollTimer;
  let refreshTimer;
  const listeners = new Set();

  const publish = (next) => {
    if (next === snapshot) return;
    snapshot = next;
    listeners.forEach((listener) => listener());
  };

  const patch = (change) => publish({ ...snapshot, ...change });

  const addNotice = (message, kind = "error") => {
    const notice = {
      id: globalThis.crypto?.randomUUID?.() ?? `${Date.now()}:${Math.random()}`,
      message,
      kind,
    };
    patch({ notices: [...snapshot.notices.slice(-3), notice] });
    return notice.id;
  };

  const updateDraft = (conversationId, change) => {
    if (!conversationId) return [];
    const current = snapshot.draftAttachments[conversationId] ?? [];
    const next = typeof change === "function" ? change(current) : change;
    patch({
      draftAttachments: {
        ...snapshot.draftAttachments,
        [conversationId]: next,
      },
    });
    return next;
  };

  const addDraftAttachments = (conversationId, attachments) =>
    updateDraft(conversationId, (current) => {
      const next = [...current];
      for (const item of attachments.map((value) => normalizeDraftAttachment(value))) {
        const duplicate = next.findIndex(
          (existing) =>
            (item.file_path && existing.file_path === item.file_path) ||
            (item.id && existing.id === item.id),
        );
        if (duplicate >= 0) next[duplicate] = { ...next[duplicate], ...item };
        else next.push(item);
      }
      return next;
    });

  const attachPeers = (conversations, devices) =>
    conversations.map((conversation) => ({
      ...conversation,
      peer: devices.find((device) => device.id === conversation.peer_id),
    }));

  const refreshWorkspace = async ({ quiet = false } = {}) => {
    try {
      const raw = await adapter.getSnapshot();
      const next = normalizeWorkspace(raw, snapshot, adapter.runtime);
      next.conversations = attachPeers(next.conversations, next.devices);
      publish(next);
      return next;
    } catch (error) {
      patch({ phase: snapshot.phase === "booting" ? "error" : "offline" });
      if (quiet) throw error;
      throw new TransportError(
        uiCopy(
          `无法读取工作区：${errorText(error)}`,
          `Unable to load the workspace: ${errorText(error)}`,
        ),
        error?.code,
        error?.status,
        error?.retryable,
      );
    }
  };

  const schedulePoll = () => {
    clearTimeout(pollTimer);
    if (!started) return;
    const delay = snapshot.transfers.some((transfer) =>
      ACTIVE_TRANSFER_STATES.has(transfer.status),
    )
      ? 1000
      : 5000;
    pollTimer = setTimeout(async () => {
      if (!started) return;
      if (document.visibilityState === "visible") {
        await refreshWorkspace({ quiet: true }).catch(() => {});
      }
      schedulePoll();
    }, delay);
  };

  const scheduleRefresh = () => {
    clearTimeout(refreshTimer);
    refreshTimer = setTimeout(async () => {
      await refreshWorkspace({ quiet: true }).catch(() => {});
      schedulePoll();
      const active = snapshot.conversations.find(
        (conversation) => conversation.id === snapshot.activeConversationId,
      );
      if (active) loadMessages(active, 40, 0, true).catch(() => {});
    }, 100);
  };

  const handleEvent = (event) => {
    if (event.type === "transport.connected") {
      if (snapshot.phase === "offline") patch({ phase: "ready" });
      scheduleRefresh();
      return;
    }
    if (event.type === "transport.disconnected") {
      if (snapshot.phase !== "booting" && snapshot.phase !== "error") patch({ phase: "offline" });
      return;
    }
    const payload = event.payload?.payload ?? event.payload;
    const eventType = String(event.type || "").replaceAll("_", ".");
    if (
      eventType.includes("capture.finished") ||
      eventType.includes("capture.ready") ||
      eventType.includes("capture-ready")
    ) {
      const conversationId = payload?.conversation_id ?? snapshot.activeConversationId;
      if (payload?.file_path || payload?.path || payload?.file) {
        addDraftAttachments(conversationId, [payload]);
      }
      return;
    }
    if (
      (eventType.includes("message") || payload?.from_id || payload?.conversation_id) &&
      payload?.content !== undefined
    ) {
      const conversationId =
        payload.conversation_id ??
        snapshot.conversations.find((conversation) => conversation.peer_id === payload.from_id)?.id;
      if (conversationId) {
        const message = normalizeMessage(payload, snapshot.self.id, conversationId);
        patch({
          messagesByConversation: {
            ...snapshot.messagesByConversation,
            [conversationId]: mergeMessages(
              snapshot.messagesByConversation[conversationId],
              [message],
            ),
          },
        });
      }
    }
    scheduleRefresh();
  };

  const loadMessages = async (conversation, limit = 40, offset = 0, quiet = false) => {
    try {
      const response = await adapter.getMessages(conversation, limit, offset);
      const rows = response?.messages ?? response ?? [];
      const messages = rows.map((item) =>
        normalizeMessage(item, snapshot.self.id, conversation.id),
      );
      const existing = offset ? snapshot.messagesByConversation[conversation.id] : [];
      patch({
        messagesByConversation: {
          ...snapshot.messagesByConversation,
          [conversation.id]: mergeMessages(existing, messages),
        },
      });
      return messages;
    } catch (error) {
      if (quiet) throw error;
      throw new TransportError(
        uiCopy(
          `无法加载消息：${errorText(error)}`,
          `Unable to load messages: ${errorText(error)}`,
        ),
        error?.code,
        error?.status,
        error?.retryable,
      );
    }
  };

  const markVisibleRead = async (conversationId) => {
    if (!snapshot.capabilities.readReceipts || document.visibilityState !== "visible") return;
    const messageIds = (snapshot.messagesByConversation[conversationId] ?? [])
      .filter(
        (message) =>
          !message.own && message.client_message_id && message.status !== "read",
      )
      .map((message) => message.client_message_id);
    if (messageIds.length) await adapter.markRead(conversationId, messageIds.slice(-100));
  };

  const activeConversation = () =>
    snapshot.conversations.find(
      (conversation) => conversation.id === snapshot.activeConversationId,
    );

  const run = async (action) => {
    switch (action.type) {
      case "bootstrap": {
        if (started) return;
        started = true;
        stopEvents = adapter.subscribe(handleEvent);
        await refreshWorkspace();
        schedulePoll();
        if (snapshot.activeConversationId) {
          const conversation = activeConversation();
          if (conversation) await loadMessages(conversation);
        }
        return;
      }
      case "navigation.open":
        patch({ activeSection: action.section });
        return;
      case "conversation.open": {
        const conversation = snapshot.conversations.find((item) => item.id === action.id);
        if (!conversation) return;
        const target = action.targetMessageId ?? action.targetClientMessageId ?? null;
        const hasTarget = () =>
          !target ||
          (snapshot.messagesByConversation[action.id] ?? []).some(
            (message) =>
              String(message.message_id ?? message.id) === String(target) ||
              message.client_message_id === target,
          );
        patch({
          activeConversationId: action.id,
          activeSection: "chat",
          focusedMessageId: target,
        });
        let pageSize = 40;
        let page = await loadMessages(conversation, pageSize);
        let offset = page.length;
        // ponytail: page backward on demand; add an around-message endpoint if huge histories become slow.
        while (!hasTarget() && page.length === pageSize) {
          pageSize = 200;
          page = await loadMessages(conversation, pageSize, offset);
          offset += page.length;
        }
        await markVisibleRead(action.id);
        return;
      }
      case "conversation.createGroup": {
        if (!snapshot.capabilities.groupChat) {
          throw new TransportError(
            uiCopy("当前后端不支持群聊", "The current backend does not support group chat"),
            "unsupported",
            0,
            false,
          );
        }
        const created = await adapter.createGroup(action.title, action.memberIds);
        await refreshWorkspace();
        if (created?.id) patch({ activeConversationId: created.id, activeSection: "chat" });
        return created;
      }
      case "conversation.loadOlder": {
        const conversation = activeConversation();
        if (!conversation) return;
        const offset = snapshot.messagesByConversation[conversation.id]?.length ?? 0;
        return loadMessages(conversation, 40, offset);
      }
      case "conversation.pin":
      case "conversation.markUnread":
      case "conversation.saveDraft": {
        const conversation = snapshot.conversations.find((item) => item.id === action.id);
        if (!conversation) return;
        const change =
          action.type === "conversation.pin"
            ? { pinned: action.value }
            : action.type === "conversation.markUnread"
              ? { forced_unread: action.value }
              : { draft: action.draft };
        await adapter.updateConversation(conversation.id, change);
        patch({
          conversations: snapshot.conversations.map((item) =>
            item.id === conversation.id ? { ...item, ...change } : item,
          ),
        });
        return;
      }
      case "message.sendText": {
        const conversation = activeConversation();
        const content = action.content.trim();
        if (!conversation || !content) return;
        const clientMessageId = globalThis.crypto.randomUUID();
        const optimistic = normalizeMessage(
          {
            client_message_id: clientMessageId,
            conversation_id: conversation.id,
            sender_id: snapshot.self.id,
            sender_name: snapshot.self.name,
            content,
            timestamp: Math.floor(Date.now() / 1000),
            status: "pending",
            own: true,
          },
          snapshot.self.id,
          conversation.id,
        );
        patch({
          messagesByConversation: {
            ...snapshot.messagesByConversation,
            [conversation.id]: mergeMessages(
              snapshot.messagesByConversation[conversation.id],
              [optimistic],
            ),
          },
        });
        try {
          const result = await adapter.sendMessage(
            conversation,
            clientMessageId,
            content,
            "text",
          );
          const acknowledged = normalizeMessage(
            {
              ...optimistic,
              ...(result?.message ?? result),
              client_message_id: clientMessageId,
              status: result?.status ?? result?.message?.status ?? "sent",
            },
            snapshot.self.id,
            conversation.id,
          );
          patch({
            messagesByConversation: {
              ...snapshot.messagesByConversation,
              [conversation.id]: mergeMessages(
                snapshot.messagesByConversation[conversation.id],
                [acknowledged],
              ),
            },
          });
          return result;
        } catch (error) {
          patch({
            messagesByConversation: {
              ...snapshot.messagesByConversation,
              [conversation.id]: mergeMessages(
                snapshot.messagesByConversation[conversation.id],
                [{ ...optimistic, status: "failed", error: errorText(error) }],
              ),
            },
          });
          throw error;
        }
      }
      case "message.sendFiles": {
        const conversation = activeConversation();
        if (!conversation) return;
        scheduleRefresh();
        const result = await adapter.sendFiles(conversation, action.files ?? []);
        scheduleRefresh();
        return result;
      }
      case "draft.pickFiles": {
        const conversation = activeConversation();
        if (!conversation) return [];
        const files = await adapter.pickFiles();
        addDraftAttachments(conversation.id, files);
        return files;
      }
      case "draft.addFiles": {
        const conversationId = action.conversationId ?? snapshot.activeConversationId;
        if (!conversationId) return [];
        if (action.rejectedNames?.length) {
          addNotice(
            uiCopy(
              `不能拖入文件夹：${action.rejectedNames.join("、")}`,
              `Folders cannot be attached: ${action.rejectedNames.join(", ")}`,
            ),
          );
        }
        const attachments = [];
        for (const file of action.files ?? []) {
          if (typeof file === "string" || file?.path || file?.file_path) {
            attachments.push(normalizeDraftAttachment(file));
          } else if (file instanceof Blob) {
            if (adapter.runtime === "tauri" && !isImageFile(file)) {
              throw new TransportError(
                uiCopy(
                  "拖入的普通文件无法读取路径，请使用附件按钮选择",
                  "Use the attachment button to choose this file",
                ),
                "file_path_unavailable",
                0,
                false,
              );
            }
            const staged = await adapter.stageImage(file);
            attachments.push({
              ...staged,
              file: staged.file ?? file,
              managed: adapter.runtime === "tauri",
            });
          }
        }
        addDraftAttachments(conversationId, attachments);
        return attachments;
      }
      case "draft.addPaths": {
        const conversationId = action.conversationId ?? snapshot.activeConversationId;
        if (!conversationId) return [];
        const checked = adapter.validateDroppedPaths
          ? await adapter.validateDroppedPaths(action.paths ?? [])
          : { files: action.paths ?? [], errors: [] };
        if (checked.errors.length) addNotice(checked.errors.join(uiCopy("；", "; ")));
        return addDraftAttachments(conversationId, checked.files);
      }
      case "draft.addManaged": {
        const conversationId =
          action.attachment?.conversation_id ??
          action.conversationId ??
          snapshot.activeConversationId;
        const attachment = {
          ...action.attachment,
          managed: Boolean(action.attachment?.file_path || action.attachment?.path),
        };
        addDraftAttachments(conversationId, [attachment]);
        return attachment;
      }
      case "draft.remove": {
        const conversationId = action.conversationId ?? snapshot.activeConversationId;
        const current = snapshot.draftAttachments[conversationId] ?? [];
        const attachment = current.find((item) => item.id === action.id);
        if (attachment?.managed && (attachment.file_path || attachment.path)) {
          await adapter
            .discardStagedAttachment?.(attachment.file_path || attachment.path)
            .catch(() => {});
        }
        updateDraft(
          conversationId,
          current.filter((item) => item.id !== action.id),
        );
        return;
      }
      case "draft.sent":
        updateDraft(action.conversationId ?? snapshot.activeConversationId, (current) =>
          current.filter((item) => item.id !== action.id),
        );
        return;
      case "capture.start": {
        const conversation = activeConversation();
        if (!conversation || !snapshot.capabilities.capture) {
          throw new TransportError(
            uiCopy("当前平台不支持截屏", "The current platform does not support screen capture"),
            "capture_unsupported",
            0,
            false,
          );
        }
        return adapter.startCapture(conversation.id);
      }
      case "capture.pending":
        return adapter.pendingCapture();
      case "capture.finish":
        return adapter.finishCapture(action.dataUrl);
      case "capture.cancel":
        return adapter.cancelCapture();
      case "capture.pin":
        return adapter.pinCapture(action.dataUrl);
      case "capture.save":
        return adapter.saveCapture(action.dataUrl);
      case "media.readMessage":
        return adapter.readMessageMedia(action.messageId);
      case "message.markRead":
        return markVisibleRead(action.conversationId);
      case "message.search": {
        const result = action.query.trim() ? await adapter.search(action.query.trim()) : [];
        patch({ searchResults: result?.results ?? result?.messages ?? result ?? [] });
        return result;
      }
      case "message.deleteLocal":
        await adapter.deleteMessages(action.ids);
        return loadMessages(activeConversation(), 40, 0);
      case "message.clearConversation": {
        const conversation = activeConversation();
        if (!conversation) return;
        await adapter.clearConversation(conversation.id);
        patch({
          messagesByConversation: {
            ...snapshot.messagesByConversation,
            [conversation.id]: [],
          },
        });
        return;
      }
      case "device.saveRemark":
        await adapter.updateDevice(action.id, { remark: action.remark });
        return refreshWorkspace();
      case "device.saveEndpoint":
        await adapter.addEndpoint(action.endpoint);
        return refreshWorkspace();
      case "device.removeEndpoint":
        await adapter.removeEndpoint(action.endpoint);
        return refreshWorkspace();
      case "device.remove":
        await adapter.removeDevice(action.id);
        return refreshWorkspace();
      case "file.accept": {
        const result = await adapter.acceptFile(action.file);
        scheduleRefresh();
        return result;
      }
      case "file.retry": {
        const result = await adapter.retryFile(action.file);
        scheduleRefresh();
        return result;
      }
      case "file.open":
        return adapter.openFile(action.file);
      case "file.reveal":
        return adapter.revealFile(action.file);
      case "file.deleteLocalCopy":
        await adapter.deleteLocalFile(action.file.message_id ?? action.file.id);
        return refreshWorkspace();
      case "transfer.cancel": {
        const previous = snapshot.transfers;
        patch({
          transfers: previous.map((transfer) =>
            transfer.id === action.id ? { ...transfer, status: "cancelling" } : transfer,
          ),
        });
        try {
          await adapter.cancelTransfer(action.id);
          scheduleRefresh();
        } catch (error) {
          patch({ transfers: previous });
          throw error;
        }
        return;
      }
      case "settings.patch":
        await adapter.patchSettings(action.patch, snapshot.settings);
        if (action.patch.theme !== undefined) storage.set("xchat.theme", action.patch.theme);
        if (action.patch.language !== undefined) {
          storage.set("xchat.language", action.patch.language);
        }
        patch({ settings: { ...snapshot.settings, ...action.patch } });
        return;
      case "notice.dismiss":
        patch({ notices: snapshot.notices.filter((notice) => notice.id !== action.id) });
        return;
      case "refresh": {
        const result = await refreshWorkspace();
        schedulePoll();
        return result;
      }
      case "shutdown":
        stopEvents();
        started = false;
        clearTimeout(pollTimer);
        clearTimeout(refreshTimer);
        return;
      default:
        throw new Error(`Unknown Xchat action: ${action.type}`);
    }
  };

  return {
    getSnapshot: () => snapshot,
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    async dispatch(action) {
      try {
        const data = await run(action);
        return { ok: true, data };
      } catch (error) {
        const detail = outcomeError(error);
        if (detail.code !== "cancelled") addNotice(detail.message);
        return { ok: false, error: detail };
      }
    },
  };
}
