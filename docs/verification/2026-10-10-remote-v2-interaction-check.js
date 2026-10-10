(async()=>{
  const results=[],wait=ms=>new Promise(r=>setTimeout(r,ms)),q=selector=>document.querySelector(selector);
  const text=()=>q('#phase3Workspace').innerText,dialog=()=>q('#phase3Dialog').innerText;
  const click=selector=>{const el=q(selector);if(!el||!el.checkVisibility()||el.disabled)throw Error('Not actionable: '+selector);el.click();};
  const act=(action,extra='')=>click(`[data-p3-action="${action}"]${extra}`);
  const set=(selector,value)=>{const el=q(selector);if(!el)throw Error('Missing: '+selector);el.value=value;el.dispatchEvent(new Event('change',{bubbles:true}));};
  const check=(name,ok)=>{if(!ok)throw Error(name);results.push({name,passed:true});};
  const suite=async(name,fn)=>{try{await fn();}catch(error){results.push({name,passed:false,error:error.message});q('#phase3Dialog').close();console.error('RA_V2_FAILURE',name,error.message);}};
  const reset=()=>{q('#phase3Dialog').close();click('.p3-reviewbar [data-page=remote]');act('reset');};
  const start=mode=>{act('remote-start',`[data-mode=${mode}]`);act('remote-send');};
  const accept=async permission=>{act('remote-accept',`[data-permission=${permission}]`);await wait(750);};
  await suite('双入口与请求协助',async()=>{
    reset();check('入口明确区分请求协助与请求控制',text().includes('请求对方协助')&&text().includes('请求控制对方'));
    act('remote-start','[data-mode=help]');check('请求协助选择我的屏幕且控制默认关闭',dialog().includes('请求张三远程协助')&&!q('#raOfferControl').checked);
    click('[name=p3Screen][value="2"]');q('#raRequestNote').value='请帮我检查网络';act('remote-send');
    check('求助方等待时没有共享画面',text().includes('正在邀请张三协助我')&&!q('.ra-desktop'));
    act('role');check('协助方收到正确方向的请求',dialog().includes('Eason 请求你远程协助')&&dialog().includes('Eason 的屏幕 2')&&dialog().includes('仅查看'));
    await accept('view');check('接受协助后查看Eason且不能控制',text().includes('Eason的远程桌面')&&text().includes('当前仅查看')&&q('[data-p3-action=remote-demo-open]').disabled);
    act('remote-control');check('仅查看升级需等待授权',text().includes('等待授权'));
    act('role');check('控制授权由Eason授予张三',dialog().includes('允许 张三 控制你的电脑'));
    act('remote-deny-control');check('拒绝升级维持仅查看',text().includes('仅查看'));
    act('remote-offer-control');act('remote-allow-control');check('共享方可明确授予本次控制',text().includes('张三 正在控制我的电脑'));
    act('remote-revoke');check('共享方即时收回控制',text().includes('张三 正在查看我的屏幕'));
    act('remote-end');act('remote-retry');check('求助方向重试仍共享本机且不继承控制',dialog().includes('请求张三远程协助')&&!q('#raOfferControl').checked);act('close');
    reset();act('remote-start','[data-mode=help]');click('#raOfferControl');act('remote-send');act('role');check('求助时明确勾选的权限告知协助方',dialog().includes('已明确允许本次鼠标键盘控制'));await accept('control');check('协助方接受已明确授权的控制',text().includes('允许控制')&&!q('[data-p3-action=remote-demo-open]').disabled);
  });
  await suite('请求控制对方',async()=>{
    reset();act('remote-start','[data-mode=control]');check('控制请求不选择或共享自己的屏幕',dialog().includes('请求控制张三的电脑')&&!q('[name=p3Screen]'));act('remote-send');act('role');
    check('被控方选择屏幕并有三种回应',dialog().includes('请求控制你的电脑')&&dialog().includes('仅允许查看')&&dialog().includes('允许本次控制')&&!!q('[data-p3-action=remote-reject]'));
    await accept('view');act('role');check('对方降级同意后只查看张三',text().includes('张三的远程桌面')&&text().includes('当前仅查看'));
    act('remote-end');reset();start('control');act('role');click('[name=p3Screen][value="2"]');await accept('control');act('role');
    check('直接控制路径无需重复申请',text().includes('允许控制')&&q('.ra-desktop').classList.contains('portrait'));
    act('remote-demo-open');check('授权后示意桌面可响应操作',text().includes('以太网适配器'));
    act('remote-revoke');check('协助方可以主动释放控制',text().includes('当前仅查看')&&q('[data-p3-action=remote-demo-open]').disabled);
  });
  await suite('会话工具与沟通',async()=>{
    reset();start('control');act('role');await accept('control');act('role');
    set('#raScale','actual');check('原始比例可滚动查看',q('.ra-desktop').classList.contains('actual'));set('#raScale','fit');
    act('remote-quality');click('[name=raQuality][value="优先流畅"]');q('.ra-advanced').open=true;set('#raFrameLimit','10');set('#raColor','reduced');act('remote-quality-save');check('画质刷新色彩设置保存',text().includes('优先流畅')&&text().includes('10 FPS')&&q('.ra-desktop').classList.contains('reduced'));
    act('remote-expand');check('全屏桌面展开',document.body.classList.contains('p3-remote-expanded'));document.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));check('Esc退出展开桌面',!document.body.classList.contains('p3-remote-expanded'));
    q('#raChatDraft').value='我看到网络已经连上了';act('remote-chat-send');check('协助沟通保留本地演示消息',text().includes('我看到网络已经连上了'));
    act('remote-chat-toggle');check('沟通面板可收起',!q('.ra-chat-panel'));act('remote-chat-toggle');check('沟通面板重开保留消息',text().includes('我看到网络已经连上了'));
    act('role');check('共享方始终可见收回暂停和结束',!!q('.ra-sharing-strip [data-p3-action=remote-revoke]')&&!!q('[data-p3-action=remote-pause]')&&!!q('.ra-sharing-strip [data-p3-action=remote-end]'));
    act('remote-pause');check('暂停隐藏桌面并收回控制',!q('.ra-desktop')&&text().includes('鼠标键盘权限已收回'));act('remote-resume');check('恢复只查看不继承控制',!!q('.ra-desktop')&&text().includes('仅查看'));
    act('remote-offer-control');act('remote-allow-control');act('remote-screen');click('[name=p3Screen][value="2"]');act('remote-screen-confirm');check('共享方切换屏幕收回控制',q('.ra-desktop').classList.contains('portrait')&&text().includes('仅查看'));
    set('#p3Scenario','weak');check('网络波动提示与画质处理入口',text().includes('网络有波动')&&!!q('[data-p3-action=remote-fast]'));act('remote-fast');
    set('#p3Scenario','disconnect');check('断线隐藏桌面并清理展开状态',!q('.ra-desktop')&&!document.body.classList.contains('p3-remote-live')&&text().includes('重新连接需要'));
    act('remote-retry');check('断线重试仍请求控制对方',dialog().includes('请求控制张三的电脑'));act('remote-send');act('role');await accept('control');set('#p3Scenario','locked');check('锁屏不保留共享画面',!q('.ra-desktop')&&text().includes('已锁屏'));
  });
  await suite('拒绝取消超时与竞态',async()=>{
    reset();start('help');act('role');act('remote-reject');check('拒绝不建立会话',text().includes('没有同意')&&!q('.ra-desktop'));
    reset();start('control');act('remote-cancel');check('发起方可以取消请求',text().includes('请求已取消')&&!q('.ra-desktop'));
    reset();start('control');act('role');act('remote-accept','[data-permission=control]');act('remote-cancel');await wait(800);check('连接中取消不被迟到回调激活',text().includes('请求已取消')&&!q('.ra-desktop'));
    reset();start('help');set('#p3Scenario','timeout');check('请求超时可重新发起',text().includes('请求已过期')&&!q('.ra-desktop'));act('remote-retry');check('超时重试保持求助方向',dialog().includes('请求张三远程协助'));act('close');
    for(const scenario of ['offline','unsupported','busy']){reset();set('#p3Scenario',scenario);check('不可用时禁用两个发起入口：'+scenario,[...document.querySelectorAll('.ra-mode-card')].every(el=>el.disabled));}
  });
  await suite('导航与其它原型回归',async()=>{
    reset();start('control');act('role');await accept('view');click('.p3-reviewbar [data-page=tasks]');check('离开远程页面恢复正常布局',!document.body.classList.contains('p3-remote-live')&&document.querySelectorAll('[data-task-id]').length===6);
    click('.p3-reviewbar [data-page=remote]');check('返回保留当前远程会话',document.body.classList.contains('p3-remote-live')&&!!q('.ra-desktop'));
    click('.p3-reviewbar [data-page=diagnostics]');check('诊断入口保留',!!q('#p3DiagPeer')&&q('#settingsListTitle').textContent==='设置');click('.p3-reviewbar [data-page=backup]');check('备份入口保留',!!q('[data-p3-action=backup-create]'));
    act('exit');check('退出评审清理远程扩展布局',!document.body.classList.contains('p3-remote-live')&&!document.body.classList.contains('p3-remote-expanded'));check('截图入口与资源仍存在',!!q('#captureBtn')&&[...document.scripts].some(el=>el.src.includes('snipaste-review')));set('#desktopReviewScenario','phase3');
  });
  reset();
  const report={time:new Date().toISOString(),viewport:[innerWidth,innerHeight],results,passed:results.filter(r=>r.passed).length,failed:results.filter(r=>!r.passed).length};
  window.__raV2QA=report;console.log('RA_V2_RESULT',JSON.stringify(report));
})();
