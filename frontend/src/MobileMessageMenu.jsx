import { useLayoutEffect, useRef, useState } from "react";
import { messageMenuPosition } from "./mobile-menu.js";

const paths = {
  emoji: <><circle cx="12" cy="12" r="9"/><path d="M8 14.5c1.8 2 6.2 2 8 0M8.5 9h.01M15.5 9h.01"/></>,
  "plus-circle": <><circle cx="12" cy="12" r="9"/><path d="M12 8v8M8 12h8"/></>,
  forward: <path d="m14 4 6 6-6 6v-4c-5 0-8 2-10 6 0-7 3-10 10-10Z"/>,
  quote: <path d="M5 6h5v6H5V6Zm9 0h5v6h-5V6ZM10 12c0 4-2 6-5 6m14-6c0 4-2 6-5 6"/>,
  copy: <><rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V4H4v12h4"/></>,
  trash: <path d="M4 6h16M9 6V3h6v3M6 6l1 14h10l1-14M10 10v6m4-6v6"/>,
  select: <path d="M9 5h11M9 11h11M9 17h5M4 5h.01M4 11h.01M4 17h.01m12 1 2 2 4-5"/>,
  bell: <path d="M18 8a6 6 0 0 0-12 0c0 7-3 7-3 9h18c0-2-3-2-3-9ZM10 21h4M12 2V1"/>,
  recall: <path d="m8 4-5 5 5 5M3 9h10a7 7 0 0 1 7 7v4"/>,
  image: <><rect x="3" y="3" width="18" height="18" rx="3"/><circle cx="8" cy="8" r="1.5"/><path d="m3 17 5-5 4 4 4-6 5 7"/></>,
  camera: <><path d="m8 5 1-2h6l1 2h4a1 1 0 0 1 1 1v13H3V6a1 1 0 0 1 1-1h4Z"/><circle cx="12" cy="12" r="4"/></>,
  file: <path d="M14 3H5v18h14V8l-5-5Zm0 0v5h5M8 12h8m-8 4h5"/>,
  folder: <path d="M3 7V5a2 2 0 0 1 2-2h4l3 3h7a2 2 0 0 1 2 2v2M3 7v13h16l3-10H7L3 20"/>,
};

export function MobileIcon({ name }) {
  return <svg className="icon mobile-icon" width="24" height="24" viewBox="0 0 24 24" aria-hidden="true">{paths[name]}</svg>;
}

export default function MobileMessageMenu({ anchor, actions, onClose }) {
  const ref = useRef(null);
  const [position, setPosition] = useState(null);
  useLayoutEffect(() => {
    const element = ref.current;
    const chat = anchor?.closest(".chat-workspace");
    const target = anchor?.querySelector(".message-body-content") || anchor;
    if (!element || !target?.isConnected) { onClose(); return; }
    const viewport = globalThis.visualViewport;
    let top = Math.max(viewport?.offsetTop || 0, chat?.querySelector(".workspace-head")?.getBoundingClientRect().bottom || 0) + 8;
    let bottom = Math.min((viewport?.offsetTop || 0) + (viewport?.height || innerHeight), chat?.querySelector(".composer")?.getBoundingClientRect().top || innerHeight) - 8;
    // A landscape phone may have less than two menu rows between header and
    // composer. Use the safe WebView viewport instead of clipping the actions.
    if (bottom - top < element.offsetHeight) {
      top = (viewport?.offsetTop || 0) + 12;
      bottom = (viewport?.offsetTop || 0) + (viewport?.height || innerHeight) - 12;
    }
    setPosition(messageMenuPosition(target.getBoundingClientRect(), element.getBoundingClientRect(), { left: 12, right: innerWidth - 12, top, bottom }));
  }, [anchor, actions.length]);
  return <div className="mobile-menu-layer" onPointerDown={(event) => { event.stopPropagation(); if (event.target === event.currentTarget) onClose(); }}>
    <div ref={ref} className={`mobile-message-menu ${position?.below ? "below" : ""} ${actions.length > 5 ? "two-rows" : ""}`} role="menu" aria-label="消息操作"
      style={{ left: position?.left, top: position?.top, visibility: position ? "visible" : "hidden", "--menu-arrow-x": `${position?.arrow || 18}px` }}
      onKeyDown={(event) => {
        if (event.key === "Escape") onClose();
        const step = { ArrowRight: 1, ArrowLeft: -1, ArrowDown: 5, ArrowUp: -5 }[event.key];
        if (step === undefined) return;
        event.preventDefault();
        const buttons = [...ref.current.querySelectorAll("button:not(:disabled)")];
        buttons[(buttons.indexOf(document.activeElement) + step + buttons.length) % buttons.length]?.focus();
      }}>
      {actions.map((action) => <button type="button" role="menuitem" key={action.icon} disabled={action.disabled} title={action.title} onClick={action.run}><MobileIcon name={action.icon}/><span>{action.label}</span></button>)}
    </div>
  </div>;
}
