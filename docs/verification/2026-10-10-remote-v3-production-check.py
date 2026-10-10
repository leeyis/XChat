"""Production React UI checks with synthetic fixture data; no existing browser/session.
Run: rtk proxy python -X utf8 docs/verification/2026-10-10-remote-v3-production-check.py
"""
import asyncio, ast, base64, json, os, subprocess, tempfile, time
from pathlib import Path
from urllib.request import urlopen
import websockets

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'docs/verification'
VITE_PORT=int(os.environ.get('XCHAT_REMOTE_V3_QA_PORT','18954'))
tree=ast.parse((OUT/'2026-10-10-remote-e2e.py').read_text(encoding='utf-8'))
exec(compile(ast.Module(body=[item for item in tree.body if isinstance(item,(ast.ClassDef,ast.FunctionDef)) and item.name in ['Page','waitfor']],type_ignores=[]),'<qa-common>','exec'))
temp=Path(tempfile.mkdtemp(prefix='xchat-remote-v3-production-'));processes=[];logs=[];checks=[]
def check(name,ok,detail=None):
 checks.append(dict(name=name,passed=bool(ok),detail=detail))
 if not ok:raise AssertionError(name+': '+str(detail))
async def run():
 fixture=(OUT/'2026-10-10-remote-v3-production-fixture.jsx').as_posix()
 html='<html><head><meta name="viewport" content="width=device-width,initial-scale=1"></head><body><div id="root"></div><script type="module" src="/@fs/'+fixture+'"></script></body></html>'
 script=temp/'vite.mjs'
 script.write_text('import {createServer} from '+json.dumps((ROOT/'node_modules/vite/dist/node/index.js').as_uri())+';const server=await createServer({configFile:'+json.dumps(str(ROOT/'vite.config.js'))+',server:{host:"127.0.0.1",port:'+str(VITE_PORT)+',strictPort:true},plugins:[{name:"remote-v3-production-qa",configureServer(s){s.middlewares.use(async(req,res,next)=>{if(!req.url.startsWith("/__remote-v3-qa__"))return next();res.setHeader("Content-Type","text/html");res.end(await s.transformIndexHtml(req.url,'+json.dumps(html)+'));});}}]});await server.listen();',encoding='utf-8')
 log=(temp/'vite.log').open('w',encoding='utf-8');logs.append(log)
 processes.append(subprocess.Popen(['rtk','proxy','node',str(script)],cwd=ROOT,stdout=log,stderr=log,creationflags=subprocess.CREATE_NO_WINDOW))
 waitfor(lambda:urlopen(f'http://127.0.0.1:{VITE_PORT}/__remote-v3-qa__',timeout=2).status==200)
 profile=temp/'chrome';profile.mkdir()
 processes.append(subprocess.Popen(['C:/Program Files/Google/Chrome/Application/chrome.exe','--headless=new','--disable-gpu','--disable-extensions','--no-first-run','--mute-audio','--remote-debugging-port=0','--user-data-dir='+str(profile),'about:blank'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,creationflags=subprocess.CREATE_NO_WINDOW))
 waitfor(lambda:(profile/'DevToolsActivePort').exists());port=(profile/'DevToolsActivePort').read_text().splitlines()[0]
 target=next(item for item in json.load(urlopen(f'http://127.0.0.1:{port}/json/list')) if item['type']=='page')
 page=await Page().connect(target);await page.call('Runtime.enable');await page.call('Page.enable')
 async def shot(name):
  await asyncio.sleep(.2)
  data=await page.call('Page.captureScreenshot',dict(format='png',captureBeyondViewport=False));(OUT/f'2026-10-10-remote-v3-production-{name}.png').write_bytes(base64.b64decode(data['data']))
 async def load(role):
  await page.call('Page.navigate',dict(url=f'http://127.0.0.1:{VITE_PORT}/__remote-v3-qa__?role='+role))
  await page.wait('!!window.qa&&!!document.querySelector(".remote-viewer,.remote-chat-session-bar")',20)
 await page.call('Emulation.setDeviceMetricsOverride',dict(width=1360,height=900,deviceScaleFactor=1,mobile=False))
 await load('viewer')
 await page.wait('document.querySelector("video")?.videoWidth===1280')
 check('查看器独立且不挂载聊天/导航',await page.js('!document.querySelector(".chat-workspace,.ra-chat-panel,.rail")'))
 await shot('viewer')
 await page.button('主机信息')
 check('未知指标不伪装成目标性能',await page.js('document.querySelector(".remote-info-popover").innerText.includes("未提供")&&!document.querySelector(".remote-info-popover").innerText.includes("H.265")'))
 await page.js("qa.publish({metrics:{fps:29.6,rtt:17,kbps:1248,codec:'VP8',decoderImplementation:'libvpx',hostEncoderImplementation:'libvpx',hostCaptureBackend:'DXGI',hostCaptureMs:3.2,hostEncodeMs:1.4,width:1280,height:720,protocol:'udp',path:'direct'}})")
 check('真实字段分别显示编码/采集与接收延时',await page.js('document.querySelector(".remote-info-popover").innerText.includes("DXGI")&&document.querySelector(".remote-info-popover").innerText.includes("VP8")&&document.querySelector(".remote-info-popover").innerText.includes("17")'))
 await shot('metrics')
 await page.js('document.querySelector("button[aria-label=关闭信息]").click()')
 await page.button('全屏');await page.wait('!!document.fullscreenElement')
 await page.button('释放控制')
 check('全屏时释放控制且保留全屏',await page.js('!!document.fullscreenElement&&qa.calls.some(item=>item.type==="release_control")'))
 await page.button('退出全屏');await page.wait('!document.fullscreenElement')
 await page.button('回到聊天')
 check('返回聊天调用主窗口API',await page.js('qa.calls.some(item=>item.type==="focus_chat")'))
 await page.js('qa.publish({audioBlocked:true})')
 await page.wait('!!document.querySelector(".remote-viewer-audio-notice")')
 await page.button('开启声音','document.querySelector(".remote-viewer-audio-notice")')
 check('查看器可直接解锁本窗口通话声音',await page.js('qa.calls.some(item=>item.type==="play_audio")&&!qa.remote.audioBlocked'))
 await load('controller')
 check('主窗口原聊天保持，只有会话条',await page.js('!!document.querySelector(".chat-workspace .remote-chat-session-bar")&&!document.querySelector(".remote-workspace,video")'))
 await page.js('window.qaConversations=qa.chat.conversations;qa.patchChat({conversations:[]})')
 await page.wait('!document.querySelector(".chat-workspace .remote-chat-session-bar")')
 await page.js('qa.patchChat({conversations:window.qaConversations})')
 await page.wait('!!document.querySelector(".chat-workspace .remote-chat-session-bar")')
 check('异步会话重新出现时重挂共享状态与语音',await page.js('!!document.querySelector(".chat-workspace .remote-chat-voice")'))
 await page.js('qa.patchChat({activeSection:"hosts"})')
 await page.wait('!document.querySelector(".chat-workspace")')
 await page.js('window.dispatchEvent(new CustomEvent("xchat:remote-chat",{detail:{peerId:"qa-remote-peer"}}))')
 await page.wait('!!document.querySelector(".chat-workspace .remote-chat-session-bar")')
 check('回到聊天能从主机页恢复原会话',await page.js('qa.chat.activeSection==="chat"&&qa.chat.activeConversationId==="qa-chat"'))
 await page.button('打开远程窗口')
 check('主窗口打开专用查看器API',await page.js('qa.calls.some(item=>item.type==="open_viewer")'))
 await page.button('静音')
 check('静音和扬声器透传媒体所属窗口',await page.js('qa.calls.some(item=>item.type==="muted")'))
 await page.js('document.querySelector("button[aria-label=关闭扬声器]").click()')
 check('扬声器走mediaAction',await page.js('qa.calls.some(item=>item.type==="speaker"&&item.enabled===false)'))
 await shot('main-chat')
 await load('host')
 check('被控方完整聊天且无本机预览',await page.js('!!document.querySelector(".chat-workspace .remote-chat-session-bar")&&!document.querySelector("video,.remote-workspace")'))
 check('原生工具条DOM不含外框留白',await page.js('getComputedStyle(document.querySelector(".remote-native-shell")).padding==="0px"&&document.querySelector(".remote-native-shell").getBoundingClientRect().width===document.querySelector(".remote-native-shell .ra-floating-panel").getBoundingClientRect().width'))
 await shot('host')
 await page.button('收回控制','document.querySelector(".remote-chat-session-bar")')
 check('共享状态条可收回控制',await page.js('!qa.remote.session.grant&&document.querySelector(".remote-chat-session-copy").innerText.includes("仅可查看")'))
 await page.button('暂停共享','document.querySelector(".remote-chat-session-bar")')
 check('暂停后聊天仍显示且无视频',await page.js('qa.remote.session.paused&&!!document.querySelector(".composer")&&!document.querySelector("video")'))
 for role in ['viewer','controller','host']:
  await load(role)
  for width in [860,390]:
   await page.call('Emulation.setDeviceMetricsOverride',dict(width=width,height=844,deviceScaleFactor=1,mobile=False))
   await asyncio.sleep(.2)
   check(f'{role} {width}px页面不横向溢出',await page.js('document.documentElement.scrollWidth<=innerWidth'))
  await page.call('Emulation.setDeviceMetricsOverride',dict(width=1360,height=900,deviceScaleFactor=1,mobile=False))
 check('生产组件无未捕获异常',not page.errors,page.errors)
 await page.call('Browser.close')
try:
 asyncio.run(run())
finally:
 for process in reversed(processes):
  if process.poll() is None:subprocess.run(['rtk','proxy','taskkill','/PID',str(process.pid),'/T','/F'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,creationflags=subprocess.CREATE_NO_WINDOW)
 for log in logs:log.close()
 report=dict(method='Production React components in isolated Vite/Chrome; synthetic session, not a transport benchmark',checks=checks,passed=sum(item['passed'] for item in checks),failed=sum(not item['passed'] for item in checks),log_directory=str(temp))
 (OUT/'2026-10-10-remote-v3-production.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
 print(json.dumps({key:value for key,value in report.items() if key!='checks'},ensure_ascii=False))
