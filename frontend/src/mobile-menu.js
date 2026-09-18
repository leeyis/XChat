// Keep the popup inside the visible chat area, including a raised keyboard.
export function messageMenuPosition(anchor, size, bounds) {
  const gap = 12;
  const center = (anchor.left + anchor.right) / 2;
  const left = Math.max(bounds.left, Math.min(center - size.width / 2, bounds.right - size.width));
  const above = anchor.top - size.height - gap;
  const below = above < bounds.top;
  const top = Math.max(bounds.top, Math.min(below ? anchor.bottom + gap : above, bounds.bottom - size.height));
  return { left, top, below, arrow: Math.max(18, Math.min(center - left, size.width - 18)) };
}
