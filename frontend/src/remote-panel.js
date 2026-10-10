// Layout is independent from media, grants and session ownership.
export const newPanelMemory=()=>({position:null,compactPosition:null});
export function clampPanel(point,size,area,margin=8) {
  const left=Number.isFinite(area.x)?area.x:0,top=Number.isFinite(area.y)?area.y:0;
  return {
    x:Math.max(left+margin,Math.min(point.x,left+Math.max(margin,area.width-size.width-margin))),
    y:Math.max(top+margin,Math.min(point.y,top+Math.max(margin,area.height-size.height-margin))),
  };
}
export function panelStatus(session,compact=false) {
  if(session?.phase==='waiting')return '等待对方接受';
  if(session?.phase==='connecting')return '正在建立连接';
  if(session?.paused)return '共享已暂停';
  if(compact)return session?.local_host?(session.grant?'对方正在控制':'屏幕共享中'):(session?.grant?'远程控制中':'远程查看中');
  return session?.grant?`${session.peer_name} 正在控制我的电脑`:`${session?.peer_name||'对方'} 正在查看我的屏幕`;
}
const panelKey=id=>`xchat.remote.toolbar.${id}`;
export function readPanelCollapsed(id) {
  try{return localStorage.getItem(panelKey(id))==='true';}catch{return false;}
}
export function writePanelCollapsed(id,collapsed) {
  try{localStorage.setItem(panelKey(id),String(collapsed));}catch{}
  window.dispatchEvent(new CustomEvent('xchat:remote-toolbar-layout',{detail:{id,collapsed}}));
}
export function clearPanelCollapsed(id) {
  if(!id)return;
  try{localStorage.removeItem(panelKey(id));}catch{}
}
export function watchPanelCollapsed(id,callback) {
  const local=e=>{if(e.detail?.id===id)callback(e.detail.collapsed===true);};
  const storage=e=>{if(e.key===panelKey(id))callback(e.newValue==='true');};
  window.addEventListener('xchat:remote-toolbar-layout',local);window.addEventListener('storage',storage);
  return()=>{window.removeEventListener('xchat:remote-toolbar-layout',local);window.removeEventListener('storage',storage);};
}
