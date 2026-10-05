// Playback state lives for one workspace session and survives message row remounts.
export function createMediaPlaybackController(maxPositions = 200) {
  const players = new Map();
  const positions = new Map();
  const remember = (player, key) => {
    // A new element starts at zero before metadata. StrictMode's rehearsal
    // cleanup and early pause events must not overwrite its saved position.
    if (players.get(player)?.restored === false) return;
    if (!Number.isFinite(player.currentTime) || player.currentTime < 0) return;
    positions.delete(key);
    positions.set(key, player.ended ? 0 : player.currentTime);
    if (positions.size > maxPositions) positions.delete(positions.keys().next().value);
  };
  const pause = (player, key) => {
    remember(player, key);
    player.pause();
  };
  return {
    register(player, key) {
      players.set(player, { key, restored: !positions.has(key) });
      return () => {
        pause(player, key);
        players.delete(player);
      };
    },
    play(player) {
      for (const [other, record] of players) {
        if (other !== player) pause(other, record.key);
      }
    },
    remember(player) {
      const record = players.get(player);
      if (record) remember(player, record.key);
    },
    restore(player) {
      const record = players.get(player);
      if (!record) return;
      record.restored = true;
      const position = positions.get(record.key);
      if (position > 0) {
        const duration = player.duration;
        player.currentTime = Number.isFinite(duration) && position >= duration ? 0 : position;
      }
    },
    pauseAll() {
      for (const [player, record] of players) pause(player, record.key);
    },
  };
}

export function mediaPositionKey(conversationId, message) {
  return `${conversationId || message.conversation_id || ""}:${message.client_message_id || message.message_id || message.id}`;
}

export function mediaTransferDirection(transfer = {}, message = {}) {
  const normalize = (value) => {
    const direction = String(value || "").toLowerCase();
    if (["send", "outgoing", "upload"].includes(direction)) return "outgoing";
    if (["receive", "incoming", "download"].includes(direction)) return "incoming";
    return null;
  };
  return normalize(transfer.direction) || normalize(message.direction) || (message.own ? "outgoing" : "incoming");
}

export function formatMediaDuration(seconds) {
  if (!Number.isFinite(seconds) || seconds < 0) return "—";
  const value = Math.floor(seconds);
  const minutes = Math.floor(value / 60);
  return `${String(minutes).padStart(2, "0")}:${String(value % 60).padStart(2, "0")}`;
}

export function imageAnimationHint(file = {}) {
  const name = String(file.file_name || file.name || file.content || "").toLowerCase();
  const mime = String(file.mime_type || file.type || "").toLowerCase().split(";")[0];
  if (/\.(gif|apng)$/.test(name) || ["image/gif", "image/apng"].includes(mime)) return true;
  if (/\.(webp|png)$/.test(name) || ["image/webp", "image/png"].includes(mime)) return null;
  return false;
}

// APNG's acTL precedes image data; WebP announces animation in its VP8X flags.
export function imagePrefixIsAnimated(bytes) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const tag = (offset) => String.fromCharCode(...bytes.subarray(offset, offset + 4));
  if (bytes.length >= 21 && tag(0) === "RIFF" && tag(8) === "WEBP") {
    return tag(12) === "VP8X" && Boolean(bytes[20] & 0x02);
  }
  if (bytes.length < 8 || tag(0) !== "\x89PNG") return false;
  for (let offset = 8; offset + 8 <= bytes.length;) {
    const length = view.getUint32(offset);
    const type = tag(offset + 4);
    if (type === "acTL") return true;
    if (type === "IDAT" || type === "IEND") return false;
    offset += 12 + length;
  }
  return false;
}

export async function detectImageAnimation(source, signal) {
  const hint = imageAnimationHint(source);
  if (hint !== null) return hint;
  const limit = 65536;
  const response = await fetch(source.url, { headers: { Range: `bytes=0-${limit - 1}` }, signal });
  if (!response.ok || !response.body) return false;
  const reader = response.body.getReader();
  const chunks = [];
  let size = 0;
  try {
    while (size < limit) {
      const { value, done } = await reader.read();
      if (done) break;
      const chunk = value.subarray(0, limit - size);
      chunks.push(chunk);
      size += chunk.length;
    }
  } finally {
    await reader.cancel().catch(() => {});
  }
  const bytes = new Uint8Array(size);
  let offset = 0;
  for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.length; }
  return imagePrefixIsAnimated(bytes);
}
