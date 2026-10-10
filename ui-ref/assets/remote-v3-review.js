/* Interactive UI review only. All participants, remote pixels and metrics are samples. */
(() => {
  const params = new URLSearchParams(location.search);
  const scenario = document.getElementById('desktopReviewScenario');
  if (scenario) {
    scenario.add(new Option('远程协作 v3 · 独立窗口与被控方', 'remote-v3'));
    scenario.addEventListener('change', () => {
      if (scenario.value !== 'remote-v3') return;
      const url = new URL(location.href);
      url.search = '?review=remote-v3';
      location.href = url.href;
    });
  }
  if (params.get('review') !== 'remote-v3') return;

  const paths = {
    monitor: '<rect x="3" y="4" width="18" height="12" rx="1.5"/><path d="M8 21h8m-4-5v5"/>',
    control: '<rect x="3" y="4" width="18" height="12" rx="1.5"/><path d="M8 21h6m-3-5v5m3-8 2 8 2-2 3 1-7-7Z"/>',
    chat: '<path d="M5 4h14a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H9l-6 4V6a2 2 0 0 1 2-2Z"/><path d="M7 10h.01M12 10h.01M17 10h.01"/>',
    folder: '<path d="M3 6h7l2 3h9v11H3zM3 6V4h7l2 2h9v3"/>',
    clipboard: '<rect x="5" y="5" width="14" height="16" rx="2"/><rect x="9" y="2" width="6" height="5" rx="1"/><path d="m8 14 3 3 5-6"/>',
    settings: '<path d="M3 5h7m5 0h6M3 12h11m5 0h2M3 19h3m5 0h10"/><circle cx="12" cy="5" r="2"/><circle cx="16" cy="12" r="2"/><circle cx="8" cy="19" r="2"/>',
    search: '<circle cx="10" cy="10" r="6"/><path d="m15 15 6 6"/>',
    phone: '<path d="m7 3 3 5-3 2c1 3 4 6 7 7l2-3 5 3-1 4C10 23 1 14 3 4Z"/>',
    mic: '<rect x="9" y="2" width="6" height="13" rx="3"/><path d="M5 10v2a7 7 0 0 0 14 0v-2M12 19v3m-4 0h8"/>',
    micOff: '<path d="m3 3 18 18M9 9v3a3 3 0 0 0 5 2M9 4a3 3 0 0 1 6 1v4M5 10v2a7 7 0 0 0 12 5m2-5v-2M12 19v3m-4 0h8"/>',
    full: '<path d="M8 3H3v5m13-5h5v5M3 16v5h5m13-5v5h-5"/>',
    close: '<path d="m6 6 12 12M6 18 18 6"/>',
    info: '<circle cx="12" cy="12" r="9"/><path d="M12 11v6m0-10v.01"/>',
    shield: '<path d="m12 2 8 4v6c0 5-8 10-8 10S4 17 4 12V6Z"/><path d="m8 12 3 3 5-6"/>',
    pause: '<path d="M8 4v16M16 4v16"/>',
    play: '<path d="m7 4 14 8-14 8Z"/>',
    minus: '<path d="M5 12h14"/>',
    arrow: '<path d="M14 3h7v7M21 3 11 13"/><path d="M10 3H4a1 1 0 0 0-1 1v16a1 1 0 0 0 1 1h16a1 1 0 0 0 1-1v-6"/>',
    signal: '<path d="M5 19v-4m5 4v-8m5 8V7m5 12V3"/>',
    ethernet: '<path d="M8 3h8v6H8zM3 15h6v6H3zm12 0h6v6h-6zM12 9v3M6 15v-3h12v3"/>',
    check: '<path d="m5 12 4 4L19 6"/>',
    send: '<path d="m3 3 19 9-19 9 4-9-4-9Zm4 9h15"/>',
    smile: '<circle cx="12" cy="12" r="9"/><path d="M8 9h.01M16 9h.01M8 14c2 3 6 3 8 0"/>',
    picture: '<rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="8" cy="8" r="1"/><path d="m3 17 5-5 5 4 4-6 4 6"/>',
    dots: '<circle cx="5" cy="12" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/>',
    grip: '<circle cx="9" cy="5" r=".6"/><circle cx="15" cy="5" r=".6"/><circle cx="9" cy="12" r=".6"/><circle cx="15" cy="12" r=".6"/><circle cx="9" cy="19" r=".6"/><circle cx="15" cy="19" r=".6"/>',
    pointer: '<path d="m4 2 2 17 4-5 4 7 3-2-4-6h6Z"/>',
  };
  const icon = name => `<svg class="rv-icon" viewBox="0 0 24 24" aria-hidden="true">${paths[name] || paths.monitor}</svg>`;
  const button = (action, label, name, classes = '', extra = '') => `<button type="button" class="rv-btn ${classes}" data-rv-action="${action}" ${extra}>${name ? icon(name) : ''}${label}</button>`;
  const escapeHtml = value => String(value).replace(/[&<>"']/g, char => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[char]));
  const state = {
    surface: ['chat', 'viewer', 'sharer'].includes(params.get('surface')) ? params.get('surface') : 'viewer',
    detached: params.get('detached') === '1', info: false, infoTab: 'performance',
    controlled: true, paused: false, compact: false, ended: false,
    voice: false, muted: false, scale: 'fit', display: '1', quality: 'balanced',
    messages: [], toolbarPosition: null,
  };
  let popup = null;
  let toastTimer;
  const root = document.createElement('section');
  root.id = 'remoteV3Review';
  root.setAttribute('aria-label', '远程协作 v3 原型评审');
  document.body.classList.add('remote-v3-review');
  document.body.append(root);

  function reviewbar() {
    return `<header class="rv-reviewbar"><strong>XChat · 远程协作评审</strong><nav class="rv-review-tabs" aria-label="原型界面"><button data-rv-surface="chat" role="tab" aria-selected="${state.surface === 'chat'}">聊天主窗口</button><button data-rv-surface="viewer" role="tab" aria-selected="${state.surface === 'viewer'}">远程独立窗口</button><button data-rv-surface="sharer" role="tab" aria-selected="${state.surface === 'sharer'}">被控方</button></nav><small>交互演示 · 画面与指标均为示例</small><a href="?review=phase3&amp;p3=remote">旧版原型</a></header>`;
  }

  function desktop() {
    return `<div class="rv-desktop ${state.scale === 'actual' ? 'rv-actual' : ''}" aria-label="Windows 远程桌面示意，非真实屏幕流"><div class="rv-desktop-icons"><span>${icon('monitor')}此电脑</span><span>${icon('folder')}工作文件</span></div><article class="rv-os-window"><div class="rv-os-title">${icon('settings')}设置<span>− □ ×</span></div><div class="rv-os-content"><aside class="rv-os-sidebar"><div class="rv-os-user"><span>E</span><div><strong>Eason</strong><small>本地帐户</small></div></div><div class="rv-os-search">查找设置</div><p>系统</p><p>蓝牙和设备</p><p class="selected">网络和 Internet</p><p>个性化</p><p>应用</p><p>帐户</p><p>时间和语言</p><p>Windows 更新</p></aside><section class="rv-os-settings"><h2>网络和 Internet</h2><div class="rv-os-connection">${icon('ethernet')}<div><strong>以太网</strong><small>已连接 · 专用网络</small></div></div><div class="rv-os-setting"><div>以太网<small>连接、网络配置文件、IP 和 DNS 设置</small></div><span>›</span></div><div class="rv-os-setting"><div>高级网络设置<small>所有网络适配器、网络重置</small></div><span>›</span></div><div class="rv-os-setting"><div>网络疑难解答<small>诊断并解决连接问题</small></div><span>打开</span></div></section></div></article><div class="rv-pointer">${icon('pointer')}<span>Eason-MBP</span></div><div class="rv-desktop-label">远程画面示意 · 屏幕 ${state.display}</div><div class="rv-os-taskbar"><span class="rv-windows-mark"><i></i><i></i><i></i><i></i></span><span class="rv-task-search">搜索</span>${icon('folder')}${icon('monitor')}${icon('settings')}<time>16:23<br>2026/10/10</time></div></div>`;
  }

  function infoPanel() {
    if (!state.info) return '';
    const metrics = `<div class="rv-metric-hero">${icon('signal')}网络延时<strong>55</strong> ms</div><svg class="rv-sparkline" viewBox="0 0 270 30" aria-label="示例延时趋势"><path d="M0 22 15 21 30 16 45 18 60 14 75 18 90 11 105 15 120 16 135 13 150 18 165 17 180 14 195 17 210 11 225 13 240 10 255 12 270 13" fill="none" stroke="#69ae7c" stroke-width="1.6"/><path d="M0 29h270" stroke="#edf1e9"/></svg><dl><dt>帧延时</dt><dd>60 ms</dd><dt>帧率</dt><dd>32 FPS</dd><dt>分辨率</dt><dd>1920 × 1080</dd><dt>带宽占用</dt><dd>405.5 Kbps</dd><dt>丢包率</dt><dd>0.0%</dd></dl><hr><dl><dt>传输通道</dt><dd>UDP P2P · 局域网直连</dd><dt>编码方式</dt><dd>H.265</dd><dt>编解码器</dt><dd>硬件编码 / 硬件解码</dd><dt>采集方式</dt><dd>DXGI</dd></dl><p class="rv-info-foot">统计口径：当前远程桌面会话 · 最近 1 秒</p>`;
    const host = `<div class="rv-metric-hero">${icon('monitor')}Eason-Windows</div><dl><dt>操作系统</dt><dd>Windows 11 · 64 位</dd><dt>处理器</dt><dd>Intel Core i7</dd><dt>内存</dt><dd>32 GB</dd><dt>显示器</dt><dd>DISPLAY${state.display} · 1920 × 1080</dd><dt>缩放</dt><dd>100%</dd></dl><hr><dl><dt>被控方 IP</dt><dd>192.168.1.102</dd><dt>主控方 IP</dt><dd>192.168.1.106</dd><dt>主控设备</dt><dd>Eason-MBP(M5)</dd><dt>会话权限</dt><dd>${state.controlled ? '屏幕、鼠标和键盘' : '仅查看屏幕'}</dd><dt>连接时长</dt><dd>02:48</dd></dl>`;
    return `<aside class="rv-info-panel" aria-label="主机与连接信息"><div class="rv-info-head"><strong>主机与连接信息</strong>${button('info', '', 'close', 'rv-quiet rv-icon-only', 'aria-label="关闭信息"')}</div><div class="rv-info-sample">示例数据，用于评审信息布局。<br>不代表当前应用已达到这些性能或已启用这些技术。</div><div class="rv-info-tabs" role="tablist"><button role="tab" data-rv-info-tab="performance" aria-selected="${state.infoTab === 'performance'}">连接性能</button><button role="tab" data-rv-info-tab="host" aria-selected="${state.infoTab === 'host'}">远程主机</button></div>${state.infoTab === 'performance' ? metrics : host}</aside>`;
  }

  function viewer() {
    return `<section class="rv-viewer" aria-label="独立远程桌面窗口"><header class="rv-window-title"><span class="rv-window-lights" aria-hidden="true"><i></i><i></i><i></i></span>${icon('monitor')}<strong>Eason-Windows</strong><span class="rv-window-caption">远程桌面 · XChat</span>${button('end', '结束协助', 'close', 'rv-quiet rv-danger rv-window-end')}</header><div class="rv-viewer-tools"><label class="rv-screen-select">${icon('monitor')}<select aria-label="远程显示器" data-rv-change="display"><option value="1" ${state.display === '1' ? 'selected' : ''}>显示器 1 · 主屏</option><option value="2" ${state.display === '2' ? 'selected' : ''}>显示器 2</option></select></label><span class="rv-separator"></span><label class="rv-scale-select"><span>显示</span><select aria-label="远程画面缩放" data-rv-change="scale"><option value="fit" ${state.scale === 'fit' ? 'selected' : ''}>适应窗口</option><option value="actual" ${state.scale === 'actual' ? 'selected' : ''}>原始大小</option></select></label><label class="rv-scale-select rv-quality">${icon('settings')}<select aria-label="画质策略" data-rv-change="quality"><option value="balanced" ${state.quality === 'balanced' ? 'selected' : ''}>自动画质</option><option value="clarity" ${state.quality === 'clarity' ? 'selected' : ''}>清晰优先</option><option value="smooth" ${state.quality === 'smooth' ? 'selected' : ''}>流畅优先</option></select></label>${button('fullscreen', '<span class="rv-tool-label">全屏</span>', 'full', 'rv-quiet', 'aria-label="全屏查看远程桌面" title="全屏"')}<span class="rv-flex"></span><span class="rv-control-label"><i class="rv-dot"></i>${state.controlled ? '控制已授权' : '仅查看'}</span>${button('control', `<span class="rv-tool-label">${state.controlled ? '释放控制' : '请求控制'}</span>`, 'control', 'rv-quiet', `title="${state.controlled ? '释放控制' : '请求控制'}"`)}<span class="rv-separator"></span>${button('chat', '<span class="rv-tool-label">回到聊天</span>', 'chat', 'rv-quiet', 'title="回到 XChat 主窗口"')}${button('info', '<span class="rv-tool-label">主机信息</span>', 'info', state.info ? 'rv-active' : 'rv-quiet', `aria-expanded="${state.info}" title="主机与连接信息"`)}${infoPanel()}</div><main class="rv-viewer-stage">${state.ended ? `<div class="rv-empty">${icon('monitor')}<h2>远程协助已结束</h2><p>与 Eason-Windows 的屏幕和控制连接已关闭。聊天仍在主窗口中保留。</p>${button('restart', '重新演示', 'play', 'rv-primary')} ${button('chat', '回到聊天', 'chat')}</div>` : desktop()}</main><footer class="rv-viewer-status"><span><i class="rv-dot"></i>${state.ended ? '已断开' : '局域网直连'}</span><button data-rv-action="info" aria-label="查看连接性能">${icon('signal')}55 ms</button><span>32 FPS</span><span class="rv-status-resolution">1920 × 1080</span><span class="rv-sample">示例指标 · 非实测</span></footer></section>`;
  }

  function voiceLine() {
    if (!state.voice) return '';
    return `<div class="rv-voice-line" role="status">${icon(state.muted ? 'micOff' : 'phone')}<strong>语音通话中</strong><span>02:18 · ${state.muted ? '麦克风已静音' : '双方已接通'}</span><i class="rv-flex"></i>${button('mute', state.muted ? '取消静音' : '静音', state.muted ? 'micOff' : 'mic', 'rv-quiet', `aria-pressed="${state.muted}"`)}${button('voice', '挂断', 'phone', 'rv-danger rv-quiet')}</div>`;
  }

  function chat(sharer = false) {
    const peer = sharer ? 'Eason-MBP(M5)' : 'Eason-Windows';
    const title = state.ended ? '远程协助已结束' : sharer ? (state.paused ? '屏幕共享已暂停' : `正在向 ${peer} 共享屏幕`) : '远程桌面已在独立窗口中连接';
    const detail = state.ended ? '聊天记录与语音通话仍可继续' : sharer ? `${state.paused ? '对方暂时看不到新的画面' : state.controlled ? '对方可控制鼠标和键盘' : '对方仅可查看屏幕'} · 显示器 1` : 'Eason-Windows · 局域网直连 · 02:48';
    const messages = state.messages.filter(item => item.sharer === sharer).map(item => `<div class="rv-message mine"><span class="rv-avatar">我</span><div><div class="rv-bubble">${escapeHtml(item.text)}</div><time>刚刚</time></div></div>`).join('');
    const controls = state.ended ? button('restart', '重新演示', 'play') : sharer ? button('control', state.controlled ? '收回控制' : '允许控制', 'shield', state.controlled ? 'rv-danger' : '') : button('popup', '打开远程窗口', 'arrow', 'rv-primary');
    return `<section class="rv-chat-app" aria-label="${sharer ? '被控方聊天窗口，没有本机屏幕预览' : '主控方聊天主窗口'}"><aside class="rv-rail"><span class="rv-self">◕<i class="rv-dot"></i></span><nav aria-label="主导航"><button class="active" data-rv-action="chat" aria-label="聊天">${icon('chat')}</button><button data-rv-action="${sharer ? 'show-pill' : 'popup'}" aria-label="远程协助">${icon('monitor')}</button><button data-rv-action="placeholder" aria-label="文件">${icon('folder')}</button><button data-rv-action="placeholder" aria-label="任务">${icon('clipboard')}</button></nav><button data-rv-action="placeholder" aria-label="设置">${icon('settings')}</button></aside><aside class="rv-conversations"><div class="rv-list-head"><span>${icon('search')}搜索</span><button data-rv-action="placeholder" aria-label="添加会话">+</button></div><div class="rv-conversation selected"><span class="rv-avatar">E</span><div><strong>${peer}</strong><small>远程协助中</small></div><time>16:23</time></div><div class="rv-conversation"><span class="rv-avatar">设</span><div><strong>产品设计组</strong><small>张三：新的原型已更新</small></div></div><div class="rv-conversation"><span class="rv-avatar">L</span><div><strong>LANClaw</strong><small>待办已整理完成</small></div></div></aside><main class="rv-chat"><header class="rv-chat-header"><div><h1>${peer}</h1><p><i class="rv-dot"></i>在线 · ${sharer ? 'macOS' : 'Windows 11'}</p></div><div class="rv-chat-actions">${button('voice', '', 'phone', state.voice ? 'rv-active rv-icon-only' : 'rv-quiet rv-icon-only', `aria-label="${state.voice ? '挂断语音' : '发起语音通话'}" title="${state.voice ? '挂断语音' : '发起语音通话'}"`)}${button('placeholder', '', 'dots', 'rv-quiet rv-icon-only', 'aria-label="更多"')}</div></header><section class="rv-session-banner"><span class="rv-session-emblem">${icon(state.paused ? 'pause' : 'monitor')}</span><div class="rv-session-copy"><strong>${title}</strong><p>${state.ended ? '' : '<i class="rv-dot"></i>'}${detail}</p></div><div class="rv-chat-actions">${controls}${state.ended ? '' : button('end', '', 'close', 'rv-quiet rv-icon-only rv-danger', 'aria-label="结束远程协助" title="结束远程协助"')}</div></section><div class="rv-messages" aria-live="polite"><div class="rv-day">今天 · 16:20</div><div class="rv-message"><span class="rv-avatar">E</span><div><div class="rv-bubble">${sharer ? '我已经连上了，先帮你看一下网络设置。' : '可以帮我看看网络设置吗？'}</div><time>16:20</time></div></div><div class="rv-message mine"><span class="rv-avatar">我</span><div><div class="rv-bubble">${sharer ? '好的，我这边保持共享。' : '可以，我打开远程桌面看一下。'}</div><time>16:21</time></div></div><div class="rv-event">${icon('shield')}${sharer ? '已允许 Eason-MBP(M5) 控制本机' : 'Eason-Windows 已允许你控制屏幕'} · 本次会话有效</div><div class="rv-message"><span class="rv-avatar">E</span><div><div class="rv-bubble">${sharer ? '看到了，我们可以边语音边操作。' : '我这边已准备好，需要我操作的时候告诉我。'}</div><time>16:23</time></div></div>${messages}</div>${voiceLine()}<form class="rv-composer" data-rv-compose><div class="rv-compose-tools"><button type="button" data-rv-action="placeholder" aria-label="表情">${icon('smile')}</button><button type="button" data-rv-action="placeholder" aria-label="发送文件">${icon('folder')}</button><button type="button" data-rv-action="placeholder" aria-label="发送图片">${icon('picture')}</button><small>聊天与远程协助分别操作</small></div><textarea aria-label="消息" placeholder="输入消息…" rows="2"></textarea><div class="rv-compose-footer"><span>Enter 发送</span><button class="rv-btn rv-primary" type="submit">${icon('send')}发送</button></div></form></main></section>`;
  }

  function sharingPill() {
    if (state.ended) return '';
    const style = state.toolbarPosition ? `style="left:${state.toolbarPosition.x}px;top:${state.toolbarPosition.y}px;transform:none"` : '';
    const grip = `<button class="rv-grip" data-rv-grip aria-label="拖动工具栏" title="拖动工具栏">${icon('grip')}</button>`;
    const classes = `${state.controlled ? '' : 'rv-viewonly'} ${state.paused ? 'rv-paused' : ''}`;
    if (state.compact) return `<div class="rv-sharing-pill rv-compact ${classes}" ${style}>${grip}${button('show-pill', `<i class="rv-dot"></i>${state.paused ? '已暂停' : state.controlled ? '正在被控制' : '共享中'}`, 'monitor', 'rv-quiet', 'aria-label="展开共享工具栏"')}${button('control', '', 'shield', 'rv-icon-only rv-danger', `title="${state.controlled ? '收回控制' : '允许控制'}" aria-label="${state.controlled ? '收回控制' : '允许控制'}"`)}${button('end', '', 'close', 'rv-icon-only rv-danger', 'title="结束协助" aria-label="结束协助"')}</div>`;
    const text = state.paused ? '共享已暂停' : state.controlled ? 'Eason-MBP(M5) 正在控制我的电脑' : '正在向 Eason-MBP(M5) 共享屏幕';
    return `<div class="rv-sharing-pill ${classes}" ${style}>${grip}${icon('control')}<strong>${text}</strong>${button('control', state.controlled ? '收回控制' : '允许控制', 'shield', state.controlled ? 'rv-danger' : '')}${button('pause', state.paused ? '继续共享' : '暂停共享', state.paused ? 'play' : 'pause')}${button('end', '结束', 'close', 'rv-danger')}${button('hide-pill', '隐藏', 'minus', 'rv-pill-hide')}</div>`;
  }

  function sharer() {
    return `<section class="rv-sharer-desktop" aria-label="被控方桌面示意">${sharingPill()}<div class="rv-sharer-app"><div class="rv-window-title">${icon('chat')}XChat<span class="rv-window-caption">− □ ×</span></div>${chat(true)}</div><footer class="rv-desktop-taskbar"><small>被控方桌面示意 · 无本机画面预览</small><span class="rv-windows-mark"><i></i><i></i><i></i><i></i></span>${icon('folder')}${icon('monitor')}<time>16:23</time></footer></section>`;
  }

  function render() {
    root.classList.toggle('rv-detached', state.detached);
    if (state.surface === 'viewer' && document.fullscreenElement?.classList.contains('rv-viewer')) {
      const template = document.createElement('template');
      template.innerHTML = viewer();
      document.fullscreenElement.innerHTML = template.content.firstElementChild.innerHTML;
      const fullButton = root.querySelector('[data-rv-action="fullscreen"]');
      fullButton?.setAttribute('aria-label', '退出全屏');
      const label = fullButton?.querySelector('.rv-tool-label');
      if (label) label.textContent = '退出全屏';
    } else {
      root.innerHTML = `${reviewbar()}<div class="rv-surface">${state.surface === 'viewer' ? viewer() : state.surface === 'sharer' ? sharer() : chat()}</div>`;
    }
    document.title = state.surface === 'viewer' ? 'Eason-Windows · XChat 远程桌面（原型）' : 'XChat · 远程协作 v3 原型';
    requestAnimationFrame(clampPill);
  }

  function clampPill() {
    const pill = root.querySelector('.rv-sharing-pill');
    if (!pill || !state.toolbarPosition) return;
    const scene = pill.parentElement.getBoundingClientRect();
    const rect = pill.getBoundingClientRect();
    const x = Math.max(8, Math.min(scene.width - rect.width - 8, state.toolbarPosition.x));
    const y = Math.max(8, Math.min(scene.height - rect.height - 8, state.toolbarPosition.y));
    state.toolbarPosition = { x, y };
    Object.assign(pill.style, { left: `${x}px`, top: `${y}px`, transform: 'none' });
  }

  function toast(message) {
    clearTimeout(toastTimer);
    root.querySelector('.rv-toast')?.remove();
    const element = document.createElement('div');
    element.className = 'rv-toast';
    element.setAttribute('role', 'status');
    element.textContent = message;
    root.querySelector('.rv-surface').append(element);
    toastTimer = setTimeout(() => element.remove(), 3500);
  }

  function changeSurface(surface) {
    state.surface = surface;
    state.info = false;
    const url = new URL(location.href);
    url.searchParams.set('surface', surface);
    history.replaceState(null, '', url);
    render();
  }

  function openViewer() {
    if (popup && !popup.closed) { popup.focus(); return; }
    const url = new URL(location.href);
    url.searchParams.set('surface', 'viewer');
    url.searchParams.set('detached', '1');
    popup = window.open(url.href, 'xchat-remote-v3-viewer', 'popup=yes,width=1280,height=820,resizable=yes,scrollbars=no');
    toast(popup ? '远程桌面已在独立窗口打开；聊天留在当前窗口。' : '浏览器阻止了弹窗。可切换上方「远程独立窗口」查看原型。');
  }

  async function handleAction(action) {
    if (action === 'popup') { openViewer(); return; }
    if (action === 'chat') {
      if (state.detached && window.opener && !window.opener.closed) { window.opener.focus(); return; }
      changeSurface('chat'); return;
    }
    if (action === 'fullscreen') {
      try {
        if (document.fullscreenElement) await document.exitFullscreen();
        else await root.querySelector('.rv-viewer').requestFullscreen();
      } catch { toast('浏览器未允许全屏，请使用浏览器的全屏入口。'); }
      return;
    }
    if (action === 'info') {
      state.info = !state.info;
      const tools = root.querySelector('.rv-viewer-tools');
      tools?.querySelector('.rv-info-panel')?.remove();
      if (state.info) tools?.insertAdjacentHTML('beforeend', infoPanel());
      tools?.querySelector('[data-rv-action="info"]')?.setAttribute('aria-expanded', String(state.info));
      tools?.querySelector('[data-rv-action="info"]')?.classList.toggle('rv-active', state.info);
      return;
    }
    if (action === 'placeholder') { toast('当前评审聚焦远程协作；此入口沿用现有功能。'); return; }
    if (action === 'control') state.controlled = !state.controlled;
    if (action === 'pause') { state.paused = !state.paused; if (state.paused) state.controlled = false; }
    if (action === 'hide-pill') state.compact = true;
    if (action === 'show-pill') state.compact = false;
    if (action === 'end') { state.ended = true; state.info = false; }
    if (action === 'restart') { state.ended = false; state.paused = false; state.controlled = true; state.compact = false; }
    if (action === 'voice') state.voice = !state.voice;
    if (action === 'mute') state.muted = !state.muted;
    render();
    if (action === 'hide-pill') root.querySelector('[data-rv-action="show-pill"]')?.focus();
    if (action === 'show-pill') root.querySelector('[data-rv-action="hide-pill"]')?.focus();
    if (action === 'voice' && state.voice) toast('语音状态演示，不访问麦克风。');
    if (action === 'control') toast(state.controlled ? '已演示本次会话的控制授权。' : '已收回鼠标和键盘控制，屏幕共享继续。');
  }

  root.addEventListener('click', event => {
    const surface = event.target.closest('[data-rv-surface]')?.dataset.rvSurface;
    if (surface) { changeSurface(surface); return; }
    const infoTab = event.target.closest('[data-rv-info-tab]')?.dataset.rvInfoTab;
    if (infoTab) {
      state.infoTab = infoTab;
      root.querySelector('.rv-info-panel')?.remove();
      root.querySelector('.rv-viewer-tools')?.insertAdjacentHTML('beforeend', infoPanel());
      return;
    }
    const action = event.target.closest('[data-rv-action]')?.dataset.rvAction;
    if (action) void handleAction(action);
  });
  root.addEventListener('change', event => {
    const setting = event.target.dataset.rvChange;
    if (!setting) return;
    state[setting] = event.target.value;
    if (setting === 'scale') root.querySelector('.rv-desktop')?.classList.toggle('rv-actual', state.scale === 'actual');
    if (setting === 'display') {
      const stage = root.querySelector('.rv-viewer-stage');
      if (stage && !state.ended) stage.innerHTML = desktop();
      toast(`已切换到显示器 ${state.display}（示意画面）。`);
    }
    if (setting === 'quality') toast('画质选项为交互演示，不影响真实传输。');
  });
  function sendMessage(form) {
    const textarea = form.querySelector('textarea');
    const text = textarea.value.trim();
    if (!text) return;
    state.messages.push({ text, sharer: state.surface === 'sharer' });
    render();
    const messages = root.querySelector('.rv-messages');
    messages.scrollTop = messages.scrollHeight;
    root.querySelector('textarea')?.focus();
  }
  root.addEventListener('submit', event => {
    const form = event.target.closest('[data-rv-compose]');
    if (!form) return;
    event.preventDefault();
    sendMessage(form);
  });
  root.addEventListener('keydown', event => {
    if (event.target.matches('[data-rv-grip]') && ['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home'].includes(event.key)) {
      event.preventDefault();
      const pill = event.target.closest('.rv-sharing-pill');
      const rect = pill.getBoundingClientRect();
      const scene = pill.parentElement.getBoundingClientRect();
      state.toolbarPosition = event.key === 'Home' ? { x: (scene.width - rect.width) / 2, y: 20 } : {
        x: rect.left - scene.left + (event.key === 'ArrowRight' ? 8 : event.key === 'ArrowLeft' ? -8 : 0),
        y: rect.top - scene.top + (event.key === 'ArrowDown' ? 8 : event.key === 'ArrowUp' ? -8 : 0),
      };
      clampPill();
      return;
    }
    if (event.key === 'Enter' && !event.shiftKey && !event.isComposing && event.target.matches('[data-rv-compose] textarea')) {
      event.preventDefault();
      sendMessage(event.target.closest('form'));
    }
    if (event.key === 'Escape' && state.info) void handleAction('info');
  });
  root.addEventListener('pointerdown', event => {
    const grip = event.target.closest('[data-rv-grip]');
    if (!grip) return;
    const pill = grip.closest('.rv-sharing-pill');
    const scene = pill.parentElement;
    const bounds = scene.getBoundingClientRect();
    const rect = pill.getBoundingClientRect();
    const start = { x: event.clientX, y: event.clientY, left: rect.left - bounds.left, top: rect.top - bounds.top };
    grip.setPointerCapture(event.pointerId);
    const move = next => {
      const x = Math.max(8, Math.min(bounds.width - rect.width - 8, start.left + next.clientX - start.x));
      const y = Math.max(8, Math.min(bounds.height - rect.height - 8, start.top + next.clientY - start.y));
      state.toolbarPosition = { x, y };
      Object.assign(pill.style, { left: `${x}px`, top: `${y}px`, transform: 'none' });
    };
    const end = () => { grip.removeEventListener('pointermove', move); grip.removeEventListener('pointerup', end); grip.removeEventListener('pointercancel', end); };
    grip.addEventListener('pointermove', move);
    grip.addEventListener('pointerup', end);
    grip.addEventListener('pointercancel', end);
  });
  document.addEventListener('fullscreenchange', () => {
    const fullButton = root.querySelector('[data-rv-action="fullscreen"]');
    if (!fullButton) return;
    fullButton.setAttribute('aria-label', document.fullscreenElement ? '退出全屏' : '全屏查看远程桌面');
    fullButton.setAttribute('title', document.fullscreenElement ? '退出全屏' : '全屏');
    const label = fullButton.querySelector('.rv-tool-label');
    if (label) label.textContent = document.fullscreenElement ? '退出全屏' : '全屏';
  });
  window.addEventListener('resize', clampPill);
  render();
})();
