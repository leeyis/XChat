(async()=>{
  document.querySelector('#p3ViewportFrame')?.remove();
  document.querySelector('#phase3Dialog').close();
  const frames=document.createElement('iframe');frames.id='p3ViewportFrame';frames.title='阶段三窄窗口验证';frames.style.cssText='position:fixed;z-index:100;left:0;top:0;border:0;background:white;height:820px;';document.body.append(frames);
  const results=[];
  for(const width of [1000,860,390]) {
    frames.style.width=width+'px';
    await new Promise((resolve,reject)=>{frames.onload=resolve;frames.onerror=reject;frames.src='/xchat-desktop-prototype.html?review=phase3&viewport='+width;});
    const win=frames.contentWindow,doc=frames.contentDocument;
    for(const theme of ['light','dark']){
      doc.documentElement.dataset.theme=theme;
      for(const page of ['tasks','diagnostics','backup','remote']){
        doc.querySelector(`.p3-reviewbar [data-page="${page}"]`).click();
        doc.querySelector('[data-p3-action="reset"]').click();
        if(page==='remote'){
          for(const action of ['remote-request','role','remote-allow-view'])doc.querySelector(`[data-p3-action="${action}"]`).click();
        }
        await new Promise(r=>setTimeout(r,30));
        const root=doc.querySelector('#phase3Workspace'),rect=root.getBoundingClientRect();
        results.push({width,theme,page,actualViewport:win.innerWidth,documentWidth:doc.documentElement.scrollWidth,root:{left:rect.left,right:rect.right,width:rect.width},passed:doc.documentElement.scrollWidth<=win.innerWidth&&rect.width>0&&rect.right<=win.innerWidth+1});
      }
    }
  }
  frames.remove();console.log('P3_RESPONSIVE',JSON.stringify({results,failed:results.filter(r=>!r.passed).length}));
})();
