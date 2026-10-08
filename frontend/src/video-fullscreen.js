// Fullscreen promotes the existing stage; it never reloads, seeks or recreates media.
export function createVideoFullscreenController(stage, { trigger, onError = () => {} } = {}) {
  const doc = stage.ownerDocument;
  const frame = doc.defaultView.requestAnimationFrame.bind(doc.defaultView);
  const scroller = stage.closest(".message-scroll");
  let savedScroll = null;
  let active = false;
  let pending = false;
  let disposed = false;
  let restoration = 0;
  const ownsFullscreen = () => Boolean(doc.fullscreenElement && (doc.fullscreenElement === stage || stage.contains(doc.fullscreenElement)));
  const restoreScroll = () => {
    if (scroller?.isConnected && savedScroll) {
      scroller.scrollTop = savedScroll.top;
      scroller.scrollLeft = savedScroll.left;
    }
  };
  const restore = () => {
    const ticket = ++restoration;
    restoreScroll();
    if (!disposed && !doc.fullscreenElement) trigger?.focus({ preventScroll: true });
    // Android restores system-bar insets after fullscreenchange. Wait for that
    // layout too, without moving a new fullscreen session or stealing focus.
    frame(() => frame(() => {
      if (!disposed && restoration === ticket && !doc.fullscreenElement) restoreScroll();
    }));
  };
  const changed = () => {
    const next = ownsFullscreen();
    if (active && !next) restore();
    active = next;
  };
  doc.addEventListener("fullscreenchange", changed);
  return {
    async enter() {
      if (disposed || pending || ownsFullscreen()) return;
      ++restoration;
      savedScroll = scroller ? { top: scroller.scrollTop, left: scroller.scrollLeft } : null;
      pending = true;
      stage.dataset.fullscreenPending = "true";
      try {
        await stage.requestFullscreen({ navigationUI: "hide" });
        if (disposed && ownsFullscreen()) await doc.exitFullscreen();
      } catch (error) {
        if (!disposed) { restore(); onError(error); }
      } finally {
        pending = false;
        delete stage.dataset.fullscreenPending;
      }
    },
    async exit() {
      if (!ownsFullscreen()) return;
      try { await doc.exitFullscreen(); }
      catch (error) { if (!disposed) onError(error); }
    },
    destroy() {
      disposed = true;
      ++restoration;
      doc.removeEventListener("fullscreenchange", changed);
      delete stage.dataset.fullscreenPending;
      if (ownsFullscreen()) void doc.exitFullscreen().catch(() => {});
    },
  };
}

export function androidVideoOwnsFocus(doc) {
  return Boolean(doc.fullscreenElement?.closest("[data-android-video-stage]") ||
    doc.querySelector("[data-android-video-stage][data-fullscreen-pending]"));
}
