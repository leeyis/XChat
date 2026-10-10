import { useEffect, useRef } from "react";
import { createPortal } from "react-dom";

export default function StabilityDialog({ title, children, actions, onClose, className="", portalRoot=document.body }) {
  const panel=useRef(null),close=useRef(onClose);close.current=onClose;
  useEffect(()=>{
    const prior=document.activeElement;
    const focusable=()=>[...panel.current.querySelectorAll('button:not(:disabled),input:not(:disabled),textarea:not(:disabled),select:not(:disabled),a[href],[tabindex="0"]')].filter(n=>n.getClientRects().length);
    (focusable()[0]||panel.current).focus();
    const key=event=>{
      if(event.key==="Escape"){event.preventDefault();event.stopPropagation();close.current?.();}
      if(event.key==="Tab"){
        const nodes=focusable(),first=nodes[0],last=nodes.at(-1);
        if(!first){event.preventDefault();panel.current.focus();}
        else if(event.shiftKey&&(document.activeElement===first||!panel.current.contains(document.activeElement))){event.preventDefault();last.focus();}
        else if(!event.shiftKey&&(document.activeElement===last||!panel.current.contains(document.activeElement))){event.preventDefault();first.focus();}
      }
    };
    document.addEventListener("keydown",key,true);
    return()=>{document.removeEventListener("keydown",key,true);if(prior?.isConnected)prior.focus();};
  },[]);
  return createPortal(<div className="modal-backdrop" role="presentation" onMouseDown={event=>{if(event.target===event.currentTarget)onClose?.();}}><section ref={panel} tabIndex={-1} className={`modal stability-dialog ${className}`} role="dialog" aria-modal="true" aria-label={title}><header><h2>{title}</h2><button aria-label="关闭" disabled={!onClose} onClick={onClose}>×</button></header><div className="stability-dialog-body">{children}</div>{actions&&<footer className="stability-dialog-actions">{actions}</footer>}</section></div>,portalRoot);
}
