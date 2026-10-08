(() => {
  const parameters = new URLSearchParams(location.search);
  const active = parameters.get('review') === 'video-fullscreen' || document.body.dataset.review === 'android-video';
  const selector = document.querySelector('#desktopReviewScenario');
  if (selector) {
    selector.add(new Option('Android 手机 · 视频全屏与还原', 'android-video'));
    document.addEventListener('change', event => {
      if (event.target !== selector || selector.value !== 'android-video') return;
      event.stopImmediatePropagation();
      location.assign('xchat-android-video-prototype.html');
    }, true);
    if (!active && !parameters.has('review')) {
      const link = document.createElement('a');
      link.id = 'android-video-review-entry';
      link.href = 'xchat-android-video-prototype.html';
      link.textContent = '查看 Android 手机视频设计 →';
      document.body.append(link);
    }
  }
  if (!active) return;
  document.title = 'XChat · Android 手机视频设计';
  const icon = path => `<svg viewBox="0 0 24 24" aria-hidden="true">${path}</svg>`;
  const back = icon('<path d="m14 5-7 7 7 7"/>');
  const expand = icon('<path d="M8 3H3v5m13-5h5v5M3 16v5h5m13-5v5h-5"/>');
  const restore = icon('<path d="M3 8h5V3m8 0v5h5M8 21v-5H3m18 0h-5v5"/>');
  const wifi = icon('<path d="M3 9a15 15 0 0 1 18 0M6 13a10 10 0 0 1 12 0m-9 4a5 5 0 0 1 6 0m-3 3h.01"/>');
  const battery = icon('<rect x="3" y="7" width="17" height="10" rx="2"/><path d="M22 10v4M6 10h11v4H6z"/>');
  const states = [
    {id:'inline',step:'01',title:'对话内播放',expanded:false,note:'点视频卡片下方「全屏」，在手机里放大。'},
    {id:'fullscreen',step:'02',title:'手机全屏播放',expanded:true,note:'画面占满手机可用屏幕，按比例显示。点「还原」或系统返回键退出。'},
    {id:'restored',step:'03',title:'还原回到对话',expanded:false,note:'视频仍停在 00:03，保留播放／暂停状态和聊天位置。'}
  ];
  const phone = state => `<section class="vf-panel" data-panel="${state.id}">
    <div class="vf-panel-head"><span class="vf-step">${state.step}</span><h2>${state.title}</h2></div>
    <div class="vf-phone" data-phone="${state.id}" aria-label="Android 手机：${state.title}">
      <span class="vf-camera"></span>
      <div class="vf-status"><span>12:09</span><span class="vf-status-icons">${wifi}${battery}</span></div>
      <header class="vf-header"><span>${back}</span><div><strong>张小北</strong><small>在线 · 局域网连接</small></div><span class="vf-spacer"></span><span>···</span></header>
      <div class="vf-messages">
        <div class="vf-time">今天 12:06</div>
        <div class="vf-row"><div class="vf-avatar">张</div><div class="vf-stack"><div class="vf-bubble">产品演示发你了，直接打开看吧。</div><small class="vf-timestamp">12:06</small></div></div>
        <div class="vf-row"><div class="vf-avatar">张</div><div class="vf-stack">
          <div class="vf-card">
            <div class="vf-stage">
              <video controls controlslist="nofullscreen" disablepictureinpicture playsinline preload="metadata" src="assets/media-demo/network-preview.mp4" poster="assets/media-demo/network-preview-poster.png" aria-label="产品演示视频"></video>
              <button class="vf-restore" type="button" aria-label="还原视频，返回对话">${restore}<span>还原</span></button>
            </div>
            <div class="vf-foot"><div class="vf-file"><b>产品演示.mp4</b><small>00:08 · 104 KB</small></div><button class="vf-expand" type="button" aria-label="全屏播放视频">${expand}<span>全屏</span></button></div>
          </div>
          <small class="vf-timestamp">12:06</small>
          <p class="vf-status-message" role="status" hidden></p>
        </div></div>
      </div>
      <div class="vf-composer"><div class="vf-input">输入消息…</div><div class="vf-tools"><span>☺　＋</span><button class="vf-send" disabled>发送</button></div></div>
      <div class="vf-nav"></div>
    </div>
    <p class="vf-panel-note">${state.note}<span class="vf-current-state" role="status"></span></p>
    <div class="vf-review-actions"><button type="button" data-review-back>试一下系统返回</button><button type="button" data-review-reset>重看此状态</button></div>
  </section>`;
  const root = document.createElement('section');
  root.id = 'video-fullscreen-review';
  root.setAttribute('aria-label', 'Android 手机视频全屏与还原设计');
  root.innerHTML = `<header class="vf-review-head"><span class="vf-platform">ANDROID · 手机客户端</span><h1>视频全屏与还原</h1><p>三个手机画面展示完整流程。每一张都可以点击体验。</p></header>
    <div class="vf-board">${states.map(phone).join('')}</div>
    <footer class="vf-review-foot">全屏预览始终显示在手机框内。手机横竖屏播放时保持画面比例；切换应用到后台时暂停。</footer>`;
  document.body.append(root);
  const records = states.map(state => {
    const panel = root.querySelector(`[data-panel="${state.id}"]`);
    return {...state,panel,phone:panel.querySelector('.vf-phone'),stage:panel.querySelector('.vf-stage'),video:panel.querySelector('video'),scroller:panel.querySelector('.vf-messages'),savedScroll:0};
  });
  let activeRecord = null;
  function setExpanded(record, expanded, focus = true) {
    if (expanded) record.savedScroll = record.scroller.scrollTop;
    record.stage.classList.toggle('vf-expanded', expanded);
    record.phone.classList.toggle('vf-is-fullscreen', expanded);
    if (!expanded) record.scroller.scrollTop = record.savedScroll;
    if (focus) {
      record.panel.querySelector(expanded ? '.vf-restore' : '.vf-expand').focus({preventScroll:true});
      record.panel.querySelector('.vf-current-state').textContent = expanded ? '当前：手机全屏播放' : '已还原，播放位置保持';
    }
  }
  function restoreRecord(record) {
    if (!record.stage.classList.contains('vf-expanded')) return;
    if (activeRecord === record && history.state?.xchatVideoReview === record.id) history.back();
    else setExpanded(record, false);
  }
  for (const record of records) {
    setExpanded(record, record.expanded, false);
    record.panel.querySelector('.vf-expand').addEventListener('click', () => {
      if (activeRecord && activeRecord !== record) setExpanded(activeRecord, false, false);
      activeRecord = record;
      history.pushState({xchatVideoReview:record.id}, '');
      setExpanded(record, true);
    });
    record.panel.querySelector('.vf-restore').addEventListener('click', () => restoreRecord(record));
    record.panel.querySelector('[data-review-back]').addEventListener('click', () => restoreRecord(record));
    record.panel.querySelector('[data-review-reset]').addEventListener('click', () => {
      record.video.pause();
      record.video.currentTime = 3.25;
      setExpanded(record, record.expanded, false);
      record.panel.querySelector('.vf-current-state').textContent = '';
    });
    record.video.addEventListener('loadedmetadata', () => { record.video.currentTime = 3.25; });
    record.video.addEventListener('play', () => records.forEach(other => { if (other !== record) other.video.pause(); }));
    record.video.addEventListener('error', () => {
      const status = record.panel.querySelector('.vf-status-message');
      status.textContent = '视频暂时无法加载，请刷新页面重试。';
      status.hidden = false;
    });
  }
  window.addEventListener('popstate', () => {
    if (!activeRecord) return;
    setExpanded(activeRecord, false);
    activeRecord = null;
  });
  document.addEventListener('keydown', event => {
    if (event.key !== 'Escape') return;
    const record = activeRecord || records.find(item => item.stage.classList.contains('vf-expanded'));
    if (record) { event.preventDefault(); restoreRecord(record); }
  });
  document.addEventListener('visibilitychange', () => {
    if (document.hidden) records.forEach(record => record.video.pause());
  });
  // A blob keeps this small local fixture seekable on the static review server.
  // File URLs already support seeking and may disallow fetch.
  if (location.protocol !== 'file:') {
    fetch('assets/media-demo/network-preview.mp4').then(response => {
      if (!response.ok) throw new Error('Video fixture unavailable');
      return response.blob();
    }).then(blob => {
      const url = URL.createObjectURL(blob);
      records.forEach(record => { record.video.src = url; });
      window.addEventListener('pagehide', () => URL.revokeObjectURL(url), {once:true});
    }).catch(() => {});
  }
})();
