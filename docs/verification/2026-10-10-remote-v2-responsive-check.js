(async()=>{
  document.querySelector('#raViewportFrame')?.remove();
  document.querySelector('#phase3Dialog').close();
  const frame=document.createElement('iframe');frame.id='raViewportFrame';frame.title='远程协助第二轮窗口验证';frame.style.cssText='position:fixed;z-index:100;left:0;top:0;border:0;background:white;height:844px;';document.body.append(frame);
  const results=[],wait=ms=>new Promise(r=>setTimeout(r,ms));
  try {
    for(const width of [1000,860,390]) {
      frame.style.width=width+'px';
      await new Promise((resolve,reject)=>{frame.onload=resolve;frame.onerror=reject;frame.src='/xchat-desktop-prototype.html?review=phase3&p3=remote&viewport=v2-'+width;});
      const win=frame.contentWindow,doc=frame.contentDocument,q=s=>doc.querySelector(s),act=(action,extra='')=>q(`[data-p3-action="${action}"]${extra}`).click();
      const close=()=>q('#phase3Dialog').close(),reset=()=>{close();q('.p3-reviewbar [data-page=remote]').click();act('reset');};
      const measure=async(name)=>{
        const rect=q('#phase3Workspace').getBoundingClientRect(),dialog=q('#phase3Dialog'),d=dialog.open?dialog.getBoundingClientRect():null;
        const dialogFits=!d||(d.width>0&&d.left>=0&&d.right<=win.innerWidth+1&&d.top>=0&&d.bottom<=win.innerHeight+1);
        const controls=[...doc.querySelectorAll('.ra-session-head [data-p3-action=remote-end],.ra-sharing-strip [data-p3-action]')].map(el=>{const r=el.getBoundingClientRect();return {action:el.dataset.p3Action,left:r.left,right:r.right,bottom:r.bottom};});
        const controlsFit=controls.every(r=>r.left>=0&&r.right<=win.innerWidth+1&&r.bottom<=win.innerHeight+1);
        results.push({width,theme:doc.documentElement.dataset.theme,state:name,actualViewport:win.innerWidth,documentWidth:doc.documentElement.scrollWidth,root:{left:rect.left,right:rect.right,width:rect.width},dialogFits,controlsFit,passed:doc.documentElement.scrollWidth<=win.innerWidth&&rect.width>0&&rect.right<=win.innerWidth+1&&dialogFits&&controlsFit});
      };
      for(const theme of ['light','dark']) {
        doc.documentElement.dataset.theme=theme;
        reset();await measure('双入口');
        act('remote-start','[data-mode=help]');await measure('请求协助对话框');close();
        act('remote-start','[data-mode=control]');await measure('请求控制对话框');act('remote-send');await measure('等待回应');
        act('role');await measure('接收控制请求');act('remote-accept','[data-permission=control]');await wait(750);
        await measure('共享方控制中');act('remote-pause');await measure('共享暂停');act('remote-resume');act('role');await measure('协助方桌面');
        act('remote-expand');await measure('展开桌面');act('remote-expand');
        act('remote-quality');await measure('画质对话框');close();
        for(const page of ['tasks','diagnostics','backup']) {q(`.p3-reviewbar [data-page=${page}]`).click();await measure(page);}
      }
    }
  } catch(error) {results.push({passed:false,error:error.message});}
  finally {frame.remove();}
  const report={time:new Date().toISOString(),results,passed:results.filter(r=>r.passed).length,failed:results.filter(r=>!r.passed).length};
  window.__raV2Responsive=report;console.log('RA_V2_RESPONSIVE',JSON.stringify(report));
})();
