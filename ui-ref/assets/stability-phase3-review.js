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
  const initialRemote = () => ({stage:'idle',permission:'view',screen:'1',quality:'自动',reason:'',controlPending:false,owner:'peer'});
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
      : page === 'remote' ? [['normal','可用设备'],['offline','对方离线'],['unsupported','对方不支持'],['busy','对方忙碌'],['disconnect','会话断线'],['locked','对方锁屏']]
      : [['normal','典型任务'],['empty','空列表']];
    bar.innerHTML = `<strong>阶段三 · 交互评审</strong>${Object.entries(pages).map(([key,title]) => `<button data-p3-action="page" data-page="${key}" aria-pressed="${page===key}">${title}</button>`).join('')}<span class="p3-spacer"></span><span class="p3-review-label">演示数据 · 不连接设备</span><select id="p3Scenario" aria-label="演示场景">${options.map(([value,label]) => `<option value="${value}" ${scenario===value?'selected':''}>${label}</option>`).join('')}</select>${page==='remote'?`<button data-p3-action="role" aria-pressed="${role==='host'}">${role==='viewer'?'切到共享方视角':'切到查看方视角'}</button>`:''}<button class="p3-theme" data-p3-action="theme">浅 / 深</button><button data-p3-action="reset">重置</button><button class="p3-review-exit" data-p3-action="exit">退出评审</button>`;
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
  dialog.addEventListener('keydown', event => { if(event.key==='Escape') { event.preventDefault(); closeDialog(); } });
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
  setTab = function(tab) { root.hidden=true; document.body.classList.remove('p3-page'); rail.classList.remove('active'); byId('settingsListTitle').textContent='设置'; previousTab(tab); if(enabled) { page=''; renderBar(); enhanceEntrances(); } };
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

  function remoteSidebar() {
    byId('list').innerHTML=`<div class="p3-sidebar-head"><b>协助一个具体的人</b><p>会话开始前，双方确认。</p></div><div class="p3-sidebar-peer"><span class="p3-peer-avatar">张</span><div><strong>张三</strong><small>DESKTOP-ZHANG · Windows</small></div></div><div class="p3-side-note"><b>本次协助范围</b>先查看共享的屏幕，再单独申请控制。<br><br>任一方都可以结束会话。断线、锁屏后，需要重新同意。<br><br>${link('回到与张三的聊天','conversation')}</div><div class="p3-side-note"><b>功能可用性</b>查看与控制按双方设备能力显示。此评审以 Windows 桌面为例。</div>`;
  }
  function demoScreen() {
    return `<div class="p3-demo-screen ${remote.screen==='2'?'portrait':''}" data-od-id="phase3-remote-screen"><div class="p3-desktop-title"><i></i><i></i><i></i><span>项目文档 / 网络检查清单</span></div><div class="p3-desktop-content"><aside class="p3-desktop-nav"><b>工作空间</b>项目概览<br>检查清单<br>共享文件<br>会议记录</aside><section class="p3-desktop-doc"><h3>网络检查清单</h3><p>本周协作记录 · 设计团队</p><table><thead><tr><th>检查项</th><th>负责人</th><th>状态</th></tr></thead><tbody><tr><td>确认设备在线</td><td>张三</td><td>已完成</td></tr><tr><td>检查当前地址</td><td>张三</td><td>已完成</td></tr><tr><td>连接消息服务</td><td>协助中</td><td>待复核</td></tr><tr><td>发送测试文件</td><td>张三</td><td>待确认</td></tr></tbody></table></section></div><span class="p3-demo-watermark">示意屏幕 · 非真实桌面</span><footer class="p3-desktop-footer"><span>屏幕 ${remote.screen}</span><span>10:48</span></footer></div>`;
  }
  function renderRemote() {
    remoteSidebar();
    const unavailable=['offline','unsupported','busy'].includes(scenario), isHost=role==='host', people=remotePeople();
    const descriptions={offline:['对方暂时离线','对方上线后才能开始协助。你可以先在聊天中说明需要帮助的内容。'],unsupported:['对方暂不支持远程协助','此设备仍可正常聊天和传文件。远程协助需双方使用支持此功能的桌面版本。'],busy:['对方正在另一场协助中','请等待对方结束当前会话，再重新发起请求。']};
    const stage=remote.stage;
    root.innerHTML=heading('远程协助',`${isHost?'共享方':'查看方'}视角 · ${isHost?people.host:people.viewer}`,stage==='active'||stage==='paused'?btn('结束协助','remote-end','','danger'):'');
    if(['active','paused'].includes(stage)) {
      const controlling=remote.permission==='control';
      root.insertAdjacentHTML('beforeend',`<div class="p3-session-bar ${controlling?'control':''}">${icon(controlling?'shield':'screen')}<strong>${stage==='paused'?'屏幕共享已暂停':controlling?(isHost?`${people.viewer} 正在控制你的电脑`:`正在控制${people.host}的电脑`):(isHost?`${people.viewer} 正在查看你的屏幕`:`正在查看${people.host}的屏幕`)}</strong><div class="p3-actions">${isHost&&controlling?btn('立即撤销控制','remote-revoke','','danger'):''}${isHost?btn(stage==='paused'?'继续共享':'暂停共享',stage==='paused'?'remote-resume':'remote-pause'):''}</div></div><div class="p3-session-grid"><div class="p3-screen-wrap">${stage==='paused'?`<div class="p3-screen-state">${icon('lock')}<b>共享已暂停</b>画面已隐藏，控制权已撤销。${isHost?'你可以继续共享或结束本次协助。':'等待对方继续共享。'}</div>`:demoScreen()}</div><aside class="p3-session-info"><section><h3>当前权限</h3><div class="p3-permission">查看屏幕<span>${stage==='paused'?'已暂停':'已允许'}</span></div><div class="p3-permission">鼠标与键盘<span>${controlling?'已允许':'未允许'}</span></div><div class="p3-permission">剪贴板与文件<span>未共享</span></div>${!isHost&&!controlling&&stage==='active'?btn(remote.controlPending?'等待对方同意':'申请控制','remote-control',remote.controlPending?'disabled':'','primary'):''}${isHost&&remote.controlPending?btn('查看控制请求','control-consent','','primary'):''}<p>本次授权只用于当前会话，可随时撤销。</p></section><section><h3>共享画面</h3><label class="p3-field"><span>画质</span><select id="p3Quality">${['自动','优先流畅','优先清晰'].map(v=>`<option ${remote.quality===v?'selected':''}>${v}</option>`).join('')}</select></label><p>屏幕 ${remote.screen} · ${remote.screen==='1'?'1920 × 1080':'1080 × 1920'}<br>自动模式优先保持聊天响应。</p>${isHost?link('切换共享屏幕','remote-screen'):''}<p>${isHost?'你正在共享所选屏幕，可随时停止。':'只有对方共享的屏幕会显示在这里。'}</p></section></aside></div>`);return;
    }
    let title='一起看，问题更容易说明白', text='向张三请求查看屏幕，或邀请张三查看你的屏幕。每次共享都需要对方明确同意。', actions=btn('请求查看对方屏幕','remote-request',unavailable?'disabled':'','primary')+btn('邀请对方查看我的屏幕','remote-invite',unavailable?'disabled':'');
    if(unavailable){[title,text]=descriptions[scenario];actions=btn('回到聊天','conversation')+btn('检查连接','diagnose');}
    else if(stage==='pending') { title=isHost?`${people.viewer} 请求查看你的屏幕`:'已发送查看请求';text=isHost?'先选择要共享的屏幕，再决定是否同意。当前没有任何画面被共享。':`等待${people.host}选择屏幕并同意。你可以取消本次请求。`;actions=isHost?btn('查看请求','view-consent','','primary')+btn('拒绝','remote-reject'):btn('取消请求','remote-cancel'); }
    else if(stage==='inviting') {title=isHost?'邀请已发出':`${people.host} 邀请你查看屏幕`;text=isHost?`等待${people.viewer}接受邀请，当前没有画面被共享。`:'接受后只查看对方选定的屏幕，鼠标键盘控制仍需单独申请。';actions=isHost?btn('取消邀请','remote-cancel'):btn('拒绝','remote-reject')+btn('接受查看邀请','remote-invite-accept','','primary');}
    else if(stage==='rejected') {title='本次协助未获同意';text='没有共享屏幕，也没有授予控制权限。你可以回到聊天确认方便的时间。';actions=btn('回到聊天','conversation')+btn('重新请求','remote-request');}
    else if(stage==='ended') {title='远程协助已结束';text='屏幕共享已停止，鼠标键盘权限已收回。聊天和文件任务可以继续。';actions=btn('回到聊天','conversation')+btn('发起新的协助','remote-request');}
    else if(stage==='disconnected') {title=remote.reason==='locked'?'对方屏幕已锁定':'连接已中断';text='画面已隐藏，控制权已收回。再次连接需要对方重新同意，之前的控制授权不会自动恢复。';actions=btn('重新请求查看','remote-request','','primary')+btn('结束协助','remote-end');}
    root.insertAdjacentHTML('beforeend',`<div class="p3-body"><div class="p3-remote-empty"><div class="p3-remote-symbol">${icon(stage==='disconnected'?'lock':'screen')}</div><h2>${title}</h2><p>${text}</p><div class="p3-actions">${actions}</div><div class="p3-remote-principles"><span>${icon('check')}每次明确同意</span><span>${icon('shield')}控制单独授权</span><span>${icon('close')}随时结束协助</span></div>${['pending','inviting'].includes(stage)?'<p style="margin-top:30px">评审提示：点击顶部视角切换按钮，体验收到请求或邀请的一方。</p>':''}</div></div>`);
  }
  function screenOptions() { return `<div class="p3-screens">${[['1','屏幕 1 · 主屏','1920 × 1080'],['2','屏幕 2 · 竖屏','1080 × 1920']].map(([value,name,size])=>`<label><div class="p3-screen-thumb">${icon('screen')}</div><input type="radio" name="p3Screen" value="${value}" ${remote.screen===value?'checked':''}> ${name}<small>${size}</small></label>`).join('')}</div>`; }
  function viewConsent(invite=false, switching=false) {
    modal(switching?'切换共享屏幕':invite?'邀请张三查看你的屏幕':`${remotePeople().viewer} 请求查看你的屏幕`,`<p>${switching?'选择新的共享屏幕，切换后控制权会收回。':invite?'先选择要共享的屏幕。对方接受邀请后开始共享。':'你可以只允许查看；鼠标键盘控制需要之后再次单独同意。'}</p>${screenOptions()}<p>对方可以看到所选屏幕中的内容，你可以随时暂停或停止共享。</p>`,btn(switching||invite?'取消':'拒绝',switching||invite?'close':'remote-reject')+btn(switching?'确认切换':invite?'发送邀请':'仅允许查看',switching?'remote-screen-confirm':invite?'remote-invite-send':'remote-allow-view','','primary'));
  }
  function controlConsent() {modal(`允许 ${remotePeople().viewer} 控制你的电脑？`,'<p>允许后，对方可以在本次会话中操作鼠标和键盘。你仍可在共享提示条中立即撤销控制。</p><div class="p3-permission">鼠标与键盘<span>本次申请</span></div><div class="p3-permission">剪贴板与文件<span>不授权</span></div><p style="margin-top:16px">结束、断线、暂停共享或锁屏后，控制权限立即失效。</p>',btn('保持仅查看','remote-deny-control')+btn('允许本次控制','remote-allow-control','','primary'));}

  function reset() { epoch++;backupToken++;dialogOperation='';closeDialog();tasks=initialTasks();filter='all';kind='all';query='';selected.clear();detailId='';scenario='normal';diagnostic='issue';diagnosticPeer='zhang';role='viewer';remote=initialRemote();backups=[{name:'XChat-2026-10-08.xchatbackup',date:'昨天 18:30',size:'128 MB',contents:'聊天记录、偏好设置'}];openPage(page||'tasks',true); }
  function enable() { if(enabled)return;enabled=true;document.body.classList.add('p3-review');bar.hidden=false;rail.hidden=false;scenarioSelect.value='phase3';openPage(new URLSearchParams(location.search).get('p3')||'tasks'); }
  function disable() { epoch++;backupToken++;enabled=false;dialogOperation='';closeDialog();bar.hidden=true;rail.hidden=true;root.hidden=true;document.body.classList.remove('p3-review','p3-page');byId('p3RemoteEntry')?.remove();byId('p3FileTasks')?.remove();byId('p3SettingsLinks')?.remove();byId('settingsListTitle').textContent='设置';previousTab('sessions'); }

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
    else if(action==='role'){role=role==='viewer'?'host':'viewer';render();if(role==='host'&&remote.stage==='pending')viewConsent();else if(role==='host'&&remote.controlPending)controlConsent();}
    else if(action==='remote-request'){scenario='normal';role='viewer';remote.owner='peer';remote.stage='pending';remote.permission='view';remote.controlPending=false;render();}
    else if(action==='view-consent')viewConsent();
    else if(action==='remote-invite')viewConsent(true);
    else if(action==='remote-invite-send'){remote.screen=dialog.querySelector('[name=p3Screen]:checked').value;remote.owner='self';role='host';remote.stage='inviting';remote.permission='view';remote.controlPending=false;closeDialog();render();}
    else if(action==='remote-invite-accept'){role='viewer';remote.stage='active';remote.permission='view';remote.controlPending=false;closeDialog();render();}
    else if(action==='remote-allow-view'){remote.screen=dialog.querySelector('[name=p3Screen]:checked').value;remote.stage='active';remote.permission='view';remote.controlPending=false;closeDialog();render();}
    else if(action==='remote-reject'){remote.stage='rejected';remote.permission='view';remote.controlPending=false;closeDialog();render();}
    else if(action==='remote-cancel'){role='viewer';remote=initialRemote();render();}
    else if(action==='remote-control'){remote.controlPending=true;render();toast('已申请控制，需对方单独同意');}
    else if(action==='control-consent')controlConsent();
    else if(action==='remote-deny-control'){remote.controlPending=false;remote.permission='view';closeDialog();render();toast('继续保持仅查看');}
    else if(action==='remote-allow-control'){remote.controlPending=false;remote.permission='control';closeDialog();render();}
    else if(action==='remote-revoke'){remote.permission='view';remote.controlPending=false;render();toast('控制权已收回，继续仅查看');}
    else if(action==='remote-pause'){remote.stage='paused';remote.permission='view';remote.controlPending=false;render();}
    else if(action==='remote-resume'){remote.stage='active';remote.permission='view';render();}
    else if(action==='remote-screen')viewConsent(false,true);
    else if(action==='remote-screen-confirm'){remote.screen=dialog.querySelector('[name=p3Screen]:checked').value;remote.permission='view';remote.controlPending=false;closeDialog();render();}
    else if(action==='remote-end'){remote.stage='ended';remote.permission='view';remote.controlPending=false;closeDialog();render();}
  });
  document.addEventListener('input',event=>{if(enabled&&event.target.id==='p3Search'){const position=event.target.selectionStart;query=event.target.value;renderTasks();const field=byId('p3Search');field.focus();field.setSelectionRange(position,position);}});
  document.addEventListener('change',event=>{
    if(!enabled)return;const target=event.target;
    if(target.dataset.taskSelect){target.checked?selected.add(target.dataset.taskSelect):selected.delete(target.dataset.taskSelect);render();}
    else if(target.id==='p3Scenario'){scenario=target.value;diagnostic='issue';selected.clear();detailId='';if(page==='remote'){remote.permission='view';remote.controlPending=false;if(['disconnect','locked'].includes(scenario)){remote.stage='disconnected';remote.reason=scenario;}else{remote=initialRemote();role='viewer';}}render();}
    else if(target.id==='p3DiagPeer'){diagnosticPeer=target.value;diagnostic='issue';render();}
    else if(target.id==='p3IncludeAddresses')updateExportPreview();
    else if(target.id==='p3Quality'){remote.quality=target.value;toast(`画质已设为${remote.quality}`);}
  });
  document.addEventListener('change',event=>{if(event.target===scenarioSelect&&scenarioSelect.value!=='phase3'&&enabled)disable();},true);
  scenarioSelect.addEventListener('change',()=>{if(scenarioSelect.value==='phase3')enable();});
  if(new URLSearchParams(location.search).get('review')==='phase3')enable();
})();
