/* Prototype only: simulated recording/transport, local synthesized playback fixture. */
(() => {
  const q=id=>document.getElementById(id), esc=value=>String(value).replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
  const glyph={voice:'<circle cx="12" cy="12" r="9"/><path d="M9 10a3 3 0 0 1 0 4m3-6a6 6 0 0 1 0 8m3-10a9 9 0 0 1 0 12"/>',wave:'<path d="M5 10a3 3 0 0 1 0 4m4-7a7 7 0 0 1 0 10m4-13a11 11 0 0 1 0 16"/>',close:'<path d="m6 6 12 12M6 18 18 6"/>',send:'<path d="M12 20V4m-6 6 6-6 6 6"/>',pause:'<path d="M8 5v14M16 5v14"/>'};
  const icon=name=>`<svg class="cv-icon" viewBox="0 0 24 24" aria-hidden="true">${glyph[name]}</svg>`;
  // Legacy review closures retain their refresh controller; the duplicate header node is removed.
  q('refreshPeerAddress')?.remove();
  q('moreBtn').title='更多';q('moreBtn').setAttribute('aria-label','更多');
  const sendArea=document.createElement('div');sendArea.className='cv-send-area';q('sendBtn').before(sendArea);
  sendArea.innerHTML=`<button class="icon-btn cv-entry" id="voiceMessageBtn" type="button" title="语音消息" aria-label="录制语音消息">${icon('voice')}</button><div class="cv-recorder" id="voiceRecorder" hidden><button class="cv-cancel" type="button" data-cv-action="cancel" aria-label="取消录制" title="取消录制">${icon('close')}</button><div class="cv-record-pill"><span class="cv-record-seconds" id="voiceRecordSeconds">0:00</span><span class="cv-wave" aria-hidden="true">${[1,3,5,7,6,4,2,3,1].map(i=>`<i style="--i:${i}"></i>`).join('')}</span><button class="cv-record-send" type="button" data-cv-action="send" aria-label="发送语音" title="发送语音" disabled>${icon('send')}</button></div></div>`;
  sendArea.append(q('sendBtn'));
  const hint=document.createElement('div');hint.className='cv-record-hint';hint.id='voiceRecordHint';hint.hidden=true;sendArea.closest('.compose-toolbar').after(hint);
  const player=document.createElement('audio');player.id='voiceMessagePlayer';player.preload='none';player.hidden=true;player.src='assets/voice-message-demo.wav';document.body.append(player);
  const select=q('desktopReviewScenario');select.add(new Option('语音消息 · 录制与播放','voice-messages'));
  const controls=document.createElement('div');controls.className='cv-review-controls';controls.hidden=true;controls.innerHTML='<p>语音消息评审：录制和收发使用模拟数据；点击气泡播放本地合成示例语音，不使用麦克风。</p><select id="voiceMessageScenario" aria-label="语音消息演示场景"><option value="normal">正常录制与收发</option><option value="mic-denied">麦克风权限不可用</option><option value="send-fail">发送失败</option><option value="missing">收到的语音文件丢失</option></select><button type="button" data-cv-action="receive">模拟收到语音</button><button type="button" data-cv-action="limit">演示录满 60 秒</button><button type="button" data-cv-action="reset">重置语音演示</button>';
  document.querySelector('.desktop-review-tools').append(controls);
  const buckets=new Map();let recording=null,recordTimer,playTimer,playing=null,playEpoch=0,sequence=0,review=false,scenario='normal';
  const entries=id=>{if(!buckets.has(id))buckets.set(id,[]);return buckets.get(id);};
  const seconds=()=>Math.min(60,Math.floor((Date.now()-recording.startedAt)/1000));
  const clock=n=>`${Math.floor(n/60)}:${String(n%60).padStart(2,'0')}`;
  function syncRecord() {
    const active=!!recording;q('voiceMessageBtn').hidden=active||!!current().ai;q('sendBtn').hidden=active;q('voiceRecorder').hidden=!active;hint.hidden=!active;
    if(!active)return;
    const duration=seconds();q('voiceRecordSeconds').textContent=clock(duration);q('voiceRecorder').querySelector('[data-cv-action=send]').disabled=duration<1;q('voiceRecorder').classList.toggle('ready',duration>=60);hint.textContent=duration>=60?'已录满 60 秒，可发送或取消':'正在录音 · 最长 60 秒';
    if(duration>=60)clearInterval(recordTimer);
  }
  function cancelRecord(restoreFocus=false) {clearInterval(recordTimer);recording=null;syncRecord();if(restoreFocus)q('voiceMessageBtn').focus();}
  function beginRecord() {
    if(recording||current().ai||q('chat').hidden)return;
    if(scenario==='mic-denied'){hint.hidden=false;hint.innerHTML='<span class="error">麦克风权限未开启，请在系统设置中允许后重试。</span><button type="button" class="cv-inline-action" data-cv-action="retry-record">重试</button>';return;}
    stopPlayback();recording={conversation:current().id,startedAt:Date.now()};syncRecord();recordTimer=setInterval(syncRecord,200);q('voiceRecorder').querySelector('[data-cv-action=cancel]').focus();
  }
  function messageRow(item) {
    const active=playing?.item===item,unheard=!item.mine&&!item.heard,unavailable=item.state==='missing';
    const content=unavailable?'<div class="cv-error-panel"><strong>语音文件不可用</strong>消息记录已保留。<button type="button" class="cv-inline-action" data-cv-action="restore" data-voice-id="'+item.id+'">重新接收</button></div>':`<button type="button" class="cv-bubble ${active?'playing':''}" data-cv-action="play" data-voice-id="${item.id}" style="--cv-width:${Math.min(260,130+item.duration*3)}px" aria-label="${active?'暂停':'播放'}示例语音，${item.duration} 秒" aria-pressed="${active}">${icon(active?'pause':'wave')}<span class="cv-duration">${item.duration}″</span><span class="cv-play-track"><i style="--cv-progress:${Math.round((item.position||0)/item.duration*100)}%"></i></span>${unheard?'<i class="cv-unheard" aria-label="尚未播放"></i>':''}</button><span class="cv-play-copy">${item.playError?'播放失败，点击重试':active?'正在播放 '+clock(Math.floor(item.position||0)):item.position?'已暂停 · 点击继续':item.mine?'点击播放':item.heard?'已播放':'点击播放 · 未播放'}</span>`;
    const stateText={sending:'正在发送',delivered:'已送达',waiting:'等待对方上线',failed:'发送失败'}[item.state]||'';
    return `<div class="msg ${item.mine?'sent':''} cv-message" data-message-type="voice" data-voice-row="${item.id}">${avatar(item.mine?{id:'self',name:'我'}:current().group?devices.find(d=>d.id==='zhang-3'):current())}<div class="msg-stack">${current().group&&!item.mine?'<span class="sender-label">张三</span>':''}${content}<div class="cv-message-meta"><span>刚刚${item.mine?' · '+stateText:''}</span>${item.state==='failed'?`<button class="cv-inline-action error" type="button" data-cv-action="retry-send" data-voice-id="${item.id}">重试发送</button>`:''}</div></div></div>`;
  }
  function renderVoiceRows() {q('messages').querySelectorAll('.cv-message').forEach(el=>el.remove());if(current().ai)return;entries(current().id).forEach(item=>q('messages').insertAdjacentHTML('beforeend',messageRow(item)));}
  function redrawItem(item) {const old=q('messages').querySelector(`[data-voice-row="${item.id}"]`);if(old)old.outerHTML=messageRow(item);}
  function sendVoice() {
    if(!recording||recording.conversation!==current().id||seconds()<1)return;
    const conversation=recording.conversation,item={id:'voice-'+ ++sequence,duration:seconds(),mine:true,state:!current().group&&!current().online?'waiting':'sending',position:0,heard:false},fail=scenario==='send-fail';
    entries(conversation).push(item);cancelRecord(true);renderVoiceRows();q('messagesScroll').scrollTop=q('messagesScroll').scrollHeight;
    if(item.state==='sending')setTimeout(()=>{if(!entries(conversation).includes(item))return;item.state=fail?'failed':'delivered';if(current().id===conversation)redrawItem(item);},550);
  }
  function receiveVoice() {const item={id:'voice-'+ ++sequence,duration:13,mine:false,state:scenario==='missing'?'missing':'delivered',position:0,heard:false};entries(current().id).push(item);renderVoiceRows();q('messagesScroll').scrollTop=q('messagesScroll').scrollHeight;}
  function stopPlayback(reset=false) {
    playEpoch++;clearInterval(playTimer);player.pause();
    if(playing){const item=playing.item;if(reset)item.position=0;playing=null;redrawItem(item);}
  }
  async function playVoice(item) {
    if(playing?.item===item){stopPlayback();q('messages').querySelector(`[data-voice-id="${item.id}"]`)?.focus();return;}
    stopPlayback();q('messages').querySelectorAll('audio,video').forEach(el=>el.pause());const epoch=++playEpoch;item.playError=false;player.loop=item.mine;player.currentTime=(item.position||0)%(Number.isFinite(player.duration)&&player.duration>0?player.duration:60);player.onended=()=>{if(playing?.item===item)stopPlayback(true);};
    try {await player.play();if(epoch!==playEpoch)return;item.heard=true;playing={item,startedAt:Date.now()-(item.position||0)*1000};redrawItem(item);q('messages').querySelector(`[data-voice-id="${item.id}"]`)?.focus();
      playTimer=setInterval(()=>{if(!playing||playing.item!==item)return;item.position=Math.min(item.duration,(Date.now()-playing.startedAt)/1000);if(item.position>=item.duration){stopPlayback(true);return;}const row=q('messages').querySelector(`[data-voice-row="${item.id}"]`);row?.querySelector('.cv-play-track>i')?.style.setProperty('--cv-progress',Math.round(item.position/item.duration*100)+'%');const copy=row?.querySelector('.cv-play-copy');if(copy)copy.textContent='正在播放 '+clock(Math.floor(item.position));},200);
    } catch {if(epoch!==playEpoch)return;item.playError=true;playing=null;redrawItem(item);}
  }
  const oldRender=renderMessages;renderMessages=function(entity){oldRender(entity);renderVoiceRows();syncRecord();};
  const oldSelect=selectDevice;selectDevice=function(id){if(id!==current().id){cancelRecord();stopPlayback();}oldSelect(id);};
  const oldTab=setTab;setTab=function(tab){cancelRecord();stopPlayback();oldTab(tab);};
  q('voiceMessageBtn').onclick=beginRecord;
  q('messageInput').addEventListener('keydown',event=>{if(recording&&event.key==='Enter'&&!event.shiftKey&&!event.isComposing){event.preventDefault();event.stopImmediatePropagation();}},true);
  q('voiceRecorder').addEventListener('keydown',event=>{if(event.key==='Escape'){event.preventDefault();event.stopPropagation();cancelRecord(true);}});
  document.addEventListener('play',event=>{if(event.target!==player&&playing)stopPlayback();},true);
  document.addEventListener('click',event=>{
    const target=event.target.closest('[data-cv-action]');if(!target||target.disabled)return;
    const action=target.dataset.cvAction,item=entries(current().id).find(x=>x.id===target.dataset.voiceId);
    if(action==='cancel')cancelRecord(true);
    else if(action==='send')sendVoice();
    else if(action==='retry-record'){scenario='normal';q('voiceMessageScenario').value='normal';beginRecord();}
    else if(action==='receive')receiveVoice();
    else if(action==='reset'){cancelRecord();stopPlayback();buckets.clear();scenario='normal';q('voiceMessageScenario').value='normal';renderVoiceRows();receiveVoice();}
    else if(action==='limit'){if(!recording)beginRecord();if(recording){recording.startedAt=Date.now()-60000;syncRecord();}}
    else if(action==='play'&&item)playVoice(item);
    else if(action==='retry-send'&&item){item.state='sending';redrawItem(item);const conversation=current().id;setTimeout(()=>{if(!entries(conversation).includes(item))return;item.state='delivered';if(current().id===conversation)redrawItem(item);},550);}
    else if(action==='restore'&&item){item.state='delivered';redrawItem(item);}
  });
  q('voiceMessageScenario').addEventListener('change',event=>{cancelRecord();scenario=event.target.value;});
  function enableReview(){review=true;document.body.classList.add('cv-review');controls.hidden=false;setTab('sessions');selectDevice('zhang-3');if(!entries('zhang-3').length)receiveVoice();}
  select.addEventListener('change',()=>{if(select.value==='voice-messages')enableReview();else if(review){review=false;document.body.classList.remove('cv-review');controls.hidden=true;cancelRecord();stopPlayback();}});
  if(new URLSearchParams(location.search).get('review')==='voice-messages'){select.value='voice-messages';select.dispatchEvent(new Event('change',{bubbles:true}));}
  syncRecord();
})();
