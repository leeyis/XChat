/* Prototype only: all state is fictional and in memory. No capture or remote APIs. */
(() => {
  const byId = id => document.getElementById(id);
  const esc = value => String(value).replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
  const paths = {
    tasks:'<rect x="8" y="2" width="8" height="4" rx="1.5"/><path d="M8 4H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V6a2 2 0 0 0-2-2h-2m-8 10 3 3 5-6"/>',
    file:'<path d="M6 3h8l4 4v14H6zM14 3v5h4M9 13h6M9 17h4"/>',
    chat:'<path d="M5 4h14a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H9l-5 3v-3H3V6a2 2 0 0 1 2-2Z"/>',
    check:'<path d="m5 12 4 4L19 6"/>',
    alert:'<path d="M12 3 2 21h20L12 3ZM12 9v5M12 18h.01"/>',
    screen:'<rect x="2" y="3" width="20" height="14" rx="2"/><path d="M8 21h8M12 17v4"/>',
    backup:'<path d="M4 8h16v13H4zM3 3h18v5H3zM9 12h6"/>',
    pulse:'<path d="M2 12h4l3-8 6 16 3-8h4"/>',
    shield:'<path d="M12 2 3 6v6c0 5 9 10 9 10s9-5 9-10V6Z"/><path d="m8 12 3 3 5-6"/>',
    search:'<circle cx="10" cy="10" r="6"/><path d="m15 15 6 6"/>',
    clock:'<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>',
    close:'<path d="m6 6 12 12M6 18 18 6"/>',
    lock:'<rect x="5" y="10" width="14" height="11" rx="2"/><path d="M8 10V7a4 4 0 0 1 8 0v3M12 14v3"/>',
  };
  const icon = name => `<svg class="icon" viewBox="0 0 24 24" aria-hidden="true">${paths[name] || paths.file}</svg>`;
  const btn = (label, action, attrs = '', style = '') => `<button type="button" class="p3-btn ${style}" data-p3-action="${action}" ${attrs}>${label}</button>`;
  const link = (label, action, attrs = '') => `<button type="button" class="p3-link" data-p3-action="${action}" ${attrs}>${label}</button>`;
  const initialTasks = () => [
    {id:'video',kind:'file',name:'产品演示视频.mp4',peer:'张三',host:'DESKTOP-ZHANG',status:'sending',detail:'742 MB / 1.2 GB · 38.4 MB/s',progress:60,time:'10:42',note:'正在发送，已确认的部分可以继续传输。'},
    {id:'source',kind:'file',name:'设计交付包.zip',peer:'设计工作站',host:'DESIGN-PC',status:'source',detail:'源文件已移动 · 86.2 MB',time:'10:38',note:'原来的文件路径已不可用。重新选择相同文件后，可以从已确认的位置继续。'},
    {id:'offline',kind:'file',name:'季度预算.xlsx',peer:'李四',host:'LISI-LAPTOP',status:'waiting',detail:'对方上线后自动发送 · 2.4 MB',time:'10:35',note:'文件已保存在发送队列中。保持 XChat 运行，对方上线后会自动继续。'},
    {id:'ack',kind:'message',name:'“更新后的方案我发给你了”',peer:'张三',host:'DESKTOP-ZHANG',status:'confirming',detail:'等待送达确认 · 15 秒后自动检查',time:'10:34',note:'消息已经发出，对方可能已经收到。继续确认会沿用原消息，不生成重复聊天记录。'},
    {id:'group',kind:'message',name:'研发协作 · 明天的评审安排',peer:'研发协作',host:'3 位收件人',status:'partial',detail:'2 / 3 已送达 · 王五待上线',time:'10:32',note:'张三和李四已经收到。继续发送时只处理王五，不重复发送给已送达成员。'},
    {id:'done',kind:'file',name:'局域网配置说明.pdf',peer:'张三',host:'DESKTOP-ZHANG',status:'done',detail:'对方已保存 · 4.8 MB',time:'10:28',note:'文件已经校验并保存到对方设备。'},
  ];
  const labels = {sending:'正在发送',source:'需要处理',waiting:'等待上线',confirming:'待确认送达',partial:'部分送达',done:'已完成',cancelled:'已取消',retrying:'正在继续'};
  let enabled = false, page = 'tasks', tasks = initialTasks(), filter = 'all', kind = 'all', query = '', selected = new Set(), detailId = '', epoch = 0;
  let scenario = 'normal', diagnostic = 'issue', diagnosticPeer = 'zhang', backups = [{name:'XChat-2026-10-08.xchatbackup',date:'昨天 18:30',size:'128 MB',contents:'聊天记录、偏好设置'}];
  const initialVoice = () => ({stage:'idle',offered:false,caller:'Eason',startedAt:0,reason:'',muted:{Eason:false,张三:false},deafened:{Eason:false,张三:false},input:{Eason:'default',张三:'default'},output:{Eason:'default',张三:'default'}});
  const initialRemote = () => ({stage:'idle',mode:'control',permission:'view',offered:'view',screen:'1',quality:'自动',fps:'30',color:'full',scale:'fit',reason:'',controlPending:false,owner:'peer',note:'',expanded:false,chatOpen:true,chatDraft:'',demoOpen:false,startedAt:0,generation:0,voice:initialVoice(),chat:[{name:'张三',text:'网络设置这里好像不太对，方便一起看一下吗？'},{name:'Eason',text:'可以，我先看看连接状态。'}]});
  let role = 'viewer', remote = initialRemote(), backupToken = 0, restoreChoice = 'valid', priorFocus;
  let dialogOperation = '';
  const remotePeople = () => remote.owner==='self' ? {host:'Eason',viewer:'张三'} : {host:'张三',viewer:'Eason'};
  const diagTarget = () => diagnosticPeer==='lisi' ? {name:'李四',address:'192.168.1.63:8888'} : {name:'张三',address:'192.168.1.42:8888'};
  const diagResult = () => scenario==='healthy' ? '连接正常' : scenario==='identity' ? '身份不匹配' : '连接超时';
  const root = document.createElement('main'); root.id = 'phase3Workspace'; root.className = 'p3-main'; root.hidden = true; root.setAttribute('data-od-id','phase3-workspace'); byId('app').append(root);
  const dialog = document.createElement('dialog'); dialog.className = 'p3-dialog'; dialog.id = 'phase3Dialog'; dialog.setAttribute('aria-labelledby','p3DialogTitle'); document.body.append(dialog);
  const bar = document.createElement('aside'); bar.className = 'p3-reviewbar'; bar.hidden = true; bar.setAttribute('aria-label','阶段三原型评审工具'); document.body.append(bar);
  const rail = document.createElement('button'); rail.className = 'rail-btn'; rail.type = 'button'; rail.title = '任务中心'; rail.setAttribute('aria-label','任务中心'); rail.dataset.p3Action = 'page'; rail.dataset.page = 'tasks'; rail.innerHTML = icon('tasks'); rail.hidden = true; document.querySelector('.rail nav').append(rail);
  const scenarioSelect = byId('desktopReviewScenario'); scenarioSelect.add(new Option('阶段三 · 任务、诊断与远程协助','phase3'));
  const pages = {tasks:'任务中心',diagnostics:'连接诊断',backup:'备份恢复',remote:'远程协助'};
  const stateColor = s => ['source'].includes(s) ? 'red' : ['sending','retrying','done'].includes(s) ? 'green' : ['partial','confirming'].includes(s) ? 'amber' : 'gray';
  const complete = t => ['done','cancelled'].includes(t.status);
  const after = (ms, callback) => { const expected = epoch; setTimeout(() => { if (enabled && expected === epoch) callback(); }, ms); };

  function renderBar() {
    const options = page === 'diagnostics' ? [['normal','端口不可达'],['healthy','连接正常'],['identity','身份不匹配']]
      : page === 'backup' ? [['normal','正常备份'],['disk','空间不足'],['corrupt','损坏的备份']]
      : page === 'remote' ? [['normal','可用设备'],['offline','对方离线'],['unsupported','对方不支持'],['busy','对方忙碌'],['timeout','请求超时'],['weak','网络较慢'],['mic-denied','麦克风不可用'],['voice-drop','语音中断'],['disconnect','会话断线'],['locked','对方锁屏']]
      : [['normal','典型任务'],['empty','空列表']];
    bar.innerHTML = `<strong>阶段三 · 交互评审</strong>${Object.entries(pages).map(([key,title]) => `<button data-p3-action="page" data-page="${key}" aria-pressed="${page===key}">${title}</button>`).join('')}<span class="p3-spacer"></span><span class="p3-review-label">演示数据 · 不连接设备</span><select id="p3Scenario" aria-label="演示场景">${options.map(([value,label]) => `<option value="${value}" ${scenario===value?'selected':''}>${label}</option>`).join('')}</select>${page==='remote'?remoteRoleSwitch():''}<button class="p3-theme" data-p3-action="theme">浅 / 深</button><button data-p3-action="reset">重置</button><button class="p3-review-exit" data-p3-action="exit">退出评审</button>`;
  }
  function heading(title, subtitle, actions = '') { return `<header class="p3-head"><div><h1 tabindex="-1">${title}</h1><p class="p3-sub">${subtitle}</p></div><div class="p3-actions">${actions}</div></header>`; }
  function closeDialog() {
    if(dialogOperation==='restore') return;
    if(dialogOperation==='backup') { backupToken++; dialogOperation=''; toast('本次备份已取消，未加入备份记录'); }
    dialog.close(); if(priorFocus?.isConnected) priorFocus.focus();
  }
  function modal(title, body, footer = '') {
    if(!dialog.open) priorFocus = document.activeElement;
    dialog.innerHTML = `<header class="p3-dialog-head"><h2 id="p3DialogTitle">${title}</h2><button class="p3-close" data-p3-action="close" aria-label="关闭对话框" ${dialogOperation==='restore'?'disabled':''}>×</button></header><div class="p3-dialog-body">${body}</div><footer class="p3-dialog-foot">${footer || btn('知道了','close')}</footer>`;
    if(!dialog.open) dialog.showModal();
  }
  dialog.addEventListener('cancel', event => { event.preventDefault(); closeDialog(); });
  dialog.addEventListener('keydown', event => { if(event.key==='Escape') { event.preventDefault(); event.stopPropagation(); closeDialog(); } });
  const notice = (title, text, style = '', actions = '') => `<div class="p3-notice ${style}">${icon(style?'alert':'check')}<div><b>${title}</b><p>${text}</p>${actions?`<div class="p3-actions">${actions}</div>`:''}</div></div>`;
  function settingsEntries() {
    if(!enabled || activeTab!=='settings' || byId('p3SettingsLinks')) return;
    const node = document.createElement('div'); node.id = 'p3SettingsLinks'; node.innerHTML = `<div class="p3-minihead">连接与数据</div>${['diagnostics','backup'].map(key => `<button type="button" class="settings-nav-row p3-setting-entry" data-p3-action="page" data-page="${key}">${icon(key==='backup'?'backup':'pulse')}<span>${pages[key]}</span></button>`).join('')}`; byId('list').append(node);
  }
  function enhanceEntrances() {
    settingsEntries();
    let entry = byId('p3RemoteEntry');
    if(!entry) { entry = document.createElement('button'); entry.id='p3RemoteEntry'; entry.className='icon-btn'; entry.type='button'; entry.title='远程协助'; entry.setAttribute('aria-label','远程协助'); entry.dataset.p3Action='page'; entry.dataset.page='remote'; entry.innerHTML=icon('screen'); document.querySelector('.chat-actions').prepend(entry); }
    entry.hidden = !enabled || !!current().group || !!current().ai;
    let tasksEntry = byId('p3FileTasks');
    if(!tasksEntry) { tasksEntry=document.createElement('button'); tasksEntry.id='p3FileTasks'; tasksEntry.className='p3-btn'; tasksEntry.dataset.p3Action='page'; tasksEntry.dataset.page='tasks'; tasksEntry.textContent='任务中心'; byId('refreshFiles').before(tasksEntry); }
    tasksEntry.hidden=!enabled;
  }
  const previousTab = setTab;
  setTab = function(tab) { root.hidden=true; document.body.classList.remove('p3-page','p3-remote-live','p3-remote-expanded'); clearInterval(remoteTimer); rail.classList.remove('active'); byId('settingsListTitle').textContent='设置'; previousTab(tab); if(enabled) { page=''; renderBar(); enhanceEntrances(); } };
  const previousSelect = selectDevice;
  selectDevice = function(id) { previousSelect(id); if(enabled) enhanceEntrances(); };
  const previousSettings = renderSettingsNav;
  renderSettingsNav = function() { previousSettings(); settingsEntries(); };
  function openPage(next, keepScenario = false) {
    if(!pages[next]) return;
    if(!keepScenario && next!==page) scenario='normal';
    setTab(['diagnostics','backup'].includes(next)?'settings':next==='remote'?'devices':'files');
    page=next; document.body.classList.add('p3-page'); app.classList.remove('info-open');
    for(const id of ['chat','fileHub','settingsWorkspace']) byId(id).hidden=true;
    root.hidden=false; rail.classList.toggle('active',page==='tasks');
    if(page==='tasks' || page==='remote') {
      document.querySelectorAll('[data-tab]').forEach(n=>n.classList.remove('active'));
      byId('settingsListTitle').hidden=false; byId('settingsListTitle').textContent=pages[page];
      byId('search').closest('.search-wrap').hidden=true; byId('addBtn').hidden=true;
    }
    render();
  }
  function render() {
    if(!enabled || !page) return;
    renderBar();
    if(page==='tasks') renderTasks(); else if(page==='diagnostics') renderDiagnostics(); else if(page==='backup') renderBackup(); else renderRemote();
    if(['diagnostics','backup'].includes(page)) { document.querySelectorAll('#list .settings-nav-row').forEach(n=>n.classList.toggle('selected',n.dataset.page===page)); }
  }
  function taskMatches(t) { return (kind==='all'||kind===t.kind) && (filter==='all'||filter==='active'&&!complete(t)&&t.status!=='source'||filter==='attention'&&['source','partial','confirming'].includes(t.status)||filter==='done'&&complete(t)) && (!query||(t.name+t.peer).toLowerCase().includes(query.toLowerCase())); }
  function taskActions(t) {
    if(t.status==='source') return btn('重新选择','source',`data-id="${t.id}"`);
    if(t.status==='done') return link('查看会话','conversation');
    if(t.status==='cancelled') return '<span class="p3-sub">已停止</span>';
    if(t.status==='waiting') return link('检查连接','diagnose',`data-id="${t.id}"`)+link('取消','cancel',`data-id="${t.id}"`);
    if(['confirming','partial'].includes(t.status)) return link('继续确认','retry',`data-id="${t.id}"`)+link('取消未送达','cancel',`data-id="${t.id}"`);
    return link('取消','cancel',`data-id="${t.id}"`);
  }
  function renderTasks() {
    const visibleTasks=scenario==='empty'?[]:tasks;
    const live=visibleTasks.filter(t=>!complete(t)), attention=visibleTasks.filter(t=>['source','partial','confirming'].includes(t.status));
    byId('list').innerHTML=`<div class="p3-sidebar-head"><b>每一次发送，都有着落</b><p>汇总来自所有会话的任务</p></div><div class="p3-side-nav">${[['all','全部任务','tasks'],['message','消息','chat'],['file','文件','file']].map(([value,label,type])=>`<button data-p3-action="kind" data-kind="${value}" aria-current="${kind===value?'page':'false'}">${icon(type)}${label}<span>${visibleTasks.filter(t=>value==='all'||t.kind===value).length}</span></button>`).join('')}</div><div class="p3-side-note"><b>任务会被保留</b>退出后重新打开 XChat，可以继续未完成的发送。<br><br>对方离线时会等待上线，不需要反复点击发送。<br><br>${link('打开连接诊断','page','data-page="diagnostics"')}</div>`;
    const rows=scenario==='empty'?[]:tasks.filter(taskMatches);
    root.innerHTML=heading('任务中心','查看发送状态，处理需要你介入的任务。',btn('清理已结束记录','clear-tasks'))+`<div class="p3-metrics"><div class="p3-metric"><b>${live.filter(t=>['sending','retrying'].includes(t.status)).length}</b><span>正在传输</span></div><div class="p3-metric"><b>${live.filter(t=>t.status==='waiting').length}</b><span>等待上线</span></div><div class="p3-metric attention"><b>${attention.length}</b><span>需要关注</span></div><div class="p3-metric"><b>${visibleTasks.filter(t=>t.status==='done').length}</b><span>已完成</span></div></div><div class="p3-filter">${[['all','全部'],['active','进行中'],['attention','需要关注'],['done','已结束']].map(([value,title])=>`<button data-p3-action="filter" data-filter="${value}" aria-pressed="${filter===value}">${title}</button>`).join('')}<label class="p3-search">${icon('search')}<input id="p3Search" aria-label="搜索任务" placeholder="文件、内容或设备" value="${esc(query)}"></label></div><div class="p3-task-layout"><div class="p3-task-table" data-od-id="phase3-task-list"><div class="p3-task-row header"><span></span><span>任务</span><span class="p3-peer-cell">发送给</span><span class="p3-status-cell">当前状态</span><span class="p3-row-actions">操作</span></div>${rows.length?rows.map(t=>`<div class="p3-task-row ${detailId===t.id?'selected':''}" data-task-id="${t.id}"><input type="checkbox" data-task-select="${t.id}" aria-label="选择 ${esc(t.name)}" ${selected.has(t.id)?'checked':''} ${complete(t)?'disabled':''}><div class="p3-task-name"><span class="p3-file-icon">${icon(t.kind==='file'?'file':'chat')}</span><button data-p3-action="detail" data-id="${t.id}"><b>${esc(t.name)}</b><small>${t.kind==='file'?'文件':'消息'} · 今天 ${t.time}</small></button></div><div class="p3-peer-cell">${t.peer}<small>${t.host}</small></div><div class="p3-status-cell"><span class="p3-state ${stateColor(t.status)}">${labels[t.status]}</span>${t.progress&&!complete(t)?`<div class="p3-progress"><span style="--p:${t.progress}%"></span></div>`:''}<div class="p3-state-meta">${t.detail}</div></div><div class="p3-row-actions">${taskActions(t)}</div></div>`).join(''):`<div class="p3-empty">${icon('check')}<b>${query?'没有匹配的任务':'这里暂时没有任务'}</b><p>${query?'换个关键词，或重置筛选。':'消息和文件开始发送后，会在这里显示状态。'}</p>${btn('重置筛选','clear-filter')}</div>`}</div>${detailId&&scenario!=='empty'?taskDetail(tasks.find(t=>t.id===detailId)):''}</div><footer class="p3-task-foot"><span>${selected.size?`已选择 ${selected.size} 项`:'所有进度以对方确认的数据为准'}</span>${selected.size?`<div class="p3-actions">${btn('继续可恢复项','batch-retry')}${btn('取消未完成项','batch-cancel','','danger')}</div>`:`<span class="p3-sub" style="margin-left:auto">${rows.length} 项任务</span>`}</footer>`;
  }
  function taskDetail(t) {
    if(!t) return '';
    return `<aside class="p3-task-detail" aria-label="任务详情"><div class="p3-detail-top"><h2>任务详情</h2><button class="p3-close" data-p3-action="detail-close" aria-label="关闭任务详情">×</button></div><span class="p3-state ${stateColor(t.status)}">${labels[t.status]}</span><h3>${esc(t.name)}</h3><p class="p3-sub">${t.note}</p><dl class="p3-key-values"><dt>发送给</dt><dd>${t.peer}</dd><dt>创建时间</dt><dd>今天 ${t.time}</dd><dt>自动继续</dt><dd>${complete(t)?'无需继续':t.status==='source'?'等待选择源文件':'网络恢复后自动尝试'}</dd></dl>${t.id==='group'?'<h3>收件人进度</h3><div class="p3-permission">张三<span>已送达</span></div><div class="p3-permission">李四<span>已送达</span></div><div class="p3-permission">王五<span>'+ (t.status==='done'?'已送达':t.status==='cancelled'?'已取消':'等待上线') +'</span></div>':''}<h3>处理记录</h3><ol class="p3-timeline"><li>已保存到本机队列<small>${t.time} · 任务已受理</small></li><li>核对设备并开始发送<small>${t.time} · 仅发送给原收件人</small></li><li>${labels[t.status]}<small>${t.detail}</small></li></ol><div class="p3-actions">${taskActions(t)}</div><hr style="border:0;border-top:1px solid var(--border);margin:24px 0">${link('诊断此设备的连接','diagnose',`data-id="${t.id}"`)}</aside>`;
  }
  function retryTask(t) {
    if(!t || complete(t) || ['sending','retrying','source','waiting'].includes(t.status)) return false;
    t.status='retrying'; t.detail=t.id==='group'?'仅继续处理未送达的王五':'正在核对原消息的送达状态'; render();
    after(1700,()=>{if(t.status!=='retrying')return;t.status='done';t.detail=t.id==='group'?'3 / 3 已送达':'对方已确认送达';t.note='原任务已完成，没有生成重复消息。';selected.delete(t.id);render();});return true;
  }
  function cancelTasks(ids) {
    const cancellable=tasks.filter(t=>ids.includes(t.id)&&!complete(t)); if(!cancellable.length)return;
    modal(`取消 ${cancellable.length} 个未完成任务？`,'<p>将停止这些任务后续的发送。已经送达的消息或文件不会被撤回，聊天记录会保留。</p><p>如果消息正在等待确认，对方可能已经收到。</p>',btn('保留任务','close')+btn('确认取消','cancel-confirm',`data-ids="${cancellable.map(t=>t.id).join(',')}"`,'danger-fill'));
  }
  function sourceDialog(id) {
    modal('重新选择源文件','<p>选择与原任务相同的文件。内容一致时，已确认的部分可以继续使用。</p><label class="p3-option"><input name="p3Source" type="radio" value="same" checked><span>设计交付包.zip<small>D:\\交付归档 · 86.2 MB · 与原文件一致</small></span></label><label class="p3-option"><input name="p3Source" type="radio" value="changed"><span>设计交付包-修改版.zip<small>D:\\交付归档 · 91.6 MB · 内容已变化</small></span></label><div id="p3SourceError" role="alert"></div>',btn('暂不处理','close')+btn('检查并继续','source-confirm',`data-id="${id}"`,'primary'));
  }

  function renderDiagnostics() {
    const busy=diagnostic==='checking', healthy=scenario==='healthy', identity=scenario==='identity', peer=diagTarget();
    const title=busy?'正在检查连接…':healthy?'连接正常，可以继续发送':identity?`当前地址上的设备与${peer.name}不一致`:'已找到设备，但暂时连接不上';
    const explain=busy?'依次检查本机服务、网络接口、对方身份与消息连接。':healthy?`已确认连接属于${peer.name}，消息和文件服务均可用。`:identity?'已停止向这个地址发送，避免内容交给错误的设备。请核对对方的新地址。':'本机服务正常；连接对方的消息端口时超时。请先确认对方 XChat 正在运行。';
    const checks=[['本机服务','消息与文件服务已就绪 · 数据保存正常','通过',true],['当前网络','以太网已启用 · VPN 接口按设置参与发现','通过',true],['设备身份',identity?'地址回应来自另一台设备':healthy?`当前地址已确认属于${peer.name}`:'已找到候选地址，连接成功后核对身份',identity?'不一致':healthy?'通过':'待确认',healthy],['消息连接',healthy?'连接与送达确认正常':identity?'身份不一致，已停止连接':`${peer.address} · 连接超时`,healthy?'通过':identity?'已停止':'未通过',healthy],['协议能力',healthy?'消息、文件续传可用 · 远程协助需单独同意':'建立可信连接后再检查可用功能',healthy?'通过':'未检查',healthy]];
    root.innerHTML=heading('连接诊断','解释“看得见却发不出”，并找到下一步。',btn('导出诊断','export',busy?'disabled':'')+btn(busy?'检查中…':'重新检查','diag-run',busy?'disabled':'','primary'))+`<div class="p3-body"><div class="p3-inline-select" style="margin-bottom:20px"><label for="p3DiagPeer">诊断对象</label><select class="p3-select" id="p3DiagPeer"><option value="zhang" ${diagnosticPeer==='zhang'?'selected':''}>张三 · DESKTOP-ZHANG</option><option value="lisi" ${diagnosticPeer==='lisi'?'selected':''}>李四 · LISI-LAPTOP</option></select><span class="p3-sub">本次检查 · 刚刚</span></div>${notice(title,explain,healthy?'':identity?'danger':'warn',!busy&&!healthy?btn('打开网络设置','network-settings'):'')}<div class="p3-two-col"><section class="p3-slab" data-od-id="phase3-diagnostic-checks"><div class="p3-slab-head"><span>检查结果</span><span class="p3-sub">${busy?'正在检查':healthy?'5 项通过':identity?'身份验证未通过':'2 项通过 · 1 项异常'}</span></div>${checks.map(([name,text,result,passed],index)=>`<div class="p3-check ${!passed?(index===3||identity&&index===2?'failed':'waiting'):''}">${busy?'<span class="p3-loading"></span>':icon(passed?'check':result==='未检查'||result==='待确认'?'clock':'alert')}<div><strong>${name}</strong><p>${text}</p></div><span>${busy?'检查中':result}</span></div>`).join('')}</section><aside class="p3-diag-side"><h2>建议按这个顺序处理</h2><ol class="p3-note-list"><li>确认对方设备已开机，并且 XChat 正在运行。</li><li>核对你们使用的网络或已保存的固定地址。</li><li>请对方检查是否允许 XChat 通过防火墙。</li></ol><p class="p3-sub">检查不会更改网络或防火墙设置。</p><div style="border-top:1px solid var(--border);margin-top:24px;padding-top:18px">${link('查看等待中的任务','page','data-page="tasks"')}</div></aside></div><details class="p3-technical"><summary>查看供排查使用的详细信息</summary><pre>本机服务：ready\n设备身份：${healthy?'已确认':identity?'不一致':'等待核对'}\n最后连接结果：${healthy?'成功':'未建立'}\n发送队列：任务仍保留\n诊断记录不包含消息正文或文件内容</pre></details></div>`;
  }
  function exportDialog() {
    modal('导出连接诊断','<p>包含服务状态、连接结果和错误时间。默认隐藏网络地址、设备标识及文件路径，不包含消息正文和文件内容。</p><label class="p3-check-label"><input id="p3IncludeAddresses" type="checkbox"><span>附带完整网络地址<small>仅在你需要让协助者核对具体地址时勾选。</small></span></label><pre class="p3-code" id="p3ExportPreview">设备：设备 A\n地址：192.168.*.*\n连接：超时\n消息正文：不包含\n文件内容：不包含\n来源：交互原型演示数据</pre>',btn('取消','close')+btn('下载演示诊断','export-save','','primary'));
    updateExportPreview();
  }
  function updateExportPreview() { byId('p3ExportPreview').textContent=`设备：设备 A\n地址：${byId('p3IncludeAddresses').checked?diagTarget().address:'192.168.*.*'}\n连接：${diagResult()}\n消息正文：不包含\n文件内容：不包含\n来源：交互原型演示数据`; }

  function renderBackup() {
    root.innerHTML=heading('备份与恢复','把聊天记录和偏好保存在自己手里。',btn('从备份恢复','restore'))+`<div class="p3-body"><section class="p3-backup-hero"><div><span class="p3-state green">本地备份</span><h2 style="font-size:19px;margin:15px 0 0">给重要记录留一份副本</h2><p class="p3-sub">选择要保存的内容，完成后检查备份是否完整。<br>本机可用空间 <span class="p3-mono">${scenario==='disk'?'80 MB':'186 GB'}</span> · 最近备份 ${backups.length>1?'刚刚':'昨天 18:30'}</p>${btn(icon('backup')+'创建备份','backup-create','','primary')}</div><div class="p3-hero-icon">${icon('backup')}</div></section><div class="p3-actions" style="justify-content:space-between;margin-bottom:10px"><h2 style="margin:0">备份记录</h2><span class="p3-sub">保存在本机 · ${backups.length} 份</span></div>${backups.map((b,i)=>`<div class="p3-backup-row">${icon('backup')}<div><b>${b.name}</b><small>${b.contents} · 已完成完整性校验</small></div><span class="p3-sub p3-backup-date">${b.date}</span><span class="p3-sub p3-mono">${b.size}</span>${btn('恢复预览','restore',`data-backup="${i}"`)}</div>`).join('')}<div class="p3-backup-note"><section><h3>哪些内容可以备份？</h3><p>聊天记录、联系人备注和偏好设置。已下载的附件可单独选择；未下载的文件不会凭空出现在备份中。</p></section><section><h3>恢复前先确认</h3><p>先校验，再预览新增记录。已有记录保留，重复项跳过；历史待发消息不会因恢复而重新发送。</p></section></div></div>`;
  }
  function backupCreate() {
    modal('创建本地备份','<p>选择本次需要保存的内容。</p><label class="p3-check-label"><input type="checkbox" checked disabled><span>聊天记录与联系人备注<small>12,432 条记录 · 约 124 MB</small></span></label><label class="p3-check-label"><input type="checkbox" id="p3BackupSettings" checked><span>偏好设置<small>不包含设备身份和远程授权。</small></span></label><label class="p3-check-label"><input type="checkbox" id="p3BackupAttachments"><span>本地已下载的附件<small>186 个文件 · 额外约 2.4 GB</small></span></label><label class="p3-field"><span>保存位置</span><select id="p3BackupLocation"><option>D:\\XChat 备份</option><option>E:\\移动硬盘\\XChat</option></select></label><p class="p3-sub">备份可能包含私人聊天内容，请妥善保管。</p>',btn('取消','close')+btn('开始备份','backup-start','','primary'));
  }
  function runBackup() {
    const attachments=byId('p3BackupAttachments')?.checked, settings=byId('p3BackupSettings')?.checked, token=++backupToken;
    if(scenario==='disk') { modal('保存位置空间不足',`<p>预计需要 ${attachments?'2.5 GB':'128 MB'}，当前演示位置仅剩 80 MB。本次尚未创建备份，聊天记录未受影响。</p>`,btn('取消','close')+btn('更换保存位置','backup-reselect','','primary')); return; }
    dialogOperation='backup';
    modal('正在创建备份','<p id="p3BackupStep" role="status">正在保存一致的聊天记录副本…</p><div class="p3-progress"><span id="p3BackupProgress" style="--p:18%"></span></div><p>备份完成并通过校验后，才会出现在备份记录中。</p>',btn('取消本次备份','backup-cancel'));
    after(800,()=>{if(token!==backupToken)return;byId('p3BackupStep')&&(byId('p3BackupStep').textContent='正在校验备份完整性…');byId('p3BackupProgress')?.style.setProperty('--p','78%');});
    after(1900,()=>{if(token!==backupToken)return;dialogOperation='';backups.unshift({name:'XChat-2026-10-09.xchatbackup',date:'刚刚',size:attachments?'2.5 GB':'128 MB',contents:'聊天记录'+(settings?'、偏好设置':'')+(attachments?'、本地附件':'')});render();modal('备份已完成',notice('已通过完整性校验','XChat-2026-10-09.xchatbackup')+'<p>演示备份已加入列表。你可以继续预览恢复过程。</p>',btn('返回备份列表','close')+btn('预览恢复','restore','','primary'));});
  }
  function restoreStart() {
    restoreChoice=scenario==='corrupt'?'corrupt':'valid';
    modal('选择要恢复的备份','<div class="p3-steps"><span class="current">1 选择备份</span><span>2 校验与预览</span><span>3 完成</span></div><p>这里用两份演示文件展示成功与校验失败的流程。</p>'+[['valid','XChat-2026-10-08.xchatbackup','128 MB · 标准备份'],['corrupt','XChat-损坏示例.xchatbackup','64 MB · 文件不完整']].map(([value,name,desc])=>`<label class="p3-option"><input type="radio" name="p3RestoreSource" value="${value}" ${value===restoreChoice?'checked':''}><span>${name}<small>${desc}</small></span></label>`).join(''),btn('取消','close')+btn('校验备份','restore-check','','primary'));
  }
  function restorePreview() {
    restoreChoice=dialog.querySelector('[name=p3RestoreSource]:checked')?.value || restoreChoice;
    if(restoreChoice==='corrupt') { modal('备份未通过校验',notice('文件不完整，无法恢复','当前聊天记录没有变化。请使用其他备份，或重新获取完整文件。','danger'),btn('取消','close')+btn('重新选择备份','restore','','primary'));return; }
    modal('确认恢复范围','<div class="p3-steps"><span>1 选择备份</span><span class="current">2 校验与预览</span><span>3 完成</span></div>'+notice('备份完整，可以恢复','创建于昨天 18:30 · 来自本机 XChat')+'<dl class="p3-key-values"><dt>新增聊天记录</dt><dd>128 条</dd><dt>已存在的记录</dt><dd>12,304 条 · 跳过重复</dd><dt>当前记录</dt><dd>全部保留</dd></dl><label class="p3-check-label"><input id="p3RestoreSettings" type="checkbox"><span>同时恢复偏好设置<small>默认保留当前设置；设备身份、快捷键和远程授权不迁移。</small></span></label><p>恢复前会自动保留当前数据副本。恢复的历史不会自动重发，未完成任务需重新检查。</p>',btn('返回','restore')+btn('确认合并恢复','restore-run','','primary'));
  }
  function runRestore() {
    const settings=byId('p3RestoreSettings').checked;
    dialogOperation='restore';
    modal('正在恢复记录','<p role="status">正在保留当前数据副本并合并记录…</p><div class="p3-progress"><span style="--p:60%"></span></div><p>请稍候。重复记录会跳过，当前记录不会删除。</p>',btn('处理中','close','disabled'));
    after(1400,()=>{dialogOperation='';modal('恢复完成',notice('已新增 128 条记录','12,304 条重复记录已跳过，原有记录均保留。')+`<p>${settings?'已恢复偏好设置，保留本机设备身份、快捷键与远程授权。':'当前偏好设置保持不变。'}恢复前的数据副本已保留。历史消息不会自动重新发送。</p>`,btn('完成','close','','primary'));});
  }

  const raPaths = {
    help:'<path d="M8 7H4v12h16V7h-4M9 22h6M12 19v3"/><path d="M12 2v11m-4-4 4 4 4-4"/>',
    control:'<rect x="2" y="3" width="20" height="14" rx="2"/><path d="M7 21h5M10 17v4m4-11 7 4-3 1-1 3-3-8Z"/>',
    expand:'<path d="M8 3H3v5m13-5h5v5M3 16v5h5m13-5v5h-5"/>',
    shrink:'<path d="M3 8h5V3m8 0v5h5M8 21v-5H3m13 5v-5h5"/>',
    back:'<path d="m14 6-6 6 6 6"/>',
    arrow:'<path d="M4 12h16m-5-5 5 5-5 5"/>',
    pause:'<path d="M8 5v14M16 5v14"/>',
    play:'<path d="m8 4 12 8-12 8Z"/>',
    tune:'<path d="M4 6h16M4 12h16M4 18h16M8 3v6m8 0v6m-6 0v6"/>',
    send:'<path d="m3 3 18 9-18 9 4-9-4-9Zm4 9h14"/>',
    mic:'<rect x="9" y="2" width="6" height="13" rx="3"/><path d="M5 10v2a7 7 0 0 0 14 0v-2M12 19v3m-4 0h8"/>',
    micOff:'<path d="m3 3 18 18M9 9v3a3 3 0 0 0 5 2M9 5a3 3 0 0 1 6 0v6M5 10v2a7 7 0 0 0 12 5M19 10v2M12 19v3m-4 0h8"/>',
    speaker:'<path d="M3 9h4l5-5v16l-5-5H3Z M16 8a6 6 0 0 1 0 8m3-11a10 10 0 0 1 0 14"/>',
    speakerOff:'<path d="M3 9h4l5-5v16l-5-5H3Zm13 0 6 6m0-6-6 6"/>',
    phone:'<path d="M7 3H3v3c0 8 7 15 15 15h3v-4l-5-2-2 2a16 16 0 0 1-7-7l2-2Z"/>',
    hangup:'<path d="M3 15v-4a17 17 0 0 1 18 0v4h-5v-4a14 14 0 0 0-8 0v4Z"/>',
    monitor:paths.screen, shield:paths.shield, chat:paths.chat, lock:paths.lock, close:paths.close, check:paths.check,
  };
  const raIcon = name => `<svg class="ra-icon" viewBox="0 0 24 24" aria-hidden="true">${raPaths[name]||paths.screen}</svg>`;
  const raButton = (label, action, glyph, extra='', style='') => btn(raIcon(glyph)+`<span>${label}</span>`,action,extra,`ra-tool ${style}`);
  const remoteActor = () => role==='host' ? remotePeople().host : remotePeople().viewer;
  const remoteRecipient = () => remote.mode==='help' ? role==='viewer' : role==='host';
  const remoteModeLabel = () => remote.mode==='help' ? '请求对方协助' : '请求控制对方';
  let remoteTimer;
  function remoteElapsed() { const seconds=Math.max(0,Math.floor((Date.now()-remote.startedAt)/1000));return `${String(Math.floor(seconds/60)).padStart(2,'0')}:${String(seconds%60).padStart(2,'0')}`; }
  function syncRemoteChrome() {
    const live=enabled&&page==='remote'&&['active','paused'].includes(remote.stage);
    document.body.classList.toggle('p3-remote-live',live);
    document.body.classList.toggle('p3-remote-expanded',live&&remote.expanded);
    clearInterval(remoteTimer);
    if(live)remoteTimer=setInterval(()=>{document.querySelectorAll('[data-ra-elapsed]').forEach(el=>el.textContent=remoteElapsed());document.querySelectorAll('[data-ra-voice-elapsed]').forEach(el=>el.textContent=voiceElapsed());},1000);
  }
  function remoteRoleSwitch() {
    const idle=remote.stage==='idle';
    return `<button data-p3-action="role" ${idle?'disabled':''}>${idle?'双方视角':remoteActor()==='Eason'?'切到张三视角':'切回我的视角'}</button>`;
  }
  function remoteSidebar() {
    byId('list').innerHTML=`<div class="ra-side-head"><span>当前会话</span>${link('返回聊天','conversation')}</div><div class="ra-contact"><span class="ra-avatar">张</span><div><strong>张三</strong><small><i class="ra-online ${scenario==='offline'?'offline':''}"></i>${scenario==='offline'?'离线':'在线'} · 研发团队</small></div></div><div class="ra-side-device">${raIcon('monitor')}<div>DESKTOP-ZHANG<small>Windows · 局域网设备</small></div></div><div class="ra-side-conversation"><time>今天 10:42</time><div class="ra-message"><b>张三</b><p>网络设置这里好像不太对，方便一起看一下吗？</p></div><div class="ra-message mine"><b>我</b><p>可以，我们用远程协助。</p></div><div class="ra-context-note">${raIcon('shield')}协助只在本次会话内有效<br>双方都可以随时结束</div></div>`;
  }
  function remoteModeCard(mode) {
    const help=mode==='help', unavailable=['offline','unsupported','busy'].includes(scenario);
    return `<button type="button" class="ra-mode-card" data-p3-action="remote-start" data-mode="${mode}" ${unavailable?'disabled':''}><span class="ra-mode-visual ${help?'help':'control'}"><span class="ra-mini-device">${raIcon('monitor')}<small>${help?'我的电脑':'对方电脑'}</small></span><span class="ra-mode-arrow">${raIcon(help?'help':'control')}</span><span class="ra-mini-person">${help?'张':'我'}<small>${help?'张三协助我':'我来协助'}</small></span></span><span class="ra-mode-eyebrow">${help?'我的电脑需要帮助':'我来解决对方的问题'}</span><strong>${help?'请求对方协助':'请求控制对方'}</strong><span class="ra-mode-copy">${help?'把我的屏幕共享给张三，由我决定是否允许他操作。':'申请查看和操作张三的电脑，等待他选择屏幕并同意。'}</span><span class="ra-mode-cta">${help?'邀请张三帮我':'请求控制张三的电脑'}${raIcon('arrow')}</span></button>`;
  }
  function renderRemote() {
    syncRemoteChrome();remoteSidebar();
    if(['active','paused'].includes(remote.stage)){renderRemoteSession();return;}
    const unavailable={offline:['张三暂时离线','对方上线后可发起协助。可以先在聊天中说明问题。'],unsupported:['对方版本暂不支持远程协助','你们仍可正常聊天和传文件。'],busy:['张三正在另一场协助中','对方结束当前协助后，你可以再发起请求。']};
    root.innerHTML=heading('远程协助','与张三一起，解决电脑上的问题。',`<span class="ra-capability">${raIcon('monitor')}Windows 桌面</span>`)+`<div class="ra-lobby">${remote.stage==='idle'?`<div class="ra-lobby-intro"><span class="ra-kicker">从当前会话开始</span><h2>哪一台电脑需要帮助？</h2><p>选择需要帮助的电脑，接通后可双向语音协作。</p></div>${unavailable[scenario]?notice(...unavailable[scenario],'warn',btn('回到聊天','conversation')+btn('检查连接','diagnose')):''}<div class="ra-mode-grid">${remoteModeCard('help')}${remoteModeCard('control')}</div><div class="ra-lobby-foot">${raIcon('shield')}共享哪块屏幕、是否允许操作，都由电脑主人决定。<span>可随时暂停或结束</span></div>`:remoteRequestCard()}</div>`;
  }
  function remoteRequestCard() {
    const people=remotePeople(),recipient=remoteRecipient(),mode=remoteModeLabel();
    let title='',copy='',actions='',status='等待回应',glyph=remote.mode==='help'?'help':'control';
    if(remote.stage==='waiting') {
      title=recipient?(remote.mode==='help'?'Eason 请求你远程协助':'Eason 请求控制你的电脑'):(remote.mode==='help'?'正在邀请张三协助我':'正在请求控制张三的电脑');
      copy=recipient?(remote.mode==='help'?`你将查看 Eason 选择的屏幕${remote.offered==='control'?'，他已允许你在本次会话中操作鼠标键盘':'，鼠标键盘暂未授权'}。`:'由你选择要共享的屏幕，并决定允许控制、仅查看或拒绝。'):'请求已发送。对方同意前，不会共享任何画面或开放操作权限。';
      actions=recipient?btn('拒绝','remote-reject')+btn('查看请求','remote-consent','','primary'):btn('取消请求','remote-cancel');
    } else if(remote.stage==='connecting') {title='对方已同意，正在建立连接';copy='连接完成后显示共享画面，聊天仍可继续使用。';status='正在连接';glyph='monitor';actions=btn('取消连接','remote-cancel');}
    else {
      const states={rejected:['对方暂时没有同意','本次请求已结束，没有共享屏幕或授予控制。','已拒绝'],cancelled:['请求已取消','本次请求不再等待回应，可以重新发起。','已取消'],expired:['对方暂未回应','请求已过期。先在聊天中确认对方是否方便，再发起一次。','已超时'],ended:['远程协助已结束','屏幕共享已停止，鼠标键盘权限已收回。','已结束'],disconnected:[remote.reason==='locked'?'共享的电脑已锁屏':'远程连接已中断','画面已隐藏，控制已停止。重新连接需要电脑主人再次同意。','已中断']};
      [title,copy,status]=states[remote.stage]||states.ended;glyph=remote.stage==='disconnected'?'lock':'monitor';
      actions=btn('返回选择','remote-home')+btn(remote.mode==='help'?'重新邀请对方协助':'重新请求控制对方','remote-retry','','primary');
    }
    return `<section class="ra-request-card"><div class="ra-request-top"><span class="ra-request-glyph">${raIcon(glyph)}</span><span class="ra-request-state ${remote.stage==='connecting'?'connecting':''}">${status}</span></div><span class="ra-kicker">${mode} · ${remoteActor()==='Eason'?'我的视角':'张三的视角'}</span><h2>${title}</h2><p>${copy}</p><div class="ra-request-route"><div><span class="ra-avatar small">${people.viewer==='Eason'?'我':'张'}</span><strong>${people.viewer}</strong><small>协助 / 操作方</small></div>${raIcon('arrow')}<div><span class="ra-avatar small muted">${people.host==='Eason'?'我':'张'}</span><strong>${people.host}的电脑</strong><small>屏幕共享方</small></div></div>${remote.note?`<blockquote><span>协助说明</span>${esc(remote.note)}</blockquote>`:''}<div class="ra-request-actions">${actions}</div><p class="ra-request-hint">${['waiting','connecting'].includes(remote.stage)?'聊天消息不受影响，你可以继续和对方沟通。':'再次发起时，会重新确认本次共享和控制权限。'}</p></section>`;
  }
  function prepareRemote(mode) {
    if(['offline','unsupported','busy'].includes(scenario))return;
    const help=mode==='help';
    modal(help?'请求张三远程协助':'请求控制张三的电脑',`<div class="ra-dialog-person"><span class="ra-avatar">张</span><div><b>张三</b><small>DESKTOP-ZHANG · 在线</small></div><span class="ra-dialog-direction">${help?'对方协助我':'我协助对方'}</span></div><p>${help?'选择要共享给张三的屏幕。对方接受后，才会开始共享。':'发送请求后，张三需要选择共享屏幕并明确允许本次控制。'}</p>${help?screenOptions()+`<label class="ra-grant-choice"><input type="checkbox" id="raOfferControl"><span><b>同时允许对方操作我的鼠标和键盘</b><small>仅本次会话；不勾选时只共享画面，之后仍可单独授权。</small></span></label>`:`<div class="ra-scope"><span>${raIcon('monitor')}查看对方共享的屏幕</span><span>${raIcon('control')}请求鼠标与键盘控制</span></div>`}${voiceChoice()}<label class="p3-field ra-note"><span>协助说明 <small>选填</small></span><textarea id="raRequestNote" maxlength="200" rows="2" placeholder="例如：帮我检查一下网络设置">${esc(remote.note)}</textarea></label>`,btn('取消','close')+btn(help?'发送协助邀请':'发送控制请求','remote-send',`data-mode="${mode}"`,'primary'));
  }
  function sendRemote(mode) {
    if(['offline','unsupported','busy'].includes(scenario))return;
    const next=initialRemote();next.mode=mode;next.owner=mode==='help'?'self':'peer';next.stage='waiting';
    next.screen=dialog.querySelector('[name=p3Screen]:checked')?.value||'1';next.offered=mode==='help'&&byId('raOfferControl')?.checked?'control':'view';next.note=byId('raRequestNote').value.trim();next.voice.offered=!!byId('raOfferVoice')?.checked;
    remote=next;role=mode==='help'?'host':'viewer';closeDialog();render();
  }
  function incomingRemoteConsent() {
    if(remote.stage!=='waiting'||!remoteRecipient())return;
    const help=remote.mode==='help';
    modal(help?'Eason 请求你远程协助':'Eason 请求控制你的电脑',`<div class="ra-dialog-person"><span class="ra-avatar">E</span><div><b>Eason</b><small>本次会话的请求</small></div></div>${remote.note?`<blockquote class="ra-note-quote">${esc(remote.note)}</blockquote>`:''}<p>${help?'接受后，你将看到 Eason 选定的屏幕。':'先选择要共享的屏幕，再决定本次允许的权限。'}</p>${help?`<div class="ra-scope"><span>${raIcon('monitor')}Eason 的屏幕 ${remote.screen}</span><span>${raIcon('control')}${remote.offered==='control'?'Eason 已明确允许本次鼠标键盘控制':'仅查看，鼠标键盘未授权'}</span></div>`:screenOptions()+`<p class="ra-consent-copy">“允许本次控制”包含所选屏幕的查看与鼠标键盘操作。你可随时收回控制，不共享剪贴板或文件。</p>`}${remote.voice.offered?voiceChoice(true):'<p class="p3-sub">本次未附带语音，可在协助中随时发起通话。</p>'}`,btn('拒绝','remote-reject')+(help?btn('接受协助','remote-accept',`data-permission="${remote.offered}"`,'primary'):btn('仅允许查看','remote-accept','data-permission="view"')+btn('允许本次控制','remote-accept','data-permission="control"','primary')));
  }
  function acceptRemote(permission) {
    if(remote.stage!=='waiting'||!remoteRecipient())return;
    if(remote.mode==='control')remote.screen=dialog.querySelector('[name=p3Screen]:checked')?.value||remote.screen;
    const current=remote,version=++remote.generation,joinVoice=remote.voice.offered&&!!byId('raJoinVoice')?.checked,voiceActor=remoteActor();
    remote.stage='connecting';remote.permission='view';closeDialog();render();
    after(650,()=>{if(remote!==current||remote.generation!==version||remote.stage!=='connecting')return;remote.permission=permission;remote.stage='active';remote.startedAt=Date.now();if(joinVoice)connectVoice(voiceActor);render();});
  }
  function stopRemote(stage,reason='') {remote.generation++;remote.stage=stage;remote.reason=reason;remote.permission='view';remote.offered='view';remote.controlPending=false;remote.expanded=false;remote.voice=initialVoice();closeDialog();render();}
  function screenOptions() {return `<div class="ra-screen-options">${[['1','主显示器','1920 × 1080'],['2','扩展显示器','1080 × 1920']].map(([value,name,size])=>`<label class="ra-screen-choice"><input type="radio" name="p3Screen" value="${value}" ${remote.screen===value?'checked':''}><span class="ra-screen-preview ${value==='2'?'portrait':''}"><span></span><b>${value}</b></span><strong>屏幕 ${value} · ${name}</strong><small>${size}</small></label>`).join('')}</div>`;}
  function switchRemoteScreen() {if(role!=='host'||!['active','paused'].includes(remote.stage))return;modal('切换共享屏幕',`<p>由你选择接下来共享的屏幕。切换后收回控制，继续保持仅查看。</p>${screenOptions()}`,btn('取消','close')+btn('共享所选屏幕','remote-screen-confirm','','primary'));}
  function controlConsent() {
    if(role!=='host'||remote.stage!=='active')return;
    modal(`允许 ${remotePeople().viewer} 控制你的电脑？`,`<div class="ra-scope"><span>${raIcon('monitor')}屏幕 ${remote.screen} · 本次会话</span><span>${raIcon('control')}鼠标与键盘</span></div><p>你可以随时在共享提示条中收回控制。暂停、切换屏幕、断线或锁屏后，控制权限自动失效。</p><p class="p3-sub">剪贴板和文件不包含在这次授权中。</p>`,btn('保持仅查看','remote-deny-control')+btn('允许本次控制','remote-allow-control','','primary'));
  }
  function remoteQuality() {
    modal('画面质量',`<p>网络较慢时降低画面细节或刷新频率，让操作更及时。</p><div class="ra-quality-options">${[['自动','随网络变化调整画面，优先保证操作响应'],['优先流畅','减少画面细节，适合网络不稳定时排查问题'],['优先清晰','保留更多文字细节，适合查看文档']].map(([value,desc])=>`<label class="p3-option"><input type="radio" name="raQuality" value="${value}" ${remote.quality===value?'checked':''}><span>${value}<small>${desc}</small></span></label>`).join('')}</div><details class="ra-advanced"><summary>更多显示选项</summary><label class="p3-field"><span>画面刷新上限</span><select id="raFrameLimit">${['10','20','30'].map(n=>`<option value="${n}" ${remote.fps===n?'selected':''}>${n} 帧 / 秒</option>`).join('')}</select></label><label class="p3-field"><span>色彩</span><select id="raColor"><option value="full" ${remote.color==='full'?'selected':''}>完整色彩</option><option value="reduced" ${remote.color==='reduced'?'selected':''}>减少色彩，降低带宽占用</option></select></label></details>`,btn('取消','close')+btn('应用','remote-quality-save','','primary'));
  }
  function remoteDesktop() {
    const controllable=role==='viewer'&&remote.permission==='control'&&remote.stage==='active';
    return `<div class="ra-desktop ${remote.screen==='2'?'portrait':''} ${remote.scale==='actual'?'actual':''} ${remote.color==='reduced'?'reduced':''}" data-od-id="phase3-remote-screen"><div class="ra-desktop-icons"><span>${raIcon('monitor')}此电脑</span><span>${paths.file?icon('file'):''}工作文件</span></div><div class="ra-os-window"><div class="ra-os-title">${raIcon('tune')}设置<span>—　□　×</span></div><div class="ra-os-layout"><aside><div class="ra-os-account"><span>${remotePeople().host.slice(0,1)}</span><b>${remotePeople().host}<small>本地账户</small></b></div><div class="ra-os-search">查找设置</div><p>系统</p><p>蓝牙和设备</p><p class="selected">网络和 Internet</p><p>个性化</p><p>应用</p></aside><section><h2>网络和 Internet</h2><div class="ra-os-network">${raIcon('monitor')}<div><strong>以太网</strong><small>已连接 · 专用网络</small></div></div><div class="ra-os-setting"><b>网络属性</b><small>专用网络　·　已连接</small><span>›</span></div><div class="ra-os-setting"><b>高级网络设置</b><small>网络适配器与连接属性</small><button data-p3-action="remote-demo-open" ${controllable?'':'disabled'} aria-label="在示意桌面中展开网络设置">${remote.demoOpen?'收起':'展开'} ›</button></div>${remote.demoOpen?`<div class="ra-os-adapter"><span class="ra-online"></span>以太网适配器 <small>连接正常　1.0 Gbps</small></div>`:''}<p class="ra-os-assist-note">与 ${remotePeople().viewer} 一起检查当前网络连接</p></section></div></div><div class="ra-os-taskbar"><span class="ra-windows-mark">▦</span><span class="ra-task-search">搜索</span><span>▣　▤　◉</span><small>10:48<br>2026/10/10</small></div><span class="ra-desktop-watermark">示意桌面 · 非真实画面</span>${controllable?`<span class="ra-remote-cursor" aria-hidden="true">➤<small>${remotePeople().viewer}</small></span>`:''}</div>`;
  }
  function remoteChatPanel() {return `<aside class="ra-chat-panel"><div class="ra-chat-head"><h3>会话沟通</h3>${btn('×','remote-chat-toggle','aria-label="收起会话沟通"','ra-chat-close')}</div><div class="ra-chat-note">${raIcon('chat')}协助时，继续把问题说清楚</div><div class="ra-chat-messages">${remote.chat.map(message=>`<div class="ra-chat-message ${message.name===remoteActor()?'mine':''}"><span>${message.name===remoteActor()?'我':message.name}</span><p>${esc(message.text)}</p></div>`).join('')}</div><div class="ra-chat-compose"><textarea id="raChatDraft" rows="3" maxlength="300" aria-label="协助消息" placeholder="说一下你看到的问题…">${esc(remote.chatDraft)}</textarea><div><span>Enter 发送</span>${raButton('发送','remote-chat-send','send','','primary')}</div></div></aside>`;}
  const voicePeer = () => remoteActor()==='Eason'?'张三':'Eason';
  function voiceElapsed() {const seconds=Math.max(0,Math.floor((Date.now()-remote.voice.startedAt)/1000));return `${String(Math.floor(seconds/60)).padStart(2,'0')}:${String(seconds%60).padStart(2,'0')}`;}
  function voiceChoice(incoming=false) {
    return `<label class="ra-voice-choice"><input type="checkbox" id="${incoming?'raJoinVoice':'raOfferVoice'}" checked><span>${raIcon('phone')}<b>${incoming?'同时接通双向语音':'同时发起语音通话'}</b><small>${incoming?'接受协助后开启麦克风和扬声器；取消勾选可只进行远程协助。':'双方同意后边说边协作；可随时静音或单独挂断语音。'}</small></span></label>`;
  }
  function connectVoice(actor) {
    const v=remote.voice;
    if(scenario==='mic-denied'){v.stage='error';v.reason=`${actor} 的麦克风权限未开启`;v.startedAt=0;return;}
    v.stage='active';v.reason='';v.startedAt=Date.now();
  }
  function voiceBar() {
    const v=remote.voice,actor=remoteActor(),peer=voicePeer(),active=v.stage==='active',incoming=v.stage==='ringing'&&v.caller!==actor;
    let title='边说边协作',copy='开启双向语音，一起看、一起解决。',actions=raButton('语音通话','voice-call','phone','','primary'),participants='';
    if(active){
      title=`双向通话 <span data-ra-voice-elapsed>${voiceElapsed()}</span>`;copy=v.deafened[actor]?'扬声器已关闭，你暂时听不到对方':v.muted[actor]?'你的麦克风已关闭，对方听不到你':v.muted[peer]?'对方已静音，你仍可以说话':'双方可以直接说话，无需按住按钮';
      participants=`<div class="ra-voice-people">${[actor,peer].map(name=>`<span class="${v.muted[name]?'muted':''}" data-voice-person="${name}">${raIcon(v.muted[name]?'micOff':'mic')}${name===actor?'我':name}<small>${v.muted[name]?'已静音':'麦克风开启'}</small></span>`).join('')}</div>`;
      actions=raButton(v.muted[actor]?'开启麦克风':'关闭麦克风','voice-mic',v.muted[actor]?'micOff':'mic',`aria-pressed="${v.muted[actor]}"`)+raButton(v.deafened[actor]?'开启扬声器':'关闭扬声器','voice-speaker',v.deafened[actor]?'speakerOff':'speaker',`aria-pressed="${v.deafened[actor]}"`)+raButton('音频设置','voice-settings','tune')+raButton('挂断语音','voice-hangup','hangup','','danger');
    } else if(v.stage==='ringing') {
      title=incoming?`${v.caller} 邀请你语音通话`:`正在呼叫 ${peer}…`;copy=incoming?'接听后开启麦克风和扬声器，远程权限保持不变。':'等待对方接听，远程画面继续。';
      actions=incoming?raButton('拒绝','voice-reject','hangup')+raButton('接听语音','voice-accept','phone','','primary'):raButton('取消呼叫','voice-hangup','hangup');
    } else if(v.stage==='error') {title=v.reason;copy='语音未接通，远程协助仍可继续。检查音频设备后重试。';actions=raButton('音频设置','voice-settings','tune')+raButton('重新呼叫','voice-call','phone','','primary');}
    else if(v.stage==='ended'||v.stage==='declined') {title=v.stage==='declined'?(v.caller===actor?'对方未接听语音':'已拒绝语音通话'):'语音已结束';copy='远程协助继续，可随时重新发起通话。';}
    return `<section class="ra-voice-bar ${active?'active':v.stage==='error'?'issue':''}" data-od-id="phase3-remote-voice" aria-label="远程协助语音"><span class="ra-voice-emblem">${raIcon('phone')}</span><div class="ra-voice-summary"><strong>${title}</strong><small>${copy}</small></div>${participants}<div class="ra-voice-actions">${actions}</div></section>`;
  }
  function voiceSettings() {
    const actor=remoteActor(),v=remote.voice;
    const devices=(id,values,selected)=>`<select id="${id}">${values.map(([value,label])=>`<option value="${value}" ${selected===value?'selected':''}>${label}</option>`).join('')}</select>`;
    modal('语音设备',`<p>${actor} 的音频设置，只影响自己这端。</p><label class="p3-field"><span>麦克风</span>${devices('raVoiceInput',[['default','系统默认麦克风'],['headset','耳机麦克风'],['usb','USB 麦克风']],v.input[actor])}</label><label class="p3-field"><span>扬声器 / 耳机</span>${devices('raVoiceOutput',[['default','系统默认输出'],['headset','耳机'],['speaker','扬声器']],v.output[actor])}</label><p class="p3-sub">使用耳机能减少回声。麦克风权限不可用时，在系统设置中允许 XChat 使用麦克风后重试。</p><p class="ra-audio-demo">示例设备 · 原型不读取麦克风或播放声音</p>`,btn('取消','close')+btn('应用','voice-settings-save','','primary'));
  }
  function handleVoiceAction(action) {
    if(!['active','paused'].includes(remote.stage))return;
    const v=remote.voice,actor=remoteActor();
    if(action==='voice-call') {if(['active','ringing'].includes(v.stage))return;if(['mic-denied','voice-drop'].includes(scenario))scenario='normal';v.stage='ringing';v.caller=actor;v.reason='';render();}
    else if(action==='voice-accept') {if(v.stage!=='ringing'||v.caller===actor)return;connectVoice(actor);render();}
    else if(action==='voice-reject') {if(v.stage!=='ringing'||v.caller===actor)return;v.stage='declined';v.startedAt=0;render();}
    else if(action==='voice-hangup') {if(!['active','ringing'].includes(v.stage))return;v.stage='ended';v.startedAt=0;render();}
    else if(action==='voice-mic') {if(v.stage!=='active')return;v.muted[actor]=!v.muted[actor];render();root.querySelector('[data-p3-action=voice-mic]')?.focus();}
    else if(action==='voice-speaker') {if(v.stage!=='active')return;v.deafened[actor]=!v.deafened[actor];render();root.querySelector('[data-p3-action=voice-speaker]')?.focus();}
    else if(action==='voice-settings')voiceSettings();
    else if(action==='voice-settings-save') {v.input[actor]=byId('raVoiceInput').value;v.output[actor]=byId('raVoiceOutput').value;closeDialog();render();toast('本端音频设备设置已更新');}
  }
  function renderRemoteSession() {
    const people=remotePeople(),host=role==='host',paused=remote.stage==='paused',control=remote.permission==='control',slow=scenario==='weak';
    const status=paused?'共享已暂停':control?(host?`${people.viewer} 正在控制我的电脑`:`正在控制 ${people.host} 的电脑`):(host?`${people.viewer} 正在查看我的屏幕`:`正在查看 ${people.host} 的屏幕`);
    root.innerHTML=`<header class="ra-session-head"><div class="ra-session-identity"><span class="ra-avatar small">${host?people.viewer[0]:people.host[0]}</span><div><h1>${host?(paused?'我的屏幕已暂停共享':'我的屏幕正在共享'):`${people.host}的远程桌面`}</h1><p><span class="ra-online"></span>${remoteActor()} · ${host?'共享方':'协助方'}<span class="ra-dot">·</span><span data-ra-elapsed>${remoteElapsed()}</span></p></div></div><div class="ra-session-head-actions"><span class="ra-session-permission ${control?'control':''}">${raIcon(control?'control':'monitor')}${paused?'已暂停':control?'允许控制':'仅查看'}</span>${raButton('结束协助','remote-end','close','','danger')}</div></header><div class="ra-toolbar"><span class="ra-screen-label">${raIcon('monitor')}屏幕 ${remote.screen}</span>${host?raButton('切换屏幕','remote-screen','monitor'):''}<span class="ra-tool-divider"></span><label class="ra-scale-label">显示<select id="raScale" aria-label="画面缩放"><option value="fit" ${remote.scale==='fit'?'selected':''}>适应窗口</option><option value="actual" ${remote.scale==='actual'?'selected':''}>原始比例 100%</option></select></label>${raButton(remote.quality,'remote-quality','tune')}${raButton(remote.expanded?'退出全屏':'全屏','remote-expand',remote.expanded?'shrink':'expand')}<span class="ra-toolbar-spacer"></span>${!host&&!control&&!paused?raButton(remote.controlPending?'等待授权':'申请控制','remote-control','control',remote.controlPending?'disabled':'','primary'):''}${!host&&control?raButton('释放控制','remote-revoke','control'):''}${host&&!control&&!paused?raButton(remote.controlPending?'处理控制请求':'允许对方控制',remote.controlPending?'control-consent':'remote-offer-control','control','','primary'):''}${raButton('沟通','remote-chat-toggle','chat',`aria-pressed="${remote.chatOpen}"`)}</div>${voiceBar()}${slow?`<div class="ra-network-warning">${icon('alert')}网络有波动，画面可能稍有延迟。${link('切换为优先流畅','remote-fast')}</div>`:''}<div class="ra-workspace ${remote.chatOpen?'with-chat':''}"><div class="ra-canvas-wrap">${host?`<div class="ra-sharing-strip ${control?'control':''}">${raIcon(control?'control':'monitor')}<strong>${status}</strong><div>${control?raButton('收回控制','remote-revoke','shield','','danger'):''}${raButton(paused?'继续共享':'暂停共享',paused?'remote-resume':'remote-pause',paused?'play':'pause')}${raButton('结束','remote-end','close','','danger')}</div></div>`:`<div class="ra-viewer-status">${raIcon(control?'control':'monitor')}<span>${paused?'对方已暂停共享':control?'鼠标与键盘已获本次授权':'当前仅查看，操作需要对方同意'}</span></div>`}<div class="ra-canvas ${remote.scale==='actual'?'actual':''}">${paused?`<div class="ra-paused">${raIcon('pause')}<h2>画面已暂停共享</h2><p>桌面已隐藏，鼠标键盘权限已收回。</p>${host?btn('继续共享','remote-resume','','primary'):'<span>等待对方继续共享</span>'}</div>`:remoteDesktop()}</div><footer class="ra-connection-strip"><span><i class="ra-online ${slow?'weak':''}"></i>${slow?'网络波动':'局域网直连'}</span><span>${slow?'186':'12'} ms</span><span>${slow?'8':remote.fps} FPS</span><span>${remote.screen==='1'?'1920 × 1080':'1080 × 1920'}</span><span class="ra-connection-right">${paused?'已暂停':remote.quality} · ${remote.scale==='actual'?'100%':'适应窗口'}</span></footer></div>${remote.chatOpen?remoteChatPanel():''}</div>`;
    const messages=root.querySelector('.ra-chat-messages');if(messages)messages.scrollTop=messages.scrollHeight;
  }
  function handleRemoteAction(action,target) {
    if(action==='role'){if(remote.stage==='idle')return;role=role==='viewer'?'host':'viewer';render();if(remote.stage==='waiting'&&remoteRecipient())incomingRemoteConsent();else if(role==='host'&&remote.controlPending)controlConsent();}
    else if(action==='remote-start')prepareRemote(target.dataset.mode);
    else if(action==='remote-send')sendRemote(target.dataset.mode);
    else if(action==='remote-consent')incomingRemoteConsent();
    else if(action==='remote-accept')acceptRemote(target.dataset.permission);
    else if(action==='remote-reject')stopRemote('rejected');
    else if(action==='remote-cancel')stopRemote('cancelled');
    else if(action==='remote-home'){remote=initialRemote();role='viewer';scenario='normal';render();}
    else if(action==='remote-retry'){scenario='normal';prepareRemote(remote.mode);}
    else if(action==='remote-control'){if(role!=='viewer'||remote.stage!=='active'||remote.permission==='control')return;remote.controlPending=true;render();}
    else if(action==='control-consent'||action==='remote-offer-control')controlConsent();
    else if(action==='remote-deny-control'){remote.controlPending=false;remote.permission='view';closeDialog();render();}
    else if(action==='remote-allow-control'){if(role!=='host'||remote.stage!=='active')return;remote.permission='control';remote.controlPending=false;closeDialog();render();}
    else if(action==='remote-revoke'){remote.permission='view';remote.controlPending=false;render();toast(role==='host'?'控制权已收回':'已释放控制，继续仅查看');}
    else if(action==='remote-pause'){if(role!=='host')return;remote.stage='paused';remote.permission='view';remote.controlPending=false;render();}
    else if(action==='remote-resume'){if(role!=='host')return;remote.stage='active';remote.permission='view';render();}
    else if(action==='remote-screen')switchRemoteScreen();
    else if(action==='remote-screen-confirm'){if(role!=='host')return;remote.screen=dialog.querySelector('[name=p3Screen]:checked').value;remote.permission='view';remote.controlPending=false;closeDialog();render();}
    else if(action==='remote-end')stopRemote('ended');
    else if(action==='remote-quality')remoteQuality();
    else if(action==='remote-quality-save'){remote.quality=dialog.querySelector('[name=raQuality]:checked').value;remote.fps=byId('raFrameLimit').value;remote.color=byId('raColor').value;closeDialog();render();}
    else if(action==='remote-fast'){remote.quality='优先流畅';render();}
    else if(action==='remote-expand'){remote.expanded=!remote.expanded;render();}
    else if(action==='remote-chat-toggle'){remote.chatOpen=!remote.chatOpen;render();}
    else if(action==='remote-chat-send'){const field=byId('raChatDraft'),value=field?.value.trim();if(!value)return;remote.chat.push({name:remoteActor(),text:value.slice(0,300)});remote.chatDraft='';render();byId('raChatDraft')?.focus();}
    else if(action==='remote-demo-open'){if(role!=='viewer'||remote.permission!=='control'||remote.stage!=='active')return;remote.demoOpen=!remote.demoOpen;render();}
  }
  function remoteScenarioChanged() {
    if(['disconnect','locked'].includes(scenario))stopRemote('disconnected',scenario);
    else if(scenario==='timeout')stopRemote('expired');
    else if(['mic-denied','voice-drop'].includes(scenario)){if(['active','paused'].includes(remote.stage)){remote.voice.stage='error';remote.voice.startedAt=0;remote.voice.reason=scenario==='mic-denied'?`${remoteActor()} 的麦克风权限未开启`:'语音连接已中断';closeDialog();}}
    else if(scenario!=='weak'){remote=initialRemote();role='viewer';}
  }

  function reset() { epoch++;backupToken++;dialogOperation='';closeDialog();tasks=initialTasks();filter='all';kind='all';query='';selected.clear();detailId='';scenario='normal';diagnostic='issue';diagnosticPeer='zhang';role='viewer';remote=initialRemote();backups=[{name:'XChat-2026-10-08.xchatbackup',date:'昨天 18:30',size:'128 MB',contents:'聊天记录、偏好设置'}];openPage(page||'tasks',true); }
  function enable() { if(enabled)return;enabled=true;document.body.classList.add('p3-review');bar.hidden=false;rail.hidden=false;scenarioSelect.value='phase3';openPage(new URLSearchParams(location.search).get('p3')||'tasks'); }
  function disable() { epoch++;backupToken++;enabled=false;dialogOperation='';closeDialog();bar.hidden=true;rail.hidden=true;root.hidden=true;document.body.classList.remove('p3-review','p3-page','p3-remote-live','p3-remote-expanded');clearInterval(remoteTimer);byId('p3RemoteEntry')?.remove();byId('p3FileTasks')?.remove();byId('p3SettingsLinks')?.remove();byId('settingsListTitle').textContent='设置';previousTab('sessions'); }

  document.addEventListener('click',event=>{
    const target=event.target.closest('[data-p3-action]');if(!target||!enabled||target.disabled)return;
    const action=target.dataset.p3Action,id=target.dataset.id,t=tasks.find(item=>item.id===id);
    if(action==='page'){closeDialog();openPage(target.dataset.page);}
    else if(action==='close')closeDialog();
    else if(action==='theme'){document.documentElement.dataset.theme=document.documentElement.dataset.theme==='dark'?'light':'dark';}
    else if(action==='reset')reset();
    else if(action==='exit'){disable();scenarioSelect.value='changed';scenarioSelect.dispatchEvent(new Event('change',{bubbles:true}));}
    else if(action==='filter'){filter=target.dataset.filter;render();}
    else if(action==='kind'){kind=target.dataset.kind;render();}
    else if(action==='clear-filter'){query='';kind='all';filter='all';scenario='normal';render();}
    else if(action==='detail'){detailId=id;render();}
    else if(action==='detail-close'){detailId='';render();}
    else if(action==='cancel')cancelTasks([id]);
    else if(action==='batch-cancel')cancelTasks([...selected]);
    else if(action==='cancel-confirm'){target.dataset.ids.split(',').forEach(key=>{const item=tasks.find(x=>x.id===key);if(item&&!complete(item)){item.status='cancelled';item.detail='后续发送已停止';selected.delete(key);}});closeDialog();render();toast('未完成的发送已取消');}
    else if(action==='retry')retryTask(t);
    else if(action==='batch-retry'){const count=tasks.filter(item=>selected.has(item.id)&&retryTask(item)).length;if(!count)toast('所选任务需等待上线或先处理源文件');}
    else if(action==='source')sourceDialog(id);
    else if(action==='source-confirm'){if(dialog.querySelector('[name=p3Source]:checked').value==='changed'){byId('p3SourceError').innerHTML='<p class="p3-error">文件内容与原任务不同，不能作为续传源。请选择原文件；修改版请在会话中作为新文件发送。</p>';return;}t.status='sending';t.detail='已找回原文件 · 正在继续发送';t.progress=48;t.note='源文件一致，从已确认的位置继续发送。';closeDialog();render();toast('已验证原文件，继续传输');}
    else if(action==='clear-tasks')modal('清理已结束的任务记录？','<p>只移除任务中心中的已完成和已取消条目。聊天记录与本地文件都保留。</p>',btn('取消','close')+btn('清理记录','clear-confirm','','primary'));
    else if(action==='clear-confirm'){tasks=tasks.filter(item=>!complete(item));detailId='';closeDialog();render();}
    else if(action==='conversation'){closeDialog();setTab('sessions');selectDevice('zhang-3');}
    else if(action==='diagnose'){diagnosticPeer=id==='offline'?'lisi':'zhang';diagnostic='issue';openPage('diagnostics');}
    else if(action==='network-settings'){setTab('settings');byId('settings-network')?.scrollIntoView({block:'start'});}
    else if(action==='diag-run'){diagnostic='checking';render();after(1100,()=>{diagnostic=scenario==='healthy'?'healthy':'issue';render();});}
    else if(action==='export')exportDialog();
    else if(action==='export-save'){const addresses=byId('p3IncludeAddresses').checked;const blob=new Blob([JSON.stringify({prototype:true,device:'设备 A',address:addresses?diagTarget().address:'192.168.*.*',result:scenario==='healthy'?'ready':scenario==='identity'?'identity_mismatch':'connection_timeout',message_content:'excluded',file_content:'excluded'},null,2)],{type:'application/json'});const url=URL.createObjectURL(blob),a=document.createElement('a');a.href=url;a.download='XChat-演示诊断.json';a.click();setTimeout(()=>URL.revokeObjectURL(url),1000);closeDialog();toast('已下载演示诊断文件');}
    else if(action==='backup-create')backupCreate();
    else if(action==='backup-start')runBackup();
    else if(action==='backup-reselect'){scenario='normal';renderBar();backupCreate();byId('p3BackupLocation').selectedIndex=1;}
    else if(action==='backup-cancel')closeDialog();
    else if(action==='restore')restoreStart();
    else if(action==='restore-check')restorePreview();
    else if(action==='restore-run')runRestore();
    else if(action.startsWith('voice-'))handleVoiceAction(action);
    else if(action==='role'||action==='control-consent'||action.startsWith('remote-'))handleRemoteAction(action,target);
  });
  document.addEventListener('input',event=>{if(enabled&&event.target.id==='p3Search'){const position=event.target.selectionStart;query=event.target.value;renderTasks();const field=byId('p3Search');field.focus();field.setSelectionRange(position,position);}});
  document.addEventListener('change',event=>{
    if(!enabled)return;const target=event.target;
    if(target.dataset.taskSelect){target.checked?selected.add(target.dataset.taskSelect):selected.delete(target.dataset.taskSelect);render();}
    else if(target.id==='p3Scenario'){scenario=target.value;diagnostic='issue';selected.clear();detailId='';if(page==='remote')remoteScenarioChanged();render();}
    else if(target.id==='p3DiagPeer'){diagnosticPeer=target.value;diagnostic='issue';render();}
    else if(target.id==='p3IncludeAddresses')updateExportPreview();
    else if(target.id==='raScale'){remote.scale=target.value;render();}
  });

  document.addEventListener('input',event=>{if(enabled&&event.target.id==='raChatDraft')remote.chatDraft=event.target.value;});
  document.addEventListener('keydown',event=>{
    if(!enabled||page!=='remote')return;
    if(event.key==='Escape'&&!dialog.open&&remote.expanded){event.preventDefault();remote.expanded=false;render();}
    if(event.target.id==='raChatDraft'&&event.key==='Enter'&&!event.shiftKey&&!event.isComposing){event.preventDefault();root.querySelector('[data-p3-action=remote-chat-send]')?.click();}
  });
  document.addEventListener('change',event=>{if(event.target===scenarioSelect&&scenarioSelect.value!=='phase3'&&enabled)disable();},true);
  scenarioSelect.addEventListener('change',()=>{if(scenarioSelect.value==='phase3')enable();});
  if(new URLSearchParams(location.search).get('review')==='phase3')enable();
})();
