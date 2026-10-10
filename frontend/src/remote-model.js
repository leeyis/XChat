export const remoteLive=session=>Boolean(session&&["waiting","connecting","active"].includes(session.phase));
export const remoteAccepted=session=>Boolean(session&&["connecting","active"].includes(session.phase));
export function remotePoint(rect,width,height,x,y) {
  if(!rect||!width||!height||rect.width<=0||rect.height<=0)return null;
  const scale=Math.min(rect.width/width,rect.height/height),w=width*scale,h=height*scale;
  const left=rect.left+(rect.width-w)/2,top=rect.top+(rect.height-h)/2;
  const point={x:(x-left)/w,y:(y-top)/h};
  return Number.isFinite(point.x)&&Number.isFinite(point.y)&&point.x>=0&&point.x<=1&&point.y>=0&&point.y<=1?point:null;
}
export function remoteInputAllowed(session,owned=true) {
  return Boolean(owned&&session?.phase==="active"&&!session.local_host&&!session.paused&&session.grant);
}
export function remoteDuration(start,now=Date.now()) {
  const seconds=Math.max(0,Math.floor(now/1000-(start||now/1000)));
  return `${String(Math.floor(seconds/60)).padStart(2,"0")}:${String(seconds%60).padStart(2,"0")}`;
}
export function remoteQuality(quality={}) {
  return {maxFramerate:[10,20,30,60].includes(quality.fps)?quality.fps:30,maxBitrate:quality.preset==="fluent"?900000:quality.preset==="clear"?5000000:2500000};
}
