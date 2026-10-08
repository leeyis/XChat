/* Standalone review surface. All application state is simulated and namespaced. */
(() => {
  'use strict';
  const icons = {
    capture:'<path d="M4 8V4h4M16 4h4v4M20 16v4h-4M8 20H4v-4M8 8h8v8H8z"/>',
    select:'<path d="M4.5 3.5 10 20l3.2-6.5L20 11Z"/>',
    rectangle:'<rect x="4" y="5" width="16" height="14" rx="1.5"/>',
    ellipse:'<ellipse cx="12" cy="12" rx="8" ry="7"/>',
    line:'<path d="m5 19 14-14"/>',
    polyline:'<path d="m4 18 5-12 6 12 5-12"/>',
    arrow:'<path d="M5 19 19 5M10 5h9v9"/>',
    pen:'<path d="m14.5 5.5 4 4M4 20l4.5-1 11-11a2.8 2.8 0 0 0-4-4l-11 11Z"/>',
    marker:'<path d="m7 12 8-8a1.4 1.4 0 0 1 2 0l3 3a1.4 1.4 0 0 1 0 2l-8 8Z"/><path d="m8 13-3 3v3h3l3-3"/>',
    text:'<path d="M4 5h16M12 5v14M8 19h8"/>',
    mosaic:'<rect x="4" y="4" width="6" height="6" rx=".75"/><rect x="14" y="4" width="6" height="6" rx=".75"/><rect x="4" y="14" width="6" height="6" rx=".75"/><rect x="14" y="14" width="6" height="6" rx=".75"/>',
    blur:'<circle cx="12" cy="12" r="8" stroke-dasharray=".01 4.18"/><circle cx="12" cy="12" r="3.5"/>',
    eraser:'<path d="M13.5 4.5a2.1 2.1 0 0 1 3 0l3 3a2.1 2.1 0 0 1 0 3L11 19H7l-2.5-2.5a2.1 2.1 0 0 1 0-3Z"/><path d="m8.5 9.5 6 6M11 19h9"/>',
    undo:'<path d="m9 5-5 5 5 5M4 10h10a5 5 0 0 1 0 10h-2"/>',
    redo:'<path d="m9 5-5 5 5 5M4 10h10a5 5 0 0 1 0 10h-2" transform="translate(24 0) scale(-1 1)"/>',
    pin:'<path d="M15 9V4H9v5a3 3 0 0 1-3 3v3h12v-3a3 3 0 0 1-3-3ZM12 15v6" transform="rotate(45 12 12)"/>',
    copy:'<rect x="8" y="8" width="12" height="13" rx="2"/><path d="M15 8V3H3v13h5"/>',
    save:'<path d="M5 4h12l3 3v12a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1ZM8 4v5h8V4M8 20v-7h8v7"/>',
    send:'<path d="m4 4 16 8-16 8 3-8ZM7 12h13"/>',
    close:'<path d="m6 6 12 12M18 6 6 18"/>',
    check:'<path d="m5 13 4 4L19 7"/>',
    history:'<path d="M4 9a9 9 0 1 1 0 8M4 3v6h6M12 7v6l4 2"/>',
    settings:'<path d="M4 6h16M4 12h16M4 18h16"/><circle cx="8" cy="6" r="2" fill="currentColor"/><circle cx="16" cy="12" r="2" fill="currentColor"/><circle cx="10" cy="18" r="2" fill="currentColor"/>',
    help:'<circle cx="12" cy="12" r="9"/><path d="M9 9a3 3 0 1 1 4 3c-1 1-1 1-1 2M12 17h.01"/>',
    more:'<circle cx="5" cy="12" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/>',
    hide:'<path d="M5 12h14"/>',
    move:'<path d="M12 3v18M3 12h18M9 6l3-3 3 3M9 18l3 3 3-3M6 9l-3 3 3 3M18 9l3 3-3 3"/>',
    trash:'<path d="M4 6h16M9 6V3h6v3M6 6l1 15h10l1-15M10 10v7M14 10v7"/>'
  };
  const svg = name => '<svg viewBox="0 0 24 24" aria-hidden="true">' + (icons[name] || icons.capture) + '</svg>';
  const esc = value => String(value).replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
  const clamp = (n,a,b) => Math.max(a, Math.min(b,n));
  const clone = value => JSON.parse(JSON.stringify(value));
  const uid = () => 'sr-' + Date.now().toString(36) + '-' + Math.random().toString(36).slice(2,7);
  const toolsList = [['select','调整选区'],['rectangle','矩形'],['ellipse','椭圆'],['line','直线'],['polyline','折线'],['arrow','箭头'],['pen','画笔'],['marker','荧光笔'],['text','文字'],['mosaic','马赛克'],['blur','模糊'],['eraser','橡皮擦']];
  const colors = ['#ea5455','#efad35','#18ac71','#2c87ca','#263830','#ffffff'];
  const toolSizes={rectangle:2,ellipse:2,line:2,polyline:2,arrow:3,pen:3,marker:18,mosaic:12,blur:8,eraser:24};
  function sizeOptions(tool){return tool==='text'?{min:10,max:96,step:2,label:'字号'}:tool==='mosaic'?{min:4,max:48,step:2,label:'颗粒'}:tool==='blur'?{min:2,max:32,step:1,label:'模糊'}:['marker','eraser'].includes(tool)?{min:4,max:80,step:2,label:'笔触'}:{min:1,max:32,step:1,label:'线宽'};}
  const button = (action, label, icon, extra) => '<button type="button" data-sr-action="'+action+'" class="sr-button '+(extra||'')+'">'+(icon?svg(icon):'')+label+'</button>';
  const root = document.createElement('section');
  root.id = 'snipasteReview'; root.className = 'sr-root'; root.hidden = true;
  root.setAttribute('aria-label','截图与贴图交互原型');
  root.innerHTML = [
    '<header class="sr-header"><div class="sr-brand"><span class="sr-brand-mark">'+svg('capture')+'</span><div><strong>XChat <span style="font-weight:400">截图</span></strong><small>CAPTURE & KEEP</small></div></div><span class="sr-review-tag">基础功能 · 交互原型</span><div class="sr-header-spacer"></div>',
    '<button class="sr-top-button" data-sr-action="manager">'+svg('history')+'<span>历史与贴图</span></button><button class="sr-top-button" data-sr-action="settings">'+svg('settings')+'<span>设置</span></button><button class="sr-top-button" data-sr-action="help">'+svg('help')+'<span>评审说明</span></button><button class="sr-top-button sr-close-review" data-sr-action="leave">返回聊天</button></header>',
    '<div class="sr-workbar">'+button('capture','开始截图 <kbd>F1</kbd>','capture','primary')+button('paste','贴图 <kbd>F3</kbd>','pin')+button('sources','更多来源',null)+button('text-demo','文字与选区检查','text')+'<div class="sr-screens"><button data-sr-density="1" class="active">100%</button><button data-sr-density="1.25">125%</button><button data-sr-density="1.5">150%</button><button data-sr-density="2">200%</button></div><span class="sr-workbar-note">像素密度模拟 · <b id="srPinCount">0 张贴图</b></span></div>',
    '<div class="sr-desktop" id="srDesktop"><canvas id="srScene" aria-label="演示桌面，不是真实屏幕"></canvas><span class="sr-screen-label">DESKTOP 01 / 演示桌面 · 不读取真实屏幕</span><div id="srPins"></div>',
    '<div class="sr-rescue" id="srRescue" hidden><span></span><button data-sr-action="rescue">恢复鼠标交互</button></div>',
    '<div class="sr-capture" id="srCapture" hidden><canvas id="srCanvas" aria-label="截图选区与标注画布"></canvas><div class="sr-hover-region" id="srHover" hidden><span class="sr-hover-label"></span></div><div class="sr-selection" id="srSelection" hidden><span class="sr-dimensions"></span>'+[ [0,0],[.5,0],[1,0],[1,.5],[1,1],[.5,1],[0,1],[0,.5] ].map(p=>'<i class="sr-handle" style="left:'+p[0]*100+'%;top:'+p[1]*100+'%"></i>').join('')+'</div>',
    '<div class="sr-capture-hint" id="srCaptureHint"></div><div class="sr-tools" id="srTools" hidden><div class="sr-tool-row">'+toolsList.map(t=>'<button type="button" class="sr-tool" data-sr-tool="'+t[0]+'" title="'+t[1]+'" aria-label="'+t[1]+'">'+svg(t[0])+'</button>').join('')+'<span class="sr-separator"></span>'+[['undo','撤销 Ctrl+Z'],['redo','重做 Ctrl+Y']].map(t=>'<button type="button" class="sr-tool" data-sr-action="'+t[0]+'" title="'+t[1]+'" aria-label="'+t[1]+'">'+svg(t[0])+'</button>').join('')+'<span class="sr-separator"></span>'+[['pin','贴图 Ctrl+T'],['save','保存 Ctrl+S']].map(t=>'<button type="button" class="sr-tool" data-sr-action="'+t[0]+'" title="'+t[1]+'" aria-label="'+t[1]+'">'+svg(t[0])+'</button>').join('')+'<button type="button" class="sr-tool" data-sr-action="draft" aria-label="加入聊天草稿" title="加入聊天草稿，不自动发送">'+svg('send')+'</button><button class="sr-tool" data-sr-action="cancel" title="取消 Esc" aria-label="取消截图">'+svg('close')+'</button><button class="sr-tool sr-finish" data-sr-action="copy" title="完成 · 复制截图" aria-label="复制并完成截图">'+svg('check')+'</button></div>',
    '<div class="sr-style-row"><span class="sr-tool-name" id="srToolName">调整选区</span><span id="srSelectionHint">拖动调整 · 方向键微调</span><div id="srParameters" class="sr-parameters"><div class="sr-swatches">'+colors.map(c=>'<button class="sr-swatch" style="background:'+c+'" data-sr-color="'+c+'" aria-label="颜色 '+c+'"></button>').join('')+'</div><input type="color" id="srColor" value="#ea5455" aria-label="自定义颜色"><span class="sr-property-separator"></span><span id="srSizeLabel">线宽</span><span id="srStrokeSample" class="sr-stroke-sample"><i></i></span><input type="range" id="srSize" min="1" max="32" value="3" aria-label="线宽"><output id="srSizeValue">3 px</output><select id="srFont" aria-label="字体" hidden><option value="Microsoft YaHei">微软雅黑</option><option value="SimSun">宋体</option><option value="Consolas">等宽</option></select><span class="sr-wheel-hint" title="向上滚动增大，向下滚动减小">滚轮调节</span></div><button class="sr-selection-reset" data-sr-action="reselect">重新框选</button></div></div>',
    '<div class="sr-magnifier" id="srMagnifier" hidden><canvas width="128" height="96"></canvas><p></p></div><textarea class="sr-text-editor" id="srText" aria-label="编辑标注文字，回车换行，点击外部完成，Delete 删除标注" spellcheck="false" wrap="off" hidden></textarea><div class="sr-text-frame" id="srTextFrame" hidden aria-label="拖动文字边框">'+['top','right','bottom','left'].map(side=>'<span class="sr-text-edge '+side+'" data-sr-text-edge="'+side+'" title="拖动边框移动文字"></span>').join('')+'</div><div class="sr-size-preview" id="srSizePreview" hidden><i></i><span></span></div></div>',
    '<aside class="sr-panel" id="srPanel" hidden aria-label="截图设置与记录"></aside><div class="sr-pin-menu" id="srMenu" hidden role="menu"></div><div class="sr-dialog-backdrop" id="srDialog" hidden></div><div class="sr-countdown" id="srCountdown" hidden><strong></strong><button data-sr-action="cancel-delay">取消</button></div><div class="sr-toast" id="srToast" role="status" aria-live="polite"></div></div>',
    '<footer class="sr-bottom"><span><i></i>本地交互演示</span><span>F1 截图 · F3 贴图 · 滚轮缩放 · 右键更多操作</span><span id="srStatus">先试试“文字与选区检查”</span></footer>'
  ].join('');
  document.body.append(root);
  const $ = id => root.querySelector('#'+id);
  const desktop=$('srDesktop'), scene=$('srScene'), layer=$('srCapture'), canvas=$('srCanvas'), ctx=canvas.getContext('2d'), editor=$('srText');
  const STORE='xchat.snipaste-review.v2';
  const state={active:false,initialized:false,density:1,pins:[],history:[],group:'默认',selected:null,tool:'select',color:colors[0],size:4,fontSize:28,font:'Microsoft YaHei',selection:null,base:null,ops:[],undo:[],redo:[],draft:null,edit:null,gesture:null,hover:null,detectMode:'window',showTools:true,capturing:false,editPin:null,panel:null,tab:'pins',lastRegion:null,clipboard:null,failCopy:false,denied:false,delayTimer:null,settings:{delay:0,detect:true,magnifier:true,chat:true,remember:true,limit:12,theme:'light',preset:'snipaste',captureCursor:false}};
  function toast(message,error=false,action=null){const el=$('srToast');el.textContent=message;if(action){const b=document.createElement('button');b.textContent=action.label;b.type='button';b.addEventListener('click',()=>{action.run();el.classList.remove('show');});el.append(b);}el.className='sr-toast show'+(error?' error':'');clearTimeout(state.toastTimer);state.toastTimer=setTimeout(()=>el.classList.remove('show'),action?7000:3500);}
  function persist(){if(!state.settings.remember)return;try{localStorage.setItem(STORE,JSON.stringify({v:2,pins:state.pins,history:state.history,settings:state.settings,group:state.group}));}catch{toast('原型存储空间不足；当前内容仍在，刷新后可能无法恢复。',true);}}
  function loadSaved(){try{const saved=JSON.parse(localStorage.getItem(STORE)||'null');if(saved?.v!==2)return false;state.pins=(saved.pins||[]).filter(p=>typeof p.src==='string'&&p.src.startsWith('data:image/')).slice(0,24).map(p=>({...p,shadow:p.shadow!==false}));state.history=(saved.history||[]).filter(p=>p.src?.startsWith('data:image/')).slice(0,20);Object.assign(state.settings,saved.settings||{});state.group=saved.group==='设计参考'?'设计参考':'默认';return true;}catch{return false;}}
  function makeCanvas(w,h){const c=document.createElement('canvas');c.width=Math.max(1,Math.round(w));c.height=Math.max(1,Math.round(h));return c;}
  function rr(c,x,y,w,h,r,fill){c.beginPath();c.roundRect(x,y,w,h,r);c.fillStyle=fill;c.fill();}
  function txt(c,value,x,y,size=16,color='#2d493b',weight=400,font='Microsoft YaHei'){c.font=weight+' '+size+'px "'+font+'"';c.fillStyle=color;c.textBaseline='alphabetic';c.fillText(value,x,y);}
  function drawScene(){
    scene.width=Math.max(1,Math.round(desktop.clientWidth*state.density));scene.height=Math.max(1,Math.round(desktop.clientHeight*state.density));
    const c=scene.getContext('2d'),scale=Math.min(scene.width/1600,scene.height/900);
    state.sceneMap={scale,x:(scene.width-1600*scale)/2,y:(scene.height-900*scale)/2};
    c.fillStyle='#e8eee8';c.fillRect(0,0,scene.width,scene.height);c.translate(state.sceneMap.x,state.sceneMap.y);c.scale(scale,scale);
    const bg=c.createLinearGradient(0,0,1600,900);bg.addColorStop(0,'#e9efe7');bg.addColorStop(1,'#cbdcd1');c.fillStyle=bg;c.fillRect(0,0,1600,900);
    c.fillStyle='#f5f8f2';c.beginPath();c.ellipse(80,680,450,600,-.7,0,7);c.fill();
    c.save();c.shadowColor='#254f3820';c.shadowBlur=50;c.shadowOffsetY=15;rr(c,135,65,980,750,14,'#fff');c.restore();
    rr(c,135,65,980,50,14,'#f5f7f4');c.fillStyle='#e2e8e0';c.fillRect(135,113,980,1);
    ['#ec9a8e','#e7c580','#98c4a2'].forEach((col,i)=>{c.fillStyle=col;c.beginPath();c.arc(159+i*20,90,5,0,7);c.fill();});
    txt(c,'工作空间 / 项目周报',241,96,13,'#728473');txt(c,'仅团队可见',980,96,11,'#90a08f');
    c.fillStyle='#fafbf8';c.fillRect(135,115,190,698);txt(c,'WORKSPACE',156,157,11,'#9aa899',500,'Consolas');
    ['项目概览','十月协作进展','设计资料','团队文档','归档'].forEach((s,i)=>{if(i===1)rr(c,148,208,165,36,5,'#e7f2e7');txt(c,s,169,195+i*37,14,i===1?'#228557':'#8a9889',i===1?600:400);});
    txt(c,'OCTOBER / 2026',374,178,12,'#8c9e8f',400,'Consolas');txt(c,'十月协作进展',374,235,40,'#284b37',500,'SimSun');
    txt(c,'设计、开发与交付，保持同步。',376,274,15,'#8c9a8e');
    [['24','已完成任务'],['8','进行中'],['96%','按时交付']].forEach((v,i)=>{rr(c,375+i*225,315,206,108,8,'#f5f8f3');txt(c,v[0],395+i*225,364,31,'#38604a',500,'Consolas');txt(c,v[1],396+i*225,395,12,'#98a394');});
    txt(c,'本周完成趋势',376,474,16,'#445d4b',600);txt(c,'10.01 — 10.07',907,474,11,'#8d9c8f',400,'Consolas');
    [0,1,2,3].forEach(i=>{c.strokeStyle='#eaf0e6';c.beginPath();c.moveTo(378,520+i*52);c.lineTo(1043,520+i*52);c.stroke();});
    const pts=[[380,654],[490,626],[600,639],[710,581],[820,595],[930,542],[1040,508]];
    c.beginPath();pts.forEach((p,i)=>i?c.lineTo(...p):c.moveTo(...p));c.lineTo(1040,690);c.lineTo(380,690);c.closePath();c.fillStyle='#e7f3e8';c.fill();
    c.beginPath();pts.forEach((p,i)=>i?c.lineTo(...p):c.moveTo(...p));c.lineWidth=3;c.strokeStyle='#439367';c.stroke();
    pts.forEach((p,i)=>{c.beginPath();c.arc(...p,4,0,7);c.fillStyle='#439367';c.fill();txt(c,'周'+['一','二','三','四','五','六','日'][i],p[0]-10,718,11,'#9aaa99');});
    c.fillStyle='#e5ede3';c.fillRect(375,748,669,1);txt(c,'更新于今天 09:41',376,780,11,'#99a697');
    c.save();c.shadowColor='#284c392a';c.shadowBlur=35;c.shadowOffsetY=10;rr(c,1160,155,345,565,13,'#fcfefb');c.restore();
    txt(c,'设计讨论群',1183,194,17,'#42604c',600);txt(c,'4 位成员',1183,218,11,'#91a28f');c.fillStyle='#e9eee7';c.fillRect(1160,239,345,1);
    rr(c,1182,270,31,31,8,'#dbe8dd');txt(c,'林',1189,291,14,'#739679');
    rr(c,1222,270,250,86,9,'#eef3ec');txt(c,'本周的周报已更新。',1236,297,13);txt(c,'截图圈一下需要讨论的位置。',1236,325,13);
    rr(c,1240,390,240,67,9,'#d8edda');txt(c,'收到，我把重点标出来。',1253,420,13);
    txt(c,'09:41',1310,487,10,'#9aaa97');rr(c,1182,520,31,31,8,'#ecdfc4');txt(c,'周',1189,541,14,'#a89772');
    rr(c,1222,520,247,64,9,'#eef3ec');txt(c,'贴在屏幕边上，方便对照。',1234,548,12);
    c.fillStyle='#e8eee5';c.fillRect(1160,620,345,1);txt(c,'输入消息…',1182,659,12,'#b0bca9');rr(c,1425,669,57,28,5,'#4c9c70');txt(c,'发送',1440,687,11,'#fff');
  }
  function sampleImage(){const c=makeCanvas(430,220),g=c.getContext('2d');g.fillStyle='#fcfdf8';g.fillRect(0,0,430,220);txt(g,'界面色彩参考',24,40,18,'#435c49',600);['#18ac71','#e8eee5','#2d493b'].forEach((col,i)=>{rr(g,24+i*134,65,114,80,6,col);txt(g,col.toUpperCase(),24+i*134,174,13,'#7c8c78',400,'Consolas');});txt(g,'XChat / DESIGN NOTES',24,203,10,'#a3af9c',400,'Consolas');return c;}
  function regionCanvas(base,r){const c=makeCanvas(r.w,r.h);c.getContext('2d').drawImage(base,r.x,r.y,r.w,r.h,0,0,c.width,c.height);return c;}
  function sourceFromCanvas(c,title){return {src:c.toDataURL('image/png'),w:c.width,h:c.height,title};}
  function enter(){root.hidden=false;state.active=true;drawScene();if(!state.initialized){state.initialized=true;const restored=loadSaved();state.clipboard=sourceFromCanvas(sampleImage(),'界面色彩参考');if(!restored){addPin(state.clipboard,false,{x:45,y:40,scale:.68});state.selected=null;}}root.classList.toggle('sr-dark',state.settings.theme==='dark');renderPins();$('srStatus').textContent='原型保存于本机 · 基础功能对齐';}
  function leave(){cancelCapture();closePanel();closeDialog();root.hidden=true;state.active=false;const s=document.getElementById('desktopReviewScenario');if(s){s.value='changed';s.dispatchEvent(new Event('change'));}}
  const baselineCache=new Map();
  function textLayout(op){
    const size=op.fontSize, font=op.font||'Microsoft YaHei', key=font+'/'+size;
    if(!baselineCache.has(key)){
      const probe=document.createElement('div');
      Object.assign(probe.style,{position:'fixed',left:'-10000px',top:'0',visibility:'hidden',font:'400 '+size+'px "'+font+'"',lineHeight:(size*1.35)+'px',whiteSpace:'pre',padding:'0',border:'0'});
      probe.append(document.createTextNode('Hg中文'));const marker=document.createElement('i');
      Object.assign(marker.style,{display:'inline-block',width:'0',height:'0',padding:'0',border:'0',verticalAlign:'baseline'});probe.append(marker);document.body.append(probe);
      baselineCache.set(key,marker.getBoundingClientRect().top-probe.getBoundingClientRect().top);probe.remove();
    }
    ctx.save();ctx.font='400 '+size+'px "'+font+'"';
    const lines=String(op.text||'').split('\n'), width=Math.max(1,...lines.map(l=>ctx.measureText(l).width));ctx.restore();
    return {baseline:baselineCache.get(key),lineHeight:size*1.35,width,height:lines.length*size*1.35,lines};
  }
  function scenePoint(x,y){const m=state.sceneMap;return {x:m.x+x*m.scale,y:m.y+y*m.scale};}
  function view(){const w=desktop.clientWidth,h=desktop.clientHeight,s=Math.min(w/canvas.width,h/canvas.height),ox=(w-canvas.width*s)/2,oy=(h-canvas.height*s)/2;Object.assign(canvas.style,{width:canvas.width*s+'px',height:canvas.height*s+'px',left:ox+'px',top:oy+'px'});return {sx:s,sy:s,ox,oy,w,h};}
  function point(event){const b=canvas.getBoundingClientRect();return {x:clamp((event.clientX-b.left)*canvas.width/b.width,0,canvas.width),y:clamp((event.clientY-b.top)*canvas.height/b.height,0,canvas.height)};}
  const inside=(p,r)=>r&&p.x>=r.x&&p.y>=r.y&&p.x<=r.x+r.w&&p.y<=r.y+r.h;
  const rectBetween=(a,b)=>({x:Math.min(a.x,b.x),y:Math.min(a.y,b.y),w:Math.abs(b.x-a.x),h:Math.abs(b.y-a.y)});
  function normalized(r){const x=clamp(r.x,0,canvas.width-1),y=clamp(r.y,0,canvas.height-1);return {x,y,w:clamp(r.x+r.w,x+1,canvas.width)-x,h:clamp(r.y+r.h,y+1,canvas.height)-y};}
  function drawOperation(g,o){
    g.save();g.lineCap='round';g.lineJoin='round';g.lineWidth=o.size;g.strokeStyle=o.color;g.fillStyle=o.color;
    if(o.tool==='text'){
      const l=textLayout(o);g.font='400 '+o.fontSize+'px "'+o.font+'"';g.textBaseline='alphabetic';
      l.lines.forEach((line,i)=>g.fillText(line,o.x,o.y+l.baseline+i*l.lineHeight));
    } else if(o.tool==='mosaic'||o.tool==='blur'){
      const r=rectBetween(o.start,o.end);if(r.w<1||r.h<1){g.restore();return;}
      const base=regionCanvas(state.base,r);g.beginPath();g.rect(r.x,r.y,r.w,r.h);g.clip();
      if(o.tool==='mosaic'){
        const tiny=makeCanvas(r.w/(o.size||9*state.density),r.h/(o.size||9*state.density));tiny.getContext('2d').drawImage(base,0,0,tiny.width,tiny.height);g.imageSmoothingEnabled=false;g.drawImage(tiny,r.x,r.y,r.w,r.h);
      }else{g.filter='blur('+(o.size||7*state.density)+'px)';g.drawImage(base,r.x,r.y,r.w,r.h);}
    } else if(o.tool==='eraser'){
      const points=o.points||[];for(let i=0;i<points.length;i++){const p=points[i],prev=points[i-1]||p,dist=Math.hypot(p.x-prev.x,p.y-prev.y),count=Math.max(1,Math.ceil(dist/Math.max(1,o.size/3)));for(let j=0;j<=count;j++){g.save();g.beginPath();g.arc(prev.x+(p.x-prev.x)*j/count,prev.y+(p.y-prev.y)*j/count,o.size/2,0,7);g.clip();g.drawImage(state.base,0,0);g.restore();}}
    } else if(o.points){
      if(o.tool==='marker'){g.globalAlpha=.3;g.lineWidth=o.size;g.lineCap='butt';}
      g.beginPath();o.points.forEach((p,i)=>i?g.lineTo(p.x,p.y):g.moveTo(p.x,p.y));if(o.points.length===1)g.lineTo(o.points[0].x+.01,o.points[0].y);g.stroke();
    } else {
      const a=o.start,b=o.end,r=rectBetween(a,b);
      if(o.tool==='rectangle')g.strokeRect(r.x,r.y,r.w,r.h);
      else if(o.tool==='ellipse'){g.beginPath();g.ellipse(r.x+r.w/2,r.y+r.h/2,r.w/2,r.h/2,0,0,7);g.stroke();}
      else {g.beginPath();g.moveTo(a.x,a.y);g.lineTo(b.x,b.y);g.stroke();if(o.tool==='arrow'){const angle=Math.atan2(b.y-a.y,b.x-a.x),head=Math.max(12*state.density,o.size*3);g.beginPath();g.moveTo(b.x,b.y);g.lineTo(b.x-head*Math.cos(angle-.4),b.y-head*Math.sin(angle-.4));g.moveTo(b.x,b.y);g.lineTo(b.x-head*Math.cos(angle+.4),b.y-head*Math.sin(angle+.4));g.stroke();}}
    }g.restore();
  }
  function positionBox(el,r){const d=view();Object.assign(el.style,{left:d.ox+r.x*d.sx+'px',top:d.oy+r.y*d.sy+'px',width:r.w*d.sx+'px',height:r.h*d.sy+'px'});}
  function paint(){
    if(!state.capturing)return;view();
    ctx.clearRect(0,0,canvas.width,canvas.height);ctx.drawImage(state.base,0,0);ctx.fillStyle='#11231b80';ctx.fillRect(0,0,canvas.width,canvas.height);
    const r=state.selection,sel=$('srSelection');sel.hidden=!r;
    if(r){ctx.save();ctx.beginPath();ctx.rect(r.x,r.y,r.w,r.h);ctx.clip();ctx.drawImage(state.base,0,0);
      state.ops.forEach(o=>{if(o.id!==state.edit?.id&&o.id!==state.draft?.id)drawOperation(ctx,o);});
      if(state.draft)drawOperation(ctx,state.draft);
      if(state.edit?.relocated)drawOperation(ctx,{...state.edit,text:editor.value});
      ctx.restore();positionBox(sel,r);sel.classList.toggle('near-top',r.y*view().sy+view().oy<33);
      sel.querySelector('.sr-dimensions').textContent=Math.round(r.w)+' × '+Math.round(r.h)+' px';
      sel.dataset.x=String(r.x);sel.dataset.y=String(r.y);sel.dataset.width=String(r.w);sel.dataset.height=String(r.h);
    }
    $('srCaptureHint').hidden=!!r;
    $('srCaptureHint').innerHTML='拖动框选 · 单击识别区域 <kbd>Tab</kbd> '+(state.detectMode==='window'?'窗口':'元素')+'识别 · <kbd>Esc</kbd> 取消';
    const bar=$('srTools');bar.hidden=!r||!state.showTools||!!state.gesture&&state.gesture.kind==='select';
    if(!bar.hidden){
      const d=view(),bw=bar.offsetWidth,bh=bar.offsetHeight;let left=clamp(d.ox+(r.x+r.w)*d.sx-bw,12,d.w-bw-12),top=d.oy+(r.y+r.h)*d.sy+12;
      if(top+bh>d.h-12)top=d.oy+r.y*d.sy-bh-12;if(top<12)top=clamp(d.oy+(r.y+r.h)*d.sy-bh-12,12,d.h-bh-12);
      bar.style.left=left+'px';bar.style.top=top+'px';
    }
    root.querySelector('[data-sr-action="undo"]').disabled=!state.undo.length;
    root.querySelector('[data-sr-action="redo"]').disabled=!state.redo.length;
    root.querySelector('[data-sr-action="draft"]').disabled=!state.settings.chat;
    root.querySelector('[data-sr-action="draft"]').title=state.settings.chat?'加入聊天草稿，不自动发送':'请先选择一个会话';
    if(state.edit)positionText();
    canvas.dataset.annotations=String(state.ops.length);canvas.dataset.textCount=String(state.ops.filter(o=>o.tool==='text').length);
  }
  function updateTool(){
    root.querySelectorAll('[data-sr-tool]').forEach(b=>{b.classList.toggle('active',b.dataset.srTool===state.tool);b.setAttribute('aria-pressed',String(b.dataset.srTool===state.tool));});
    root.querySelectorAll('[data-sr-color]').forEach(b=>b.classList.toggle('active',b.dataset.srColor===state.color));
    $('srToolName').textContent=toolsList.find(t=>t[0]===state.tool)?.[1]||'调整选区';
    const text=state.tool==='text',spec=sizeOptions(state.tool),size=text?state.fontSize:state.size;$('srSizeLabel').textContent=spec.label;$('srSize').max=spec.max;$('srSize').min=spec.min;$('srSize').step=spec.step;$('srSize').value=size;$('srSize').setAttribute('aria-label',spec.label);$('srSizeValue').textContent=size+' px';
    $('srParameters').hidden=state.tool==='select';$('srSelectionHint').hidden=state.tool!=='select';$('srStrokeSample').hidden=text;$('srStrokeSample').firstElementChild.style.height=clamp(size,1,16)+'px';$('srStrokeSample').firstElementChild.style.background=state.color;
    canvas.style.cursor=state.tool==='text'?'text':state.tool==='select'?'crosshair':'crosshair';
    $('srFont').hidden=!text;$('srFont').value=state.font;$('srColor').value=state.color;
  }
  function snapshot(){state.undo.push(clone(state.ops));state.undo=state.undo.slice(-80);state.redo=[];}
  function commitOperation(op){snapshot();const i=state.ops.findIndex(o=>o.id===op.id);if(i<0)state.ops.push(clone(op));else state.ops[i]=clone(op);state.draft=null;paint();}
  function changeTool(tool){commitText();finishPolyline();if(toolSizes[state.tool]!=null)toolSizes[state.tool]=state.size;state.tool=tool;if(toolSizes[tool]!=null)state.size=toolSizes[tool];state.wheelDelta=0;updateTool();paint();}
  function openText(op,p){
    commitText();state.edit=clone(op||{id:uid(),tool:'text',x:p.x,y:p.y,text:'',color:state.color,fontSize:state.fontSize*state.density,font:state.font});
    state.edit.original=op?clone(op):null;state.tool='text';state.color=state.edit.color;state.font=state.edit.font;state.fontSize=Math.round(state.edit.fontSize/state.density);
    editor.value=state.edit.text;editor.hidden=false;$('srTextFrame').hidden=false;updateTool();positionText();paint();editor.focus({preventScroll:true});editor.setSelectionRange(editor.value.length,editor.value.length);
  }
  function positionText(){
    if(!state.edit)return;const o=state.edit,d=view(),l=textLayout({...o,text:editor.value||'输入文字'}),wanted=Math.max(o.fontSize*5,l.width+8/d.sx),width=Math.min(wanted,(d.w-32)/d.sx),height=Math.min(Math.max(l.height,o.fontSize*1.35),Math.max(o.fontSize*1.35,(d.h-150)/d.sy));
    const actualX=d.ox+o.x*d.sx,actualY=d.oy+o.y*d.sy,left=clamp(actualX,14,d.w-width*d.sx-14),top=clamp(actualY,14,d.h-height*d.sy-14);
    o.relocated=Math.abs(left-actualX)>1||Math.abs(top-actualY)>1;
    Object.assign(editor.style,{left:left+'px',top:top+'px',width:width+'px',height:height+'px',fontFamily:'"'+o.font+'"',fontSize:o.fontSize+'px',fontWeight:'400',lineHeight:l.lineHeight+'px',color:o.color,transform:'scale('+d.sx+','+d.sy+')',transformOrigin:'top left'});
    const frame=$('srTextFrame');Object.assign(frame.style,{left:left-5+'px',top:top-4+'px',width:width*d.sx+10+'px',height:height*d.sy+8+'px'});frame.classList.toggle('relocated',o.relocated);
    editor.classList.toggle('relocated',o.relocated);
    editor.dataset.anchorX=String(o.x);editor.dataset.anchorY=String(o.y);editor.dataset.fontPixels=String(o.fontSize);
    const bar=$('srTools');
    if(!bar.hidden){
      const x=parseFloat(bar.style.left),y=parseFloat(bar.style.top),editTop=top-8,editRight=left+width*d.sx+8;
      if(x<editRight+10&&x+bar.offsetWidth>left-10&&y<top+height*d.sy+10&&y+bar.offsetHeight>editTop-10){
        const above=editTop-bar.offsetHeight-12,below=top+height*d.sy+12;
        bar.style.top=(above>=7?above:Math.min(below,d.h-bar.offsetHeight-7))+'px';
      }
    }
  }
  function commitText(){
    if(!state.edit)return;const o={...state.edit,text:editor.value};delete o.original;delete o.relocated;
    const original=state.edit.original;state.edit=null;editor.hidden=true;$('srTextFrame').hidden=true;
    if(!o.text.trim()&&original){snapshot();state.ops=state.ops.filter(item=>item.id!==o.id);state.selectedText=null;paint();}
    else if(o.text.trim()){if(JSON.stringify(o)!==JSON.stringify(original))commitOperation(o);else paint();state.selectedText=o.id;}else paint();
  }
  function cancelText(){state.edit=null;editor.hidden=true;$('srTextFrame').hidden=true;paint();}
  function deleteText(){if(!state.edit)return;const id=state.edit.id;if(editor.value.trim())commitText();if(state.ops.some(o=>o.id===id)){snapshot();state.ops=state.ops.filter(o=>o.id!==id);}state.selectedText=null;cancelText();}
  function textAt(p){return [...state.ops].reverse().find(o=>{if(o.tool!=='text')return false;const l=textLayout(o);return inside(p,{x:o.x,y:o.y,w:Math.max(l.width,12*state.density),h:l.height});});}
  function finishPolyline(){if(state.draft?.tool==='polyline'){if(state.draft.points.length>1)commitOperation(state.draft);else state.draft=null;paint();}}
  function beginCapture(base,region=null){
    closePanel();closeDialog();root.classList.add('sr-is-capturing');$('srToast').classList.remove('show');$('srMenu').hidden=true;state.capturing=true;state.selection=region;state.base=base;state.ops=[];state.undo=[];state.redo=[];state.edit=null;state.selectedText=null;state.draft=null;state.gesture=null;state.hover=null;state.tool='select';state.showTools=true;editor.hidden=true;$('srTextFrame').hidden=true;layer.hidden=false;
    canvas.width=base.width;canvas.height=base.height;$('srHover').hidden=true;updateTool();paint();$('srStatus').textContent='截图中 · 所有坐标按原图像素计算';
  }
  function startCapture(mode='normal'){
    if(mode==='normal'&&state.capturing){if(state.edit)editor.focus({preventScroll:true});toast('当前截图仍在编辑，完成或取消后再开始。');return;}
    if(state.denied){state.denied=false;toast('屏幕录制权限未授权（模拟）。请授权后重试。',true);return;}
    cancelCapture();state.editPin=null;
    const run=()=>{root.classList.add('sr-is-capturing');drawScene();let base=makeCanvas(scene.width,scene.height);base.getContext('2d').drawImage(scene,0,0);if(mode==='whiteboard'){const g=base.getContext('2d');g.fillStyle='#fff';g.fillRect(0,0,base.width,base.height);}
      if(state.settings.captureCursor&&mode!=='whiteboard'){const g=base.getContext('2d'),m=state.sceneMap;g.save();g.translate(m.x,m.y);g.scale(m.scale,m.scale);g.beginPath();g.moveTo(865,435);g.lineTo(865,464);g.lineTo(873,456);g.lineTo(878,469);g.lineTo(884,466);g.lineTo(879,454);g.lineTo(890,453);g.closePath();g.fillStyle='#fff';g.fill();g.strokeStyle='#264132';g.lineWidth=1.5;g.stroke();g.restore();}
      beginCapture(base,mode==='whiteboard'?{x:0,y:0,w:base.width,h:base.height}:null);
      if(mode==='normal'&&state.settings.presetW>0&&state.settings.presetH>0){const w=Math.min(base.width,state.settings.presetW),h=Math.min(base.height,state.settings.presetH);state.selection={x:(base.width-w)/2,y:(base.height-h)/2,w,h};paint();}
      if(mode==='text-demo'){const d=state.sceneMap.scale;state.selection={...scenePoint(350,160),w:715*d,h:390*d};state.ops=[{id:uid(),tool:'text',...scenePoint(388,299),text:'单击修改文字，拖动边框移动',color:'#de514b',fontSize:28*state.density,font:'Microsoft YaHei'},{id:uid(),tool:'text',...scenePoint(388,396),text:'Enter 换行\n点击外部完成编辑',color:'#257f58',fontSize:22*state.density,font:'Microsoft YaHei'}];state.tool='text';updateTool();paint();}
    };
    if(state.settings.delay&&mode==='normal'){let left=Number(state.settings.delay);$('srCountdown').hidden=false;$('srCountdown').querySelector('strong').textContent=left;state.delayTimer=setInterval(()=>{left--;if(left<=0){clearInterval(state.delayTimer);state.delayTimer=null;$('srCountdown').hidden=true;run();}else $('srCountdown').querySelector('strong').textContent=left;},1000);}else run();
  }
  function cancelCapture(){if(state.delayTimer){clearInterval(state.delayTimer);state.delayTimer=null;}$('srCountdown').hidden=true;state.capturing=false;state.gesture=null;state.edit=null;state.draft=null;layer.hidden=true;editor.hidden=true;$('srTextFrame').hidden=true;$('srMagnifier').hidden=true;$('srSizePreview').hidden=true;state.editPin=null;root.classList.remove('sr-is-capturing');if(state.active){drawScene();renderPins();}}
  function changeDensity(density){
    if(state.busy||density===state.density)return;
    const ratio=density/state.density,editId=state.edit?.id,caret=editor.selectionStart;
    commitText();finishPolyline();state.gesture=null;state.density=density;
    root.querySelectorAll('[data-sr-density]').forEach(b=>b.classList.toggle('active',Number(b.dataset.srDensity)===density));
    drawScene();
    if(state.capturing){
      const base=makeCanvas(state.base.width*ratio,state.base.height*ratio);base.getContext('2d').drawImage(state.base,0,0,base.width,base.height);state.base=base;canvas.width=base.width;canvas.height=base.height;
      const scalePoint=p=>({x:p.x*ratio,y:p.y*ratio});
      const scaleOp=op=>{const o=clone(op);if(o.tool==='text'){o.x*=ratio;o.y*=ratio;o.fontSize*=ratio;}if(o.size)o.size*=ratio;if(o.start)o.start=scalePoint(o.start);if(o.end)o.end=scalePoint(o.end);if(o.points)o.points=o.points.map(scalePoint);return o;};
      state.ops=state.ops.map(scaleOp);state.undo=state.undo.map(list=>list.map(scaleOp));state.redo=state.redo.map(list=>list.map(scaleOp));
      if(state.selection){const r=state.selection;state.selection={x:r.x*ratio,y:r.y*ratio,w:r.w*ratio,h:r.h*ratio};}
      updateTool();paint();
      const op=state.ops.find(o=>o.id===editId);if(op){openText(op);editor.setSelectionRange(caret,caret);}
    }
    toast('像素密度模拟为 '+Math.round(density*100)+'%；当前选区与标注已保留。');
  }
  function captureDocument(){return {baseSrc:state.base.toDataURL('image/png'),region:clone(state.selection),ops:clone(state.ops),density:state.density};}
  function exportImage(){
    commitText();finishPolyline();if(!state.selection)throw new Error('请先选择截图区域');
    const merged=makeCanvas(canvas.width,canvas.height),g=merged.getContext('2d');g.drawImage(state.base,0,0);state.ops.forEach(o=>drawOperation(g,o));
    return regionCanvas(merged,state.selection);
  }
  function record(c,doc){const item={...sourceFromCanvas(c,'截图 '+new Date().toLocaleTimeString('zh-CN',{hour:'2-digit',minute:'2-digit'})),id:uid(),date:Date.now(),document:doc};state.history.unshift(item);state.history=state.history.slice(0,Number(state.settings.limit));state.lastRegion=clone(doc.region);persist();return item;}
  function download(c){const a=document.createElement('a');a.href=c.toDataURL('image/png');a.download='XChat-'+new Date().toISOString().replace(/[:.]/g,'-')+'.png';a.click();}
  async function copyImage(c){
    if(state.failCopy){state.failCopy=false;throw new Error('剪贴板写入失败（模拟）；选区和标注仍保留');}
    if(!navigator.clipboard?.write||!window.ClipboardItem)throw new Error('浏览器不支持图片复制，请保存图片或直接贴图');
    const blob=await new Promise(resolve=>c.toBlob(resolve,'image/png'));if(!blob)throw new Error('无法生成图片');
    try{await navigator.clipboard.write([new ClipboardItem({'image/png':blob})]);}catch{throw new Error('浏览器未允许写入剪贴板；请重试，或保存图片');}
    state.clipboard=sourceFromCanvas(c,'剪贴板图片');
  }
  async function output(action){
    if(state.busy||!state.capturing||!state.selection)return;
    if(action==='draft'&&!state.settings.chat){toast('请先选择一个会话；复制、保存和贴图仍可使用。',true);return;}
    state.busy=true;
    try{
      const c=exportImage(),doc=captureDocument(),pinId=state.editPin;
      if(action==='copy')await copyImage(c);
      if(action==='save')download(c);
      const item=record(c,doc);
      if(action==='pin'){
        const existing=state.pins.find(p=>p.id===pinId);
        if(existing){Object.assign(existing,sourceFromCanvas(c,existing.title),{document:doc});persist();renderPins();}
        else addPin({...item,document:doc});
      }
      cancelCapture();
      if(action==='draft'){showDialog('已加入聊天草稿','<p>设计讨论群 · 图片附件</p><img class="sr-draft-preview" src="'+item.src+'" alt="截图草稿预览"><div class="sr-note">截图已放入模拟草稿，尚未发送。实际应用会绑定发起截图时的会话。</div>',button('dialog-close','继续编辑聊天',null,'primary'));}
      else toast(action==='copy'?'图片已复制到系统剪贴板':action==='save'?'已交给浏览器下载 PNG':'已贴到桌面，可拖动、缩放或右键操作');
    }catch(error){toast(error.message||'操作失败，编辑内容已保留',true);}finally{state.busy=false;}
  }
  function addPin(source,save=true,extra={}){
    if(state.pins.length>=24){toast('此原型最多保留 24 张贴图，请先销毁不需要的贴图。',true);return;}
    const p={id:uid(),src:source.src,w:source.w,h:source.h,title:source.title||'截图',document:source.document||null,group:state.group,x:60+state.pins.length*35,y:65+state.pins.length*28,scale:Math.min(1,420/source.w,270/source.h),rotation:0,flipX:1,flipY:1,opacity:1,hidden:false,through:false,thumbnail:false,shadow:true,...extra};
    state.pins.push(p);state.selected=p.id;if(save)persist();renderPins();return p;
  }
  function pinDimensions(p){const scale=p.thumbnail?Math.min(150/p.w,95/p.h):p.scale,swap=Math.abs(p.rotation%180)===90;return {scale,w:(swap?p.h:p.w)*scale,h:(swap?p.w:p.h)*scale};}
  function renderPins(){
    const visible=state.pins.filter(p=>!p.hidden&&p.group===state.group);
    $('srPins').innerHTML=visible.map(p=>{
      const d=pinDimensions(p),left=clamp(Number(p.x)||0,-d.w+40,desktop.clientWidth-40),top=clamp(Number(p.y)||0,0,desktop.clientHeight-35);p.x=left;p.y=top;
      return '<div class="sr-pin '+(state.selected===p.id?'selected ':'')+(p.shadow===false?'no-shadow ':'')+(p.through?'through':'')+'" data-pin-id="'+p.id+'" style="left:'+left+'px;top:'+top+'px;width:'+d.w+'px;height:'+d.h+'px;z-index:'+(state.selected===p.id?30:3)+'" tabindex="0" aria-label="贴图 '+esc(p.title)+'"><img class="sr-pin-image" src="'+p.src+'" alt="'+esc(p.title)+'" draggable="false" style="position:absolute;width:'+p.w*d.scale+'px;height:'+p.h*d.scale+'px;left:'+(d.w-p.w*d.scale)/2+'px;top:'+(d.h-p.h*d.scale)/2+'px;opacity:'+p.opacity+';transform:rotate('+p.rotation+'deg) scale('+p.flipX+','+p.flipY+')"><span class="sr-pin-label">'+Math.round(p.scale*100)+'% · '+Math.round(p.opacity*100)+'%'+(p.through?' · 穿透模拟':p.thumbnail?' · 缩略图':' · 右键操作')+'</span></div>';
    }).join('');
    const n=state.pins.filter(p=>p.through&&!p.hidden&&p.group===state.group).length;$('srRescue').hidden=!n;$('srRescue').querySelector('span').textContent=n+' 张贴图正在模拟鼠标穿透';
    $('srPinCount').textContent=visible.length+' 张贴图';
    const hidden=state.pins.filter(p=>p.hidden).length;root.querySelector('[data-sr-action="manager"] span').textContent=hidden?'贴图 · '+hidden+' 张已收起':'历史与贴图';
    if(state.panel==='manager')renderManager();
  }
  function showPinMenu(p,x,y){
    state.selected=p.id;renderPins();const menu=$('srMenu');
    menu.innerHTML=[
      '<button data-pin-menu="edit">标注图片 <kbd>Space</kbd></button><button data-pin-menu="copy">复制当前图像 <kbd>Ctrl+C</kbd></button><button data-pin-menu="original">复制原始图像</button><button data-pin-menu="save">保存图像 <kbd>Ctrl+S</kbd></button><hr>',
      '<button data-pin-menu="reset">原始大小 <kbd>100%</kbd></button><button data-pin-menu="rotate">顺时针旋转 <kbd>1</kbd></button><button data-pin-menu="flip">水平翻转 <kbd>3</kbd></button><button data-pin-menu="thumbnail">'+(p.thumbnail?'恢复完整图片':'切换缩略图')+'</button>',
      '<label>透明度<input type="range" min="15" max="100" value="'+Math.round(p.opacity*100)+'" data-pin-opacity aria-label="贴图透明度"><output>'+Math.round(p.opacity*100)+'%</output></label><button data-pin-menu="shadow" role="menuitemcheckbox" aria-checked="'+(p.shadow!==false)+'"><span>窗口阴影</span><svg class="sr-menu-check" viewBox="0 0 24 24" aria-hidden="true"><path d="m5 12 4 4L19 6"/></svg></button><hr>',
      '<button data-pin-menu="through">'+(p.through?'关闭':'启用')+'鼠标穿透 <kbd>模拟</kbd></button><label>所属分组<select data-pin-group aria-label="贴图分组"><option'+(p.group==='默认'?' selected':'')+'>默认</option><option'+(p.group==='设计参考'?' selected':'')+'>设计参考</option></select></label><hr><button data-pin-menu="hide">隐藏 <kbd>Esc</kbd></button><button class="danger" data-pin-menu="destroy">销毁 <kbd>Shift+Esc</kbd></button>'
    ].join('');menu.hidden=false;menu.style.left=clamp(x,8,desktop.clientWidth-menu.offsetWidth-8)+'px';menu.style.top=clamp(y,8,desktop.clientHeight-menu.offsetHeight-8)+'px';
  }
  async function imageCanvas(p,original=false){
    const image=new Image();image.src=p.src;await image.decode();
    if(original){const c=makeCanvas(p.w,p.h);c.getContext('2d').drawImage(image,0,0,p.w,p.h);return c;}
    const swap=Math.abs(p.rotation%180)===90,c=makeCanvas((swap?p.h:p.w)*p.scale,(swap?p.w:p.h)*p.scale),g=c.getContext('2d');
    g.translate(c.width/2,c.height/2);g.rotate(p.rotation*Math.PI/180);g.scale(p.flipX*p.scale,p.flipY*p.scale);g.drawImage(image,-p.w/2,-p.h/2,p.w,p.h);return c;
  }
  async function editSource(item,pinId=null){
    const img=new Image();img.src=item.document?.baseSrc||item.src;
    try{await img.decode();const base=makeCanvas(img.naturalWidth,img.naturalHeight);base.getContext('2d').drawImage(img,0,0);if(item.document){state.density=item.document.density||1;root.querySelectorAll('[data-sr-density]').forEach(b=>b.classList.toggle('active',Number(b.dataset.srDensity)===state.density));}beginCapture(base,item.document?clone(item.document.region):{x:0,y:0,w:base.width,h:base.height});state.ops=clone(item.document?.ops||[]);state.editPin=pinId;state.tool='text';updateTool();paint();}
    catch{toast('无法读取该图片，其他记录仍保留。',true);}
  }
  async function pinAction(action,p){
    if(!p)return;$('srMenu').hidden=true;
    if(action==='edit'){await editSource(p,p.id);return;}
    if(action==='copy'||action==='original'||action==='save'){try{const c=await imageCanvas(p,action==='original');if(action==='save'){download(c);toast('已交给浏览器下载 PNG');}else{await copyImage(c);toast('图片已复制');}}catch(e){toast(e.message,true);}return;}
    if(action==='hide'){p.hidden=true;toast('贴图已收起，也可从“历史与贴图”找回',false,{label:'恢复',run:()=>{p.hidden=false;state.group=p.group;persist();renderPins();}});}
    if(action==='destroy')state.pins=state.pins.filter(item=>item.id!==p.id);
    if(action==='rotate')p.rotation=(p.rotation+90)%360;
    if(action==='rotate-back')p.rotation=(p.rotation+270)%360;
    if(action==='flip')p.flipX*=-1;
    if(action==='flip-y')p.flipY*=-1;
    if(action==='reset'){p.scale=1;p.thumbnail=false;}
    if(action==='thumbnail')p.thumbnail=!p.thumbnail;
    if(action==='shadow')p.shadow=p.shadow===false;
    if(action==='through'){p.through=!p.through;toast('仅模拟当前网页内穿透；可从右上角恢复鼠标交互。');}
    persist();renderPins();
  }
  function paste(){if(!state.clipboard){toast('演示剪贴板为空，请先截图或选择更多来源。',true);return;}cancelCapture();addPin(state.clipboard);toast('已从演示剪贴板创建一张独立贴图');}
  function handleAt(p){
    if(!state.selection)return null;const r=state.selection,d=view();
    return [['nw',0,0],['n',.5,0],['ne',1,0],['e',1,.5],['se',1,1],['s',.5,1],['sw',0,1],['w',0,.5]].find(h=>Math.hypot((p.x-r.x-r.w*h[1])*d.sx,(p.y-r.y-r.h*h[2])*d.sy)<9)?.[0];
  }
  function guessRegion(p){
    const d=state.sceneMap.scale,items=state.detectMode==='window'?[[1160,155,345,565,'聊天窗口'],[135,65,980,750,'文档窗口']]:[[375,315,206,108,'统计卡片'],[600,315,206,108,'统计卡片'],[825,315,206,108,'统计卡片'],[373,451,680,287,'趋势图'],[1222,270,250,86,'消息气泡']];
    return items.map(a=>({...scenePoint(a[0],a[1]),w:a[2]*d,h:a[3]*d,label:a[4]})).find(r=>inside(p,r))||null;
  }
  function showMagnifier(p){
    const el=$('srMagnifier');if(!state.settings.magnifier||state.edit||state.selection&&state.tool!=='select'){el.hidden=true;return;}
    const d=view(),g=el.querySelector('canvas').getContext('2d');g.imageSmoothingEnabled=false;g.clearRect(0,0,128,96);g.drawImage(state.base,clamp(p.x-8,0,canvas.width-16),clamp(p.y-6,0,canvas.height-12),16,12,0,0,128,96);
    g.strokeStyle='#17bc7b';g.lineWidth=1;g.strokeRect(60.5,44.5,8,8);g.beginPath();g.moveTo(64,0);g.lineTo(64,40);g.moveTo(64,56);g.lineTo(64,96);g.moveTo(0,48);g.lineTo(56,48);g.moveTo(72,48);g.lineTo(128,48);g.stroke();
    const rgb=state.base.getContext('2d').getImageData(clamp(Math.floor(p.x),0,canvas.width-1),clamp(Math.floor(p.y),0,canvas.height-1),1,1).data;
    state.pixelColor=state.rgb?'rgb('+Array.from(rgb).slice(0,3).join(', ')+')':'#'+Array.from(rgb).slice(0,3).map(n=>n.toString(16).padStart(2,'0')).join('').toUpperCase();
    el.querySelector('p').innerHTML='X '+Math.round(p.x)+' · Y '+Math.round(p.y)+'<br>'+state.pixelColor+' · C 复制';
    let x=d.ox+p.x*d.sx+24,y=d.oy+p.y*d.sy+24;if(x+132>d.w)x=d.ox+p.x*d.sx-148;if(y+146>d.h)y=d.oy+p.y*d.sy-153;
    el.style.left=clamp(x,5,d.w-135)+'px';el.style.top=clamp(y,5,d.h-145)+'px';el.hidden=false;
  }
  canvas.addEventListener('pointerdown',e=>{
    if(e.button!==0||state.busy)return;e.preventDefault();const p=point(e);state.selectedText=null;
    commitText();const handle=handleAt(p);
    if(handle){state.gesture={kind:'resize',handle,origin:clone(state.selection)};}
    else if(state.selection&&inside(p,state.selection)){
      if(state.tool==='text'){state.gesture={kind:'text',start:p,client:{x:e.clientX,y:e.clientY},op:clone(textAt(p)||null),moved:false};}
      else if(state.tool==='select')state.gesture={kind:'move-selection',start:p,origin:clone(state.selection)};
      else if(state.tool==='polyline'){
        if(!state.draft)state.draft={id:uid(),tool:'polyline',color:state.color,size:state.size*state.density,points:[p]};else state.draft.points.push(p);paint();return;
      }else{
        state.gesture={kind:'draw'};const o={id:uid(),tool:state.tool,color:state.color,size:state.size*state.density};
        if(['pen','marker','eraser'].includes(state.tool))o.points=[p];else{o.start=p;o.end=p;}state.draft=o;
      }
    }else {finishPolyline();state.gesture={kind:'select',start:p,client:{x:e.clientX,y:e.clientY},candidate:state.settings.detect?guessRegion(p):null};state.selection={x:p.x,y:p.y,w:1,h:1};state.tool='select';updateTool();}
    layer.setPointerCapture(e.pointerId);$('srHover').hidden=true;paint();
  });
  layer.addEventListener('pointermove',e=>{
    if(!state.capturing||state.busy)return;const p=point(e),g=state.gesture;state.cursorPoint=p;
    if(!g){
      if(e.target===canvas&&!state.selection&&state.settings.detect){state.hover=guessRegion(p);$('srHover').hidden=!state.hover;if(state.hover){positionBox($('srHover'),state.hover);$('srHover').querySelector('span').textContent=state.hover.label+' · 单击选择（模拟识别）';}}
      if(e.target===canvas)showMagnifier(p);return;
    }
    if(g.kind==='select')state.selection=normalized(rectBetween(g.start,p));
    if(g.kind==='move-selection'){state.selection={...g.origin,x:clamp(g.origin.x+p.x-g.start.x,0,canvas.width-g.origin.w),y:clamp(g.origin.y+p.y-g.start.y,0,canvas.height-g.origin.h)};}
    if(g.kind==='resize'){
      let l=g.origin.x,t=g.origin.y,r=l+g.origin.w,b=t+g.origin.h;
      if(g.handle.includes('w'))l=clamp(p.x,0,r-1);if(g.handle.includes('e'))r=clamp(p.x,l+1,canvas.width);
      if(g.handle.includes('n'))t=clamp(p.y,0,b-1);if(g.handle.includes('s'))b=clamp(p.y,t+1,canvas.height);
      state.selection={x:l,y:t,w:r-l,h:b-t};
    }
    if(g.kind==='draw'&&state.draft){
      const r=state.selection,q={x:clamp(p.x,r.x,r.x+r.w),y:clamp(p.y,r.y,r.y+r.h)};
      if(state.draft.points)state.draft.points.push(q);else{
        if(e.shiftKey){const a=state.draft.start,dx=q.x-a.x,dy=q.y-a.y;if(['rectangle','ellipse'].includes(state.tool)){const n=Math.min(Math.abs(dx),Math.abs(dy));q.x=a.x+Math.sign(dx)*n;q.y=a.y+Math.sign(dy)*n;}else if(Math.abs(dx)>Math.abs(dy)*2)q.y=a.y;else if(Math.abs(dy)>Math.abs(dx)*2)q.x=a.x;}
        state.draft.end=q;
      }
    }
    if(g.kind==='text'){
      if(Math.hypot(e.clientX-g.client.x,e.clientY-g.client.y)>4)g.moved=true;
      if(g.moved&&g.op)state.draft={...g.op,x:clamp(g.op.x+p.x-g.start.x,0,canvas.width-1),y:clamp(g.op.y+p.y-g.start.y,0,canvas.height-1)};
    }
    if(g.kind==='edit-move'&&state.edit){if(Math.hypot(e.clientX-g.client.x,e.clientY-g.client.y)>4)g.moved=true;if(g.moved){state.edit.x=clamp(g.origin.x+p.x-g.start.x,0,canvas.width-1);state.edit.y=clamp(g.origin.y+p.y-g.start.y,0,canvas.height-1);}}
    showMagnifier(p);paint();
  });
  function endGesture(e){
    const g=state.gesture;if(!g)return;state.gesture=null;$('srTextFrame').classList.remove('dragging');
    if(e.type==='pointercancel'){
      if(g.origin&&['resize','move-selection'].includes(g.kind))state.selection=g.origin;
      if(g.kind==='edit-move'&&state.edit)Object.assign(state.edit,g.origin);
      state.draft=null;paint();return;
    }
    if(g.kind==='select'){
      if(Math.hypot(e.clientX-g.client.x,e.clientY-g.client.y)<4){state.selection=g.candidate?normalized(g.candidate):null;}
    }
    if(g.kind==='draw'&&state.draft)commitOperation(state.draft);
    if(g.kind==='text'){
      if(g.moved&&g.op&&state.draft){commitOperation(state.draft);state.selectedText=g.op.id;}
      else if(!g.moved)openText(g.op,g.start);
    }
    if(g.kind==='edit-move'&&state.edit){const id=state.edit.id;commitText();const op=state.ops.find(o=>o.id===id);if(op)openText(op);}
    paint();
  }
  layer.addEventListener('pointerup',endGesture);layer.addEventListener('pointercancel',endGesture);
  layer.addEventListener('dblclick',e=>{if(e.target===canvas&&state.tool==='polyline')finishPolyline();});
  layer.addEventListener('contextmenu',e=>{
    if(e.target.closest('.sr-tools,textarea'))return;e.preventDefault();
    if(state.edit){commitText();return;}if(state.draft){finishPolyline();if(state.draft){state.draft=null;paint();}return;}
    if(state.selection){state.selection=null;state.showTools=true;paint();}else cancelCapture();
  });
  $('srTextFrame').addEventListener('pointerdown',e=>{
    if(e.button!==0||!state.edit)return;e.preventDefault();const id=state.edit.id;commitText();const op=state.ops.find(o=>o.id===id);if(!op)return;
    openText(op);$('srTextFrame').classList.add('dragging');state.gesture={kind:'edit-move',start:point(e),client:{x:e.clientX,y:e.clientY},origin:clone(state.edit),moved:false};layer.setPointerCapture(e.pointerId);
  });
  root.addEventListener('pointerdown',e=>{
    if(e.button!==0||!state.edit||e.target.closest('#srText,.sr-text-frame,.sr-style-row'))return;
    const target=e.target===canvas?textAt(point(e)):null,editedId=state.edit.id;commitText();
    if(e.target===canvas){e.preventDefault();e.stopPropagation();if(target&&target.id!==editedId)openText(target);}
  },true);
  editor.addEventListener('input',()=>{positionText();paint();});
  editor.addEventListener('keydown',e=>{
    e.stopPropagation();if(e.isComposing||e.keyCode===229)return;
    if(e.key==='Delete'){e.preventDefault();deleteText();}
    else if(e.key==='Escape'){e.preventDefault();cancelText();}
  });
  function styleChange(){
    if(state.edit){Object.assign(state.edit,{color:state.color,fontSize:state.fontSize*state.density,font:state.font});positionText();paint();}
    updateTool();
  }
  $('srTools').addEventListener('pointerdown',e=>{if(e.target.closest('button'))e.preventDefault();});
  layer.addEventListener('wheel',e=>{
    if(state.tool==='select'||!state.selection||state.busy||e.ctrlKey||e.metaKey||e.target.closest('select')||Math.abs(e.deltaX)>Math.abs(e.deltaY))return;
    e.preventDefault();const spec=sizeOptions(state.tool),delta=e.deltaY*(e.deltaMode===1?16:e.deltaMode===2?100:1);
    if(Math.sign(delta)!==Math.sign(state.wheelDelta||delta))state.wheelDelta=0;state.wheelDelta=(state.wheelDelta||0)+delta;
    if(Math.abs(state.wheelDelta)<24)return;
    const steps=Math.max(1,Math.min(3,Math.round(Math.abs(state.wheelDelta)/100))),direction=state.wheelDelta<0?1:-1;state.wheelDelta=0;
    if(state.tool==='text')state.fontSize=clamp(state.fontSize+direction*spec.step*steps,spec.min,spec.max);else {state.size=clamp(state.size+direction*spec.step*steps,spec.min,spec.max);toolSizes[state.tool]=state.size;}
    if(state.draft&&state.draft.tool!=='text')state.draft.size=state.size*state.density;
    styleChange();paint();const hint=$('srSizePreview'),size=state.tool==='text'?state.fontSize:state.size,b=desktop.getBoundingClientRect();hint.querySelector('span').textContent=spec.label+' '+size+' px';hint.querySelector('i').style.cssText='width:'+clamp(size,3,30)+'px;height:'+clamp(size,3,30)+'px;background:'+state.color;hint.style.left=clamp(e.clientX-b.left+22,12,desktop.clientWidth-140)+'px';hint.style.top=clamp(e.clientY-b.top+22,12,desktop.clientHeight-55)+'px';hint.hidden=false;clearTimeout(state.sizeTimer);state.sizeTimer=setTimeout(()=>hint.hidden=true,850);
  },{passive:false});
  $('srColor').addEventListener('input',e=>{state.color=e.target.value;styleChange();});
  $('srSize').addEventListener('input',e=>{if(state.tool==='text')state.fontSize=Number(e.target.value);else state.size=Number(e.target.value);styleChange();});
  [$('srSize'),$('srColor')].forEach(control=>control.addEventListener('change',()=>{if(state.edit)editor.focus({preventScroll:true});}));
  $('srFont').addEventListener('change',e=>{state.font=e.target.value;styleChange();if(state.edit)editor.focus({preventScroll:true});});
  const pins=$('srPins');let pinDrag=null;
  pins.addEventListener('pointerdown',e=>{
    if(e.button!==0||e.target.closest('button'))return;const el=e.target.closest('[data-pin-id]');if(!el)return;
    const p=state.pins.find(p=>p.id===el.dataset.pinId);if(!p||p.through)return;
    e.preventDefault();state.selected=p.id;pins.querySelectorAll('.sr-pin').forEach(n=>n.classList.toggle('selected',n===el));el.style.zIndex='30';
    pinDrag={p,el,x:e.clientX,y:e.clientY,px:p.x,py:p.y};el.setPointerCapture(e.pointerId);
  });
  pins.addEventListener('pointermove',e=>{if(!pinDrag)return;const g=pinDrag,d=pinDimensions(g.p);g.p.x=clamp(g.px+e.clientX-g.x,-d.w+35,desktop.clientWidth-35);g.p.y=clamp(g.py+e.clientY-g.y,0,desktop.clientHeight-35);g.el.style.left=g.p.x+'px';g.el.style.top=g.p.y+'px';});
  pins.addEventListener('pointerup',()=>{if(pinDrag){pinDrag=null;persist();}});
  pins.addEventListener('pointercancel',()=>{if(pinDrag){Object.assign(pinDrag.p,{x:pinDrag.px,y:pinDrag.py});pinDrag=null;renderPins();}});
  pins.addEventListener('wheel',e=>{
    const el=e.target.closest('[data-pin-id]');if(!el)return;e.preventDefault();const p=state.pins.find(p=>p.id===el.dataset.pinId);state.selected=p.id;
    if(e.ctrlKey||e.metaKey)p.opacity=clamp(p.opacity+(e.deltaY<0?.05:-.05),.15,1);else p.scale=clamp(p.scale*(e.deltaY<0?1.1:1/1.1),.1,4);
    p.thumbnail=false;renderPins();persist();
  },{passive:false});
  pins.addEventListener('contextmenu',e=>{const el=e.target.closest('[data-pin-id]');if(!el)return;e.preventDefault();const b=desktop.getBoundingClientRect();showPinMenu(state.pins.find(p=>p.id===el.dataset.pinId),e.clientX-b.left,e.clientY-b.top);});
  pins.addEventListener('dblclick',e=>{if(e.target.closest('button'))return;const el=e.target.closest('[data-pin-id]');if(el)pinAction(e.shiftKey?'thumbnail':'hide',state.pins.find(p=>p.id===el.dataset.pinId));});
  function panelShell(title,sub,body){
    $('srPanel').hidden=false;$('srPanel').innerHTML='<header class="sr-panel-head"><div><h2>'+title+'</h2><small>'+sub+'</small></div><button class="sr-tool" data-sr-action="panel-close" aria-label="关闭侧栏">'+svg('close')+'</button></header><div class="sr-panel-body">'+body+'</div>';
  }
  function closePanel(){state.panel=null;$('srPanel').hidden=true;}
  function openPanel(name){commitText();state.panel=name;$('srMenu').hidden=true;if(name==='manager')renderManager();if(name==='settings')renderSettings();if(name==='help')renderHelp();}
  function renderManager(){
    const history=state.tab==='history',items=history?state.history:state.pins.filter(p=>p.group===state.group);
    const tabs='<div class="sr-tabs"><button data-sr-tab="pins" class="'+(!history?'active':'')+'">贴图 '+state.pins.length+'</button><button data-sr-tab="history" class="'+(history?'active':'')+'">截图历史 '+state.history.length+'</button></div>';
    const groups=history?'':'<div class="sr-filter">'+['默认','设计参考'].map(g=>'<button data-sr-group="'+g+'" class="'+(state.group===g?'active':'')+'">'+g+'</button>').join('')+'</div><div class="sr-manager-actions">'+button('show-pins','显示本组',null,'small')+button('hide-pins','隐藏本组',null,'small')+button('recover-pins','找回屏外贴图',null,'small')+'</div>';
    const cards=items.length?items.map(p=>'<article class="sr-card"><div class="sr-card-preview"><img src="'+p.src+'" alt="'+esc(p.title)+'"><span class="sr-tag">'+(history?'历史':p.hidden?'已隐藏':p.through?'穿透中':'显示中')+'</span></div><div class="sr-card-content"><b>'+esc(p.title)+'</b><small>'+p.w+' × '+p.h+' px</small><div class="sr-card-actions">'+(history?'<button data-sr-history="pin" data-record-id="'+p.id+'">贴图</button><button data-sr-history="edit" data-record-id="'+p.id+'">回放</button>':'<button data-sr-manage="show" data-record-id="'+p.id+'">'+(p.hidden?'恢复':'定位')+'</button><button data-sr-manage="hide" data-record-id="'+p.id+'">隐藏</button>')+'</div></div></article>').join(''):'<div class="sr-empty">'+(history?'完成一次截图后，记录会出现在这里。':'这个分组还没有贴图。')+'</div>';
    panelShell('历史与贴图','隐藏可恢复，销毁会移除贴图。',tabs+groups+'<div class="sr-cards">'+cards+'</div><div class="sr-note">'+(history?'截图记录保留原始画面、选区与标注，可回放后继续修改文字。':'当前分组：'+esc(state.group)+'。在贴图右键菜单中可更换分组。')+'</div>');
  }
  function checkSetting(name,title,sub){return '<label class="sr-setting-row"><span><strong>'+title+'</strong><small>'+sub+'</small></span><input type="checkbox" data-sr-setting="'+name+'" '+(state.settings[name]?'checked':'')+'></label>';}
  function renderSettings(){
    const s=state.settings;
    panelShell('截图设置','仅作用于此原型，不更改应用设置。',
      '<label class="sr-field"><span>快捷键预设</span><select data-sr-setting="preset"><option value="snipaste"'+(s.preset==='snipaste'?' selected':'')+'>经典 · F1 / F3</option><option value="xchat"'+(s.preset==='xchat'?' selected':'')+'>XChat · Ctrl+Shift+A / F3</option></select><small>按键仅在此页面获得焦点时有效。</small></label>'+
      '<label class="sr-field"><span>开始前延时</span><select data-sr-setting="delay">'+[0,3,5].map(n=>'<option value="'+n+'"'+(Number(s.delay)===n?' selected':'')+'>'+(n?n+' 秒':'立即开始')+'</option>').join('')+'</select></label>'+
      '<div class="sr-preset-fields"><label class="sr-field"><span>预设宽度 px</span><input type="number" min="0" max="3200" data-sr-setting="presetW" value="'+(s.presetW||0)+'"></label><label class="sr-field"><span>预设高度 px</span><input type="number" min="0" max="1800" data-sr-setting="presetH" value="'+(s.presetH||0)+'"></label></div><small style="color:var(--sr-muted)">宽高均为正数时生效，0 表示自由框选。</small>'+
      checkSetting('detect','自动识别区域','演示画面中的窗口和控件边界')+checkSetting('magnifier','显示像素放大镜','框选时查看像素位置与色值')+checkSetting('captureCursor','包含鼠标指针','在演示桌面中加入指针示例')+checkSetting('chat','当前有聊天会话','关闭后禁用加入草稿，截图仍可独立使用')+checkSetting('remember','恢复贴图与历史','保存在本机原型存储中')+
      '<label class="sr-field"><span>历史上限</span><select data-sr-setting="limit">'+[6,12,20].map(n=>'<option value="'+n+'"'+(Number(s.limit)===n?' selected':'')+'>'+n+' 条</option>').join('')+'</select></label>'+
      '<label class="sr-field"><span>界面外观</span><select data-sr-setting="theme"><option value="light"'+(s.theme==='light'?' selected':'')+'>浅色</option><option value="dark"'+(s.theme==='dark'?' selected':'')+'>深色</option></select></label>'+
      button('whiteboard','打开白板','pen')+'<div class="sr-note">原生快捷键注册、自动保存目录、系统打印和多屏权限由桌面端实现；本页不伪装这些系统操作。</div>');
  }
  function renderHelp(){
    panelShell('评审说明','XChat 截图与贴图 · 交互评审',
      '<div class="sr-note">此处使用内置演示桌面。窗口识别、像素密度和穿透是网页内模拟；图片下载与剪贴板写入会使用真实浏览器能力。</div>'+
      '<h3>先检查文字和选区</h3><ol class="sr-review-list"><li>截图时工作台顶部和底部隐藏，画布铺满页面。Ctrl+A 选择整个画面。</li><li>单击文字原位修改；Enter 直接换行，点击文字框外部完成，Esc 放弃本次修改。</li><li>悬浮文字边框出现手形，拖动边框移动；编辑激活时 Delete 删除整条标注，Backspace 正常删字。</li><li>滚轮快速调整当前画笔、形状和箭头线宽，也可调整字号、马赛克颗粒、模糊强度、橡皮擦大小。</li><li>移动或缩放选区时文字保持原图坐标。模拟密度在截图前选择。</li></ol>'+
      '<div class="sr-manager-actions">'+button('text-demo','进入文字与选区检查','text','small')+'</div>'+
      '<h3>基础操作</h3><div class="sr-note">Ctrl+Z 撤销 · Ctrl+Y 重做<br>Ctrl+Shift+Z 清空标注<br>Enter 复制截图 · Ctrl+T 贴图<br>贴图：1/2 旋转，3/4 翻转，滚轮缩放<br>Ctrl+滚轮调透明度，双击隐藏，Shift+双击缩略</div>'+
      '<h3>恢复与失败场景</h3><div class="sr-manager-actions">'+button('fail-copy','下次复制失败',null,'small')+button('deny-capture','下次权限拒绝',null,'small')+button('reset-review','重置演示',null,'small')+'</div>'+
      '<div class="sr-note"><a href="../docs/plans/2026-10-06-snipaste-alignment-baseline.md" target="_blank" rel="noopener">阅读完整对齐基线与验收用例 ↗</a><br>文字旋转与缩放把手、GIF 帧控制、HTML 富文本贴图、其他图片格式、贴图取色与替换、完整鼠标快捷操作及系统命令行已列入基础基线，本轮尚未演示。</div>');
  }
  let dialogPreviousFocus=null;
  function showDialog(title,body,footer=''){dialogPreviousFocus=document.activeElement;$('srDialog').hidden=false;$('srDialog').innerHTML='<section class="sr-dialog" role="dialog" aria-modal="true" aria-label="'+esc(title)+'"><div class="sr-dialog-head"><h2>'+title+'</h2><button class="sr-tool" data-sr-action="dialog-close" aria-label="关闭弹窗">'+svg('close')+'</button></div>'+body+'<div class="sr-dialog-error" role="alert"></div><div class="sr-dialog-foot">'+footer+'</div></section>';requestAnimationFrame(()=>$('srDialog').querySelector('textarea,input,button')?.focus());}
  function closeDialog(){$('srDialog').hidden=true;$('srDialog').innerHTML='';if(dialogPreviousFocus?.isConnected)dialogPreviousFocus.focus({preventScroll:true});dialogPreviousFocus=null;}
  function showSources(type='text'){
    showDialog('从内容创建贴图','<p>F3 使用演示剪贴板。也可以输入文字、颜色，或选择本地图片。</p><div class="sr-paste-options">'+[['text','文字'],['color','颜色'],['file','图片文件']].map(a=>'<button data-sr-source="'+a[0]+'" class="'+(a[0]===type?'active':'')+'">'+a[1]+'</button>').join('')+'</div>'+
      (type==='text'?'<label class="sr-field"><span>文字内容</span><textarea id="srPasteText">选区与文字要准确对齐。\n检查输入、提交、再次编辑和移动。</textarea></label>':type==='color'?'<label class="sr-field"><span>颜色值</span><input type="text" id="srPasteColor" value="#18AC71" placeholder="#18AC71"></label>':'<label class="sr-field"><span>本地图片</span><input type="file" id="srPasteFile" accept="image/png,image/jpeg,image/webp,image/gif"><small>本原型支持浏览器可解码的 PNG、JPEG、WebP 与 GIF。</small></label>'),
      button('dialog-close','取消')+button('create-source','创建贴图','pin','primary'));
    state.sourceType=type;
  }
  async function createSource(){
    let source;
    try{
      if(state.sourceType==='text'){
        const text=$('srPasteText').value;if(!text.trim())throw new Error('请输入文字内容');const lines=text.split('\n').slice(0,30),c=makeCanvas(540,Math.max(140,lines.length*32+48)),g=c.getContext('2d');g.fillStyle='#fffef5';g.fillRect(0,0,c.width,c.height);lines.forEach((l,i)=>txt(g,l,24,48+i*32,20,'#4b624e'));source=sourceFromCanvas(c,'文字便签');
      }else if(state.sourceType==='color'){
        const value=$('srPasteColor').value.trim();if(!/^#[\da-f]{6}$/i.test(value))throw new Error('请输入六位十六进制颜色，例如 #18AC71');const c=makeCanvas(330,220),g=c.getContext('2d');g.fillStyle=value;g.fillRect(0,0,330,160);g.fillStyle='#fff';g.fillRect(0,160,330,60);txt(g,value.toUpperCase(),20,200,23,'#41604a',400,'Consolas');source=sourceFromCanvas(c,'色卡 '+value.toUpperCase());
      }else{
        const file=$('srPasteFile').files[0];if(!file)throw new Error('请选择一张图片');if(file.size>8*1024*1024)throw new Error('此原型支持 8 MB 以内的图片');
        const src=await new Promise((resolve,reject)=>{const r=new FileReader();r.onload=()=>resolve(r.result);r.onerror=reject;r.readAsDataURL(file);}),img=new Image();img.src=src;await img.decode();source={src,w:img.naturalWidth,h:img.naturalHeight,title:file.name};
      }
      state.clipboard=source;closeDialog();addPin(source);toast('已创建贴图，也已放入演示剪贴板');
    }catch(e){$('srDialog').querySelector('.sr-dialog-error').textContent=e.message||'无法读取这张图片';}
  }
  const commands={
    capture:()=>startCapture(), 'text-demo':()=>startCapture('text-demo'), whiteboard:()=>startCapture('whiteboard'),
    leave,paste,sources:()=>showSources(),manager:()=>openPanel('manager'),settings:()=>openPanel('settings'),help:()=>openPanel('help'),
    'panel-close':closePanel,'dialog-close':closeDialog,'create-source':createSource,'cancel-delay':cancelCapture,cancel:cancelCapture,
    pin:()=>output('pin'),copy:()=>output('copy'),save:()=>output('save'),draft:()=>output('draft'),
    reselect:()=>{commitText();finishPolyline();state.selection=null;state.tool='select';updateTool();paint();},
    'commit-text':commitText,
    'delete-text':deleteText,
    undo:()=>{commitText();finishPolyline();if(state.undo.length){state.redo.push(clone(state.ops));state.ops=state.undo.pop();paint();}},
    redo:()=>{commitText();finishPolyline();if(state.redo.length){state.undo.push(clone(state.ops));state.ops=state.redo.pop();paint();}},
    rescue:()=>{state.pins.forEach(p=>p.through=false);persist();renderPins();toast('所有贴图已恢复鼠标交互');},
    'show-pins':()=>{state.pins.filter(p=>p.group===state.group).forEach(p=>p.hidden=false);persist();renderPins();},
    'hide-pins':()=>{state.pins.filter(p=>p.group===state.group).forEach(p=>p.hidden=true);persist();renderPins();},
    'recover-pins':()=>{state.pins.filter(p=>p.group===state.group).forEach((p,i)=>{p.x=35+i*30;p.y=35+i*25;p.hidden=false;});persist();renderPins();},
    'fail-copy':()=>{state.failCopy=true;toast('下一次图片复制将模拟失败，并保留编辑内容');},
    'deny-capture':()=>{state.denied=true;toast('下一次截图将模拟权限拒绝');},
    'reset-review':()=>showDialog('重置演示','<p>清除本原型的贴图与历史记录，恢复示例。不会改动 XChat 的消息、文件或应用设置。</p>',button('dialog-close','取消')+button('confirm-reset','重置演示',null,'primary')),
    'confirm-reset':()=>{try{localStorage.removeItem(STORE);}catch{}cancelCapture();closeDialog();closePanel();state.pins=[];state.history=[];state.initialized=false;enter();toast('演示已重置');}
  };
  root.addEventListener('click',e=>{
    const action=e.target.closest('[data-sr-action]')?.dataset.srAction;if(action){commands[action]?.();return;}
    const tool=e.target.closest('[data-sr-tool]')?.dataset.srTool;if(tool){changeTool(tool);return;}
    const color=e.target.closest('[data-sr-color]')?.dataset.srColor;if(color){state.color=color;styleChange();return;}
    const density=e.target.closest('[data-sr-density]')?.dataset.srDensity;if(density){changeDensity(Number(density));return;}
    const tab=e.target.closest('[data-sr-tab]')?.dataset.srTab;if(tab){state.tab=tab;renderManager();return;}
    const group=e.target.closest('[data-sr-group]')?.dataset.srGroup;if(group){state.group=group;persist();renderPins();return;}
    const source=e.target.closest('[data-sr-source]')?.dataset.srSource;if(source){showSources(source);return;}
    const menuAction=e.target.closest('[data-pin-menu]')?.dataset.pinMenu;if(menuAction){pinAction(menuAction,state.pins.find(p=>p.id===state.selected));return;}
    const recordEl=e.target.closest('[data-record-id]');
    if(recordEl){if(recordEl.dataset.srHistory){const item=state.history.find(r=>r.id===recordEl.dataset.recordId);if(item){if(recordEl.dataset.srHistory==='pin')addPin(item);else editSource(item);}}
      else{const p=state.pins.find(p=>p.id===recordEl.dataset.recordId);if(p){p.hidden=recordEl.dataset.srManage==='hide';if(!p.hidden){p.x=40;p.y=50;state.selected=p.id;}persist();renderPins();}}return;}
    if(!e.target.closest('.sr-pin-menu'))$('srMenu').hidden=true;
  });
  root.addEventListener('change',e=>{
    const name=e.target.dataset.srSetting;
    if(name){state.settings[name]=e.target.type==='checkbox'?e.target.checked:['delay','limit','presetW','presetH'].includes(name)?Math.max(0,Number(e.target.value)||0):e.target.value;root.classList.toggle('sr-dark',state.settings.theme==='dark');state.history=state.history.slice(0,Number(state.settings.limit));if(name==='remember'&&!state.settings.remember){try{localStorage.removeItem(STORE);}catch{}}else persist();paint();}
    if(e.target.hasAttribute('data-pin-group')){const p=state.pins.find(p=>p.id===state.selected);if(p){p.group=e.target.value;persist();renderPins();$('srMenu').hidden=true;}}
  });
  root.addEventListener('input',e=>{if(e.target.hasAttribute('data-pin-opacity')){const p=state.pins.find(p=>p.id===state.selected);if(p){p.opacity=Number(e.target.value)/100;e.target.nextElementSibling.textContent=e.target.value+'%';const img=pins.querySelector('[data-pin-id="'+p.id+'"] img');if(img)img.style.opacity=p.opacity;persist();}}});
  function handleKey(e){
    if(!state.active)return;
    if(!$('srDialog').hidden&&e.key==='Tab'){
      const els=[...$('srDialog').querySelectorAll('button,input,select,textarea,a[href]')].filter(el=>!el.disabled),first=els[0],last=els.at(-1);
      if(e.shiftKey&&e.target===first){e.preventDefault();last?.focus();}else if(!e.shiftKey&&e.target===last){e.preventDefault();first?.focus();}return;
    }
    const typing=e.target.closest?.('input,textarea,select,[contenteditable="true"]');
    if(typing||e.isComposing)return;
    const key=e.key.toLowerCase(),mod=e.ctrlKey||e.metaKey,p=state.pins.find(p=>p.id===state.selected);let handled=true;
    if(e.key==='Escape'){
      if(!$('srDialog').hidden)closeDialog();else if(state.panel)closePanel();
      else if(state.capturing){if(state.draft){state.draft=null;state.gesture=null;paint();}else cancelCapture();}
      else if(p)pinAction(e.shiftKey?'destroy':'hide',p);
    }else if(state.settings.preset==='snipaste'&&e.key==='F1'||state.settings.preset==='xchat'&&mod&&e.shiftKey&&key==='a'){startCapture();}
    else if(e.key==='F3')paste();
    else if(state.capturing){
      if(mod&&key==='z'){if(e.shiftKey){cancelText();state.ops=[];state.undo=[];state.redo=[];state.draft=null;paint();}else commands.undo();}
      else if(mod&&key==='y')commands.redo();
      else if(mod&&key==='c')output('copy');
      else if(mod&&key==='s')output('save');
      else if(mod&&key==='t')output('pin');
      else if(mod&&key==='a'){commitText();state.selection={x:0,y:0,w:canvas.width,h:canvas.height};paint();}
      else if(e.key==='Enter'&&!e.target.closest('button')){if(state.draft?.tool==='polyline')finishPolyline();else output('copy');}
      else if(e.key==='Tab'&&!state.selection){state.detectMode=state.detectMode==='window'?'element':'window';$('srHover').hidden=true;paint();}
      else if(e.key===' '&&!e.target.closest('button')){state.showTools=!state.showTools;paint();}
      else if(key==='t'&&!mod)changeTool('text');
      else if(key==='b'&&!mod)changeTool('pen');
      else if(key==='r'&&!mod){if(state.lastRegion){state.selection=normalized(state.lastRegion);paint();}else toast('还没有成功截图的选区记录');}
      else if(key==='c'&&!mod&&!$('srMagnifier').hidden){navigator.clipboard?.writeText(state.pixelColor).then(()=>toast('已复制 '+state.pixelColor)).catch(()=>toast('浏览器未允许复制色值',true));}
      else if(e.key==='Shift'&&!$('srMagnifier').hidden){state.rgb=!state.rgb;if(state.cursorPoint)showMagnifier(state.cursorPoint);}
      else if(['ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(e.key)&&state.selection){
        const dx=e.key==='ArrowLeft'?-1:e.key==='ArrowRight'?1:0,dy=e.key==='ArrowUp'?-1:e.key==='ArrowDown'?1:0;
        const textOp=state.tool==='text'&&state.ops.find(o=>o.id===state.selectedText);
        if(textOp&&!mod&&!e.shiftKey)commitOperation({...textOp,x:clamp(textOp.x+dx,0,canvas.width-1),y:clamp(textOp.y+dy,0,canvas.height-1)});
        else {
          const r={...state.selection};
          if(mod||e.shiftKey){const change=e.shiftKey?-1:1;if(dx<0){r.x-=change;r.w+=change;}if(dx>0)r.w+=change;if(dy<0){r.y-=change;r.h+=change;}if(dy>0)r.h+=change;state.selection=normalized(r);}
          else state.selection={...r,x:clamp(r.x+dx,0,canvas.width-r.w),y:clamp(r.y+dy,0,canvas.height-r.h)};
          paint();
        }
      }else if((key===','||key==='.')&&state.history.length){state.replayIndex=clamp((state.replayIndex??-1)+(key===','?1:-1),0,state.history.length-1);editSource(state.history[state.replayIndex]);}
      else handled=false;
    }else if(p&&!p.hidden){
      if(['1','2','3','4'].includes(key))pinAction({'1':'rotate','2':'rotate-back','3':'flip','4':'flip-y'}[key],p);
      else if(e.key===' '&&!e.target.closest('button'))pinAction('edit',p);
      else if(mod&&key==='c')pinAction('copy',p);
      else if(mod&&key==='s')pinAction('save',p);
      else if(['ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(e.key)){p.x+=e.key==='ArrowLeft'?-1:e.key==='ArrowRight'?1:0;p.y+=e.key==='ArrowUp'?-1:e.key==='ArrowDown'?1:0;renderPins();persist();}
      else handled=false;
    }else handled=false;
    if(handled){e.preventDefault();e.stopImmediatePropagation();}
  }
  document.addEventListener('keydown',handleKey,true);
  root.addEventListener('keydown',e=>e.stopPropagation());
  new ResizeObserver(()=>{if(!state.active)return;requestAnimationFrame(()=>{if(state.capturing)paint();else{drawScene();renderPins();}});}).observe(desktop);
  const scenario=document.getElementById('desktopReviewScenario');
  if(scenario){scenario.add(new Option('XChat 截图与贴图 · 交互评审','snipaste'));scenario.addEventListener('change',()=>{if(scenario.value==='snipaste')enter();else if(state.active){cancelCapture();closePanel();closeDialog();state.active=false;root.hidden=true;}});}
  const captureEntry=document.getElementById('captureBtn');
  if(captureEntry)captureEntry.onclick=()=>{if(scenario)scenario.value='snipaste';enter();startCapture();};
  const params=new URLSearchParams(location.search);
  if(['capture','snipaste'].includes(params.get('review'))){if(scenario)scenario.value='snipaste';enter();if(params.get('focus')==='text')startCapture('text-demo');}
})();
