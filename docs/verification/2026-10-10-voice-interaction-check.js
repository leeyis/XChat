(async()=>{
 const q=s=>document.querySelector(s),wait=ms=>new Promise(r=>setTimeout(r,ms)),results=[];
 const click=s=>{const e=q(s);if(!e||!e.checkVisibility()||e.disabled)throw Error('Not actionable: '+s);e.click();};
 const act=(a,e='')=>click(`[data-p3-action="${a}"]${e}`),cv=(a,e='')=>click(`[data-cv-action="${a}"]${e}`);
 const set=(s,v)=>{const e=q(s);e.value=v;e.dispatchEvent(new Event('change',{bubbles:true}));};
 const check=(name,ok)=>{if(!ok)throw Error(name);results.push({name,passed:true});};
 const body=()=>q('#phase3Workspace').innerText,voice=()=>q('.ra-voice-bar')?.innerText||'';
 const reset=()=>{q('#phase3Dialog').close();click('.p3-reviewbar [data-page=remote]');act('reset');};
 const connect=async(mode='control',permission='view',offer=true,join=true)=>{reset();act('remote-start',`[data-mode=${mode}]`);if(!offer)click('#raOfferVoice');act('remote-send');act('role');if(offer&&!join)click('#raJoinVoice');act('remote-accept',`[data-permission=${permission}]`);await wait(750);};
 const suite=async(name,fn)=>{try{await fn();}catch(error){results.push({name,passed:false,error:error.message});q('#phase3Dialog').close();console.error('RA_VOICE_FAILURE',name,error.message);}};
 await suite('远程邀请附带语音与双方独立控制',async()=>{
   reset();act('remote-start','[data-mode=control]');check('两种请求中可直接邀请双向语音',q('#raOfferVoice').checked);act('remote-send');check('接受前无通话条或远程画面',!q('.ra-voice-bar')&&!q('.ra-desktop'));act('role');check('接收方明确选择加入语音',q('#raJoinVoice').checked&&q('#phase3Dialog').innerText.includes('开启麦克风'));act('remote-accept','[data-permission=view]');await wait(750);
   check('仅查看也能双向通话',voice().includes('双向通话')&&body().includes('仅查看'));
   act('voice-mic');check('共享方可关闭自己的麦克风',q('[data-p3-action=voice-mic]').getAttribute('aria-pressed')==='true');act('role');check('对方静音不改变本机麦克风',voice().includes('对方已静音')&&q('[data-p3-action=voice-mic]').getAttribute('aria-pressed')==='false');
   act('voice-speaker');check('关闭扬声器保留本机麦克风',voice().includes('暂时听不到对方')&&q('[data-p3-action=voice-mic]').getAttribute('aria-pressed')==='false');act('role');check('扬声器开关按双方分别保存',q('[data-p3-action=voice-speaker]').getAttribute('aria-pressed')==='false');
   act('voice-settings');set('#raVoiceInput','usb');set('#raVoiceOutput','headset');act('voice-settings-save');act('role');act('voice-settings');check('音频设备选择不影响对端',q('#raVoiceInput').value==='default'&&q('#raVoiceOutput').value==='default');act('close');act('role');act('voice-settings');check('本端音频设备选择保留',q('#raVoiceInput').value==='usb'&&q('#raVoiceOutput').value==='headset');act('close');
   act('remote-pause');check('暂停画面仍可双向语音',voice().includes('双向通话')&&!q('.ra-desktop'));act('remote-resume');act('remote-offer-control');act('remote-allow-control');act('remote-revoke');check('收回控制保留语音',voice().includes('双向通话')&&body().includes('仅查看'));act('remote-screen');click('[name=p3Screen][value="2"]');act('remote-screen-confirm');check('切换屏幕保留语音',voice().includes('双向通话')&&q('.ra-desktop').classList.contains('portrait'));
   act('remote-chat-toggle');check('收起文字沟通仍显示语音操作',!!q('[data-p3-action=voice-hangup]')&&!q('.ra-chat-panel'));act('remote-expand');check('展开桌面仍可静音和挂断',!!q('[data-p3-action=voice-mic]')&&!!q('[data-p3-action=voice-hangup]'));act('remote-expand');
 });
 await suite('中途呼叫与语音故障隔离',async()=>{
   act('voice-hangup');check('挂断语音不停止远程画面',voice().includes('语音已结束')&&!!q('.ra-desktop'));act('voice-call');check('任一方可重新呼叫',voice().includes('正在呼叫'));act('role');check('接收方有明确接听与拒绝入口',!!q('[data-p3-action=voice-accept]')&&!!q('[data-p3-action=voice-reject]'));act('voice-reject');check('拒绝方反馈方向正确',voice().includes('已拒绝'));act('role');check('呼叫方看见未接听',voice().includes('对方未接听'));act('voice-call');act('role');act('voice-accept');check('中途接听后通话恢复',voice().includes('双向通话'));
   set('#p3Scenario','mic-denied');check('麦克风不可用不阻断远程画面',voice().includes('麦克风权限未开启')&&!!q('.ra-desktop'));act('voice-call');act('role');act('voice-accept');check('处理音频故障后可重新邀请',voice().includes('双向通话'));set('#p3Scenario','voice-drop');check('语音掉线保留远程协助',voice().includes('语音连接已中断')&&!!q('.ra-desktop'));act('voice-call');act('voice-hangup');check('呼叫可取消',voice().includes('语音已结束'));
   set('#p3Scenario','locked');check('锁屏同时结束共享与通话',!q('.ra-desktop')&&!q('.ra-voice-bar'));
 });
 await suite('语音选择与会话生命周期',async()=>{
   await connect('help','view',false);check('发起人可只邀请协助',voice().includes('边说边协作')&&!voice().includes('双向通话'));act('voice-call');act('role');act('voice-accept');check('求助方向可中途接通语音',voice().includes('双向通话'));
   await connect('control','control',true,false);check('接收方可接受控制但不加入语音',voice().includes('边说边协作')&&body().includes('允许控制'));
   await connect('help','view',true,true);check('求助方向也能随请求直接通话',voice().includes('双向通话'));act('remote-end');check('结束协助清理通话工具',!q('.ra-voice-bar'));act('remote-retry');act('remote-send');act('role');check('重连仍需重新选择语音',!!q('#raJoinVoice'));act('remote-reject');check('拒绝远程请求不会开始语音',!q('.ra-voice-bar'));
 });
 await suite('聊天语音入口、录制与收发',async()=>{
   set('#desktopReviewScenario','voice-messages');q('#voiceMessagePlayer').muted=true;
   check('顶栏重复刷新按钮已从DOM移除',!q('#refreshPeerAddress'));if(!q('#app').classList.contains('info-open'))click('#moreBtn');check('更多抽屉保留刷新设备地址',!!q('[data-refresh-peer]'));click('[data-refresh-peer]');await wait(1550);check('更多中的刷新动作仍可完成',q('#chatSub').textContent.includes('地址已更新')||q('#chatSub').textContent.includes('连接已验证'));
   if(q('#app').classList.contains('info-open'))click('#moreBtn');check('语音入口紧邻发送按钮',q('#sendBtn').parentElement===q('#voiceMessageBtn').parentElement);
   q('#messageInput').value='保留这段文字草稿';q('#messageInput').dispatchEvent(new Event('input',{bubbles:true}));click('#voiceMessageBtn');check('录制显示取消波形发送箭头',!!q('.cv-wave')&&!q('#voiceRecorder').hidden&&q('[data-cv-action=send]').disabled);cv('cancel');check('取消录制保留文字草稿',q('#messageInput').value==='保留这段文字草稿'&&q('#voiceRecorder').hidden);
   click('#voiceMessageBtn');await wait(1250);cv('send');check('语音独立加入聊天并保留文字草稿',q('.cv-message.sent')&&q('#messageInput').value==='保留这段文字草稿');await wait(650);check('发送语音显示送达状态',q('.cv-message.sent').textContent.includes('已送达'));
   click('#voiceMessageBtn');cv('limit');check('60秒上限停留待发送而不自动发出',q('#voiceRecorder').classList.contains('ready')&&q('#voiceRecordSeconds').textContent==='1:00'&&!q('[data-cv-action=send]').disabled);cv('cancel');
   const count=q('#messages').querySelectorAll('.cv-message').length;click('#voiceMessageBtn');selectDevice('macbook');check('切换会话停止录制且无消息串入',q('#voiceRecorder').hidden&&!q('.cv-message'));selectDevice('zhang-3');check('返回会话保留已发与已收语音',q('#messages').querySelectorAll('.cv-message').length===count);
   set('#voiceMessageScenario','mic-denied');click('#voiceMessageBtn');check('麦克风权限失败有可恢复提示',q('#voiceRecordHint').textContent.includes('权限未开启')&&q('#voiceRecorder').hidden);cv('retry-record');check('修复权限后可重试录制',!q('#voiceRecorder').hidden);cv('cancel');
   set('#voiceMessageScenario','send-fail');click('#voiceMessageBtn');await wait(1250);cv('send');await wait(650);const before=q('#messages').querySelectorAll('.cv-message').length;cv('retry-send');await wait(650);check('发送重试沿用原语音条目',q('#messages').querySelectorAll('.cv-message').length===before&&!q('[data-cv-action=retry-send]'));
 });
 await suite('语音播放、独占与恢复',async()=>{
   set('#voiceMessageScenario','normal');cv('receive');const player=q('#voiceMessagePlayer');player.muted=true;const received=[...document.querySelectorAll('.cv-message:not(.sent)')],first=received[0].dataset.voiceRow,last=received.at(-1).dataset.voiceRow;
   cv('play',`[data-voice-id="${first}"]`);await wait(450);check('接收语音可直接播放本地音频',!player.paused&&q(`[data-voice-row="${first}"] .playing`)&&player.duration>0);check('播放后清除该语音未听标记',!q(`[data-voice-row="${first}"] .cv-unheard`));
   cv('play',`[data-voice-id="${first}"]`);check('再次点击暂停播放',player.paused&&!q('.cv-bubble.playing'));cv('play',`[data-voice-id="${first}"]`);await wait(150);cv('play',`[data-voice-id="${last}"]`);await wait(200);check('同时只播放一条语音',q('#messages').querySelectorAll('.cv-bubble.playing').length===1&&!!q(`[data-voice-row="${last}"] .playing`));
   selectDevice('macbook');check('切换会话停止播放',player.paused);selectDevice('zhang-3');set('#voiceMessageScenario','missing');cv('receive');check('文件缺失保留消息并提供重新接收',!!q('.cv-error-panel'));cv('restore');check('重新接收后恢复播放入口',!q('.cv-error-panel'));
   check('截图入口及原截图脚本保持存在',!!q('#captureBtn')&&[...document.scripts].some(s=>s.src.includes('snipaste-review')));set('#voiceMessageScenario','normal');q('#messageInput').value='';q('#messageInput').dispatchEvent(new Event('input',{bubbles:true}));
 });
 const report={time:new Date().toISOString(),results,passed:results.filter(r=>r.passed).length,failed:results.filter(r=>!r.passed).length};window.__voiceQA=report;console.log('RA_VOICE_RESULT',JSON.stringify(report));
})();
