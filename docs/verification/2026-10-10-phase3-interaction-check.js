(async () => {
  const results=[];
  const wait=ms=>new Promise(r=>setTimeout(r,ms));
  const $=selector=>document.querySelector(selector);
  const visible=el=>!!el&&el.checkVisibility()&&el.getBoundingClientRect().width>0;
  const click=selector=>{const el=$(selector);if(!visible(el)||el.disabled)throw Error('Not actionable: '+selector);el.click();};
  const act=(name,extra='')=>click(`[data-p3-action="${name}"]${extra}`);
  const set=(selector,value,event='change')=>{const el=$(selector);if(!visible(el))throw Error('Missing input: '+selector);el.value=value;el.dispatchEvent(new Event(event,{bubbles:true}));};
  const page=name=>click(`.p3-reviewbar [data-page="${name}"]`);
  const text=()=>$('#phase3Workspace').innerText;
  const dialog=()=>$('#phase3Dialog').innerText;
  const assert=(condition,message)=>{if(!condition)throw Error(message);};
  const check=(name,condition)=>{assert(condition,name);results.push({name,passed:true});};
  const reset=()=>act('reset');
  const suite=async(name,fn)=>{try{await fn();}catch(error){results.push({name,passed:false,error:error.message});console.error('P3_QA_FAILURE',name,error.message);$('#phase3Dialog').close();}};
  await suite('任务中心',async()=>{
    reset();page('tasks');
    check('任务初始六项和夹板图标',document.querySelectorAll('[data-task-id]').length===6&&!!$('.rail [data-page="tasks"] rect'));
    act('filter','[data-filter="attention"]');check('需要关注筛选',document.querySelectorAll('[data-task-id]').length===3);
    act('filter','[data-filter="all"]');set('#p3Search','不存在的文件','input');check('搜索空结果',text().includes('没有匹配的任务'));
    act('clear-filter');act('detail','[data-id="group"]');check('群消息逐收件人明细',text().includes('收件人进度')&&text().includes('王五'));
    act('detail-close');act('source','[data-id="source"]');click('[name="p3Source"][value="changed"]');act('source-confirm');check('不同源文件阻止续传',dialog().includes('不能作为续传源'));
    click('[name="p3Source"][value="same"]');act('source-confirm');check('相同源文件继续',text().includes('已找回原文件'));
    act('retry','[data-id="ack"]');act('cancel','[data-id="ack"]');act('cancel-confirm');await wait(1850);
    check('取消后异步重试不覆盖状态',$('[data-task-id="ack"]').innerText.includes('已取消'));
    act('retry','[data-id="group"]');await wait(1850);act('detail','[data-id="group"]');
    check('群重试完成且收件人明细更新',$('[data-task-id="group"]').innerText.includes('3 / 3 已送达')&&$('.p3-task-detail').innerText.includes('王五\n已送达'));
    act('detail-close');set('#p3Scenario','empty');check('空场景数量一致',!document.querySelectorAll('[data-task-id]').length&&[...document.querySelectorAll('.p3-metric b')].every(n=>n.textContent==='0'));
    reset();act('diagnose','[data-id="offline"]');check('等待任务打开正确诊断对象',$('#p3DiagPeer').value==='lisi'&&text().includes('192.168.1.63'));
  });
  await suite('连接诊断',async()=>{
    page('diagnostics');set('#p3DiagPeer','lisi');set('#p3Scenario','healthy');check('健康结果匹配所选设备',text().includes('连接属于李四'));
    act('export');check('诊断默认隐藏地址',dialog().includes('192.168.*.*')&&dialog().includes('连接正常'));click('#p3IncludeAddresses');check('明确选择附带当前地址',dialog().includes('192.168.1.63:8888'));act('close');
    set('#p3Scenario','identity');check('身份不匹配阻止发送',text().includes('与李四不一致')&&text().includes('已停止向这个地址发送'));
    set('#p3Scenario','normal');act('diag-run');check('检查期间不能重复提交',$('[data-p3-action="diag-run"]').disabled&&$('[data-p3-action="export"]').disabled);await wait(1250);check('检查结束提供下一步',text().includes('连接超时')&&visible($('[data-p3-action="network-settings"]')));
  });
  await suite('备份恢复',async()=>{
    page('backup');reset();act('backup-create');act('backup-start');act('backup-cancel');await wait(2000);check('取消备份不产生记录或迟到弹窗',document.querySelectorAll('.p3-backup-row').length===1&&!$('#phase3Dialog').open);
    set('#p3Scenario','disk');act('backup-create');click('#p3BackupAttachments');act('backup-start');check('空间检查计入附件',dialog().includes('2.5 GB')&&dialog().includes('80 MB'));
    act('backup-reselect');check('可更换保存位置',$('#p3BackupLocation').selectedIndex===1);act('backup-start');await wait(2050);check('校验成功才加入备份记录',dialog().includes('已通过完整性校验')&&document.querySelectorAll('.p3-backup-row').length===2);act('close');
    set('#p3Scenario','corrupt');act('restore');act('restore-check');check('损坏备份不能进入恢复',dialog().includes('未通过校验')&&!$('[data-p3-action="restore-run"]'));act('close');
    set('#p3Scenario','normal');act('restore');act('restore-check');check('恢复前预览合并与保留记录',dialog().includes('跳过重复')&&dialog().includes('全部保留')&&!$('#p3RestoreSettings').checked);
    click('#p3RestoreSettings');act('restore-run');check('恢复提交期间防重复操作',$('#phase3Dialog [data-p3-action="close"]').disabled);await wait(1550);check('恢复设置反馈遵循选择',dialog().includes('已恢复偏好设置')&&dialog().includes('快捷键'));act('close');
  });
  await suite('远程协助',async()=>{
    page('remote');reset();
    for(const state of ['offline','unsupported','busy']){set('#p3Scenario',state);check('不可用场景阻止请求：'+state,!$('[data-p3-action="remote-request"]')&&visible($('[data-p3-action="conversation"]')));}
    set('#p3Scenario','normal');act('remote-request');check('同意前无屏幕画面',!$('.p3-demo-screen'));
    act('role');check('收到查看请求需要明确同意',dialog().includes('Eason 请求查看')&&visible($('[data-p3-action="remote-allow-view"]')));act('remote-reject');check('拒绝请求保持无画面',!$('.p3-demo-screen')&&text().includes('未获同意'));
    act('remote-request');act('role');act('remote-allow-view');check('允许查看未授予控制',text().includes('Eason 正在查看')&&text().includes('鼠标与键盘\n未允许'));
    act('role');act('remote-control');act('role');check('控制必须再次确认',dialog().includes('允许 Eason 控制'));act('remote-deny-control');check('拒绝控制继续仅查看',text().includes('鼠标与键盘\n未允许'));
    act('role');act('remote-control');act('role');act('remote-allow-control');check('单独同意后允许控制',text().includes('Eason 正在控制'));
    act('remote-revoke');check('即时撤销控制',text().includes('鼠标与键盘\n未允许'));
    act('role');act('remote-control');act('role');act('remote-allow-control');act('remote-pause');check('暂停隐藏画面并撤回控制',!$('.p3-demo-screen')&&text().includes('鼠标与键盘\n未允许'));
    act('remote-resume');check('继续共享仍为仅查看',!!$('.p3-demo-screen')&&text().includes('鼠标与键盘\n未允许'));
    act('remote-screen');click('[name="p3Screen"][value="2"]');act('remote-screen-confirm');check('共享方选择第二屏',$('.p3-demo-screen').classList.contains('portrait'));
    set('#p3Scenario','disconnect');check('断线隐藏画面',!$('.p3-demo-screen')&&text().includes('重新同意'));act('remote-request');act('role');act('remote-allow-view');check('重连必须重新同意且无控制',text().includes('鼠标与键盘\n未允许'));
    set('#p3Scenario','locked');check('锁屏隐藏画面',!$('.p3-demo-screen')&&text().includes('屏幕已锁定'));act('remote-end');check('结束会话收回授权',text().includes('鼠标键盘权限已收回'));
    reset();act('remote-invite');act('remote-invite-send');check('邀请接受前不共享',!$('.p3-demo-screen')&&text().includes('当前没有画面'));
    act('role');act('remote-invite-accept');check('邀请方向显示正确共享者',text().includes('正在查看Eason的屏幕'));
    act('remote-control');act('role');check('邀请方向控制申请来自张三',dialog().includes('允许 张三 控制'));act('remote-allow-control');check('邀请方向共享方身份正确',text().includes('张三 正在控制你的电脑'));
    act('remote-end');
  });
  await suite('主题与原有入口',async()=>{
    page('tasks');act('theme');check('深色主题切换',document.documentElement.dataset.theme==='dark');act('theme');
    check('页面无横向溢出',document.documentElement.scrollWidth<=innerWidth);
    act('exit');check('退出评审隐藏阶段三界面',!document.body.classList.contains('p3-review')&&!visible($('#phase3Workspace')));
    check('原截图入口保留',!!$('#captureBtn')&&[...document.scripts].some(s=>s.src.includes('snipaste-review')));
    set('#desktopReviewScenario','phase3');check('可重新进入阶段三',document.body.classList.contains('p3-review'));
  });
  reset();page('tasks');
  window.__p3QA={time:new Date().toISOString(),viewport:[innerWidth,innerHeight],results,passed:results.filter(r=>r.passed).length,failed:results.filter(r=>!r.passed).length};
  const out=document.createElement('pre');out.id='p3QAResult';out.hidden=true;out.textContent=JSON.stringify(window.__p3QA);document.body.append(out);
  console.log('P3_QA_RESULT',JSON.stringify(window.__p3QA));
})();
