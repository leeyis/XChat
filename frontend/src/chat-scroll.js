// Keep a reader's message anchor while transfer updates change content geometry.
export function createChatScrollController() {
  let conversation;
  let following = true;
  let anchors = [];
  let savedTop = 0;
  let firstKey;
  let paused = false;

  const rows = (viewport) => [...viewport.querySelectorAll("[data-message-key]")];
  const key = (row) => row.dataset.messageKey;
  const maximum = (viewport) => Math.max(0, viewport.scrollHeight - viewport.clientHeight);

  function setTop(viewport, top) {
    const next = Math.max(0, Math.min(top, maximum(viewport)));
    // DOM heights are rounded while high-DPI scroll offsets can be fractional.
    if (Math.abs(viewport.scrollTop - next) <= 1) return;
    viewport.scrollTo({ top: next, behavior: "auto" });
  }

  function capture(viewport) {
    savedTop = viewport.scrollTop;
    following = maximum(viewport) - savedTop <= 32;
    const top = viewport.getBoundingClientRect().top;
    anchors = [];
    for (const row of rows(viewport)) {
      const bounds = row.getBoundingClientRect();
      if (bounds.bottom <= top || bounds.top >= top + viewport.clientHeight) continue;
      anchors.push({ key: key(row), offset: bounds.top - top });
      // Neighboring visible rows provide a fallback if the first row is deleted.
      if (anchors.length === 3) break;
    }
  }

  function reconcile(viewport, id, { hold = false } = {}) {
    const changedConversation = conversation !== id;
    const current = rows(viewport);
    const currentFirstKey = current[0]?.dataset.messageKey;
    const prepended = !changedConversation && firstKey != null &&
      currentFirstKey !== firstKey &&
      current.some((row) => key(row) === firstKey);
    if (changedConversation) {
      conversation = id;
      following = !hold;
      anchors = [];
      savedTop = 0;
    }
    firstKey = currentFirstKey;
    paused = hold;
    if (hold) return;

    if (following && !prepended) {
      setTop(viewport, maximum(viewport));
    } else {
      const top = viewport.getBoundingClientRect().top;
      const anchor = anchors.find((item) => current.some((row) => key(row) === item.key));
      const row = anchor && current.find((item) => key(item) === anchor.key);
      setTop(viewport, row
        ? viewport.scrollTop + row.getBoundingClientRect().top - top - anchor.offset
        : savedTop);
    }
    capture(viewport);
  }

  return {
    capture,
    reconcile,
    resize(viewport) {
      if (!paused) reconcile(viewport, conversation);
    },
  };
}
