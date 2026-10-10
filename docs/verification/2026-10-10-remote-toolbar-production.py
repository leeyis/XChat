"""Compare actual React controls against approved HTML, then exercise real pointer input.
Uses isolated Chrome/Vite and UI fixtures only. Native/media validation is separate.
"""
import asyncio, ast, base64, io, json, subprocess, tempfile, time
from pathlib import Path
from urllib.request import urlopen
import websockets
from PIL import Image, ImageChops, ImageStat

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'docs/verification'
tree=ast.parse((OUT/'2026-10-10-remote-e2e.py').read_text(encoding='utf-8'))
exec(compile(ast.Module(body=[n for n in tree.body if isinstance(n,(ast.ClassDef,ast.FunctionDef)) and n.name in ['Page','waitfor']],type_ignores=[]),'<qa-common>','exec'))
temp=Path(tempfile.mkdtemp(prefix='xchat-toolbar-production-'));processes=[];logs=[];checks=[];comparisons=[]

def check(name,ok,detail=None):
 checks.append(dict(name=name,passed=bool(ok),detail=detail))
 if not ok:raise AssertionError(name+': '+str(detail))

async def run():
 fixture=(OUT/'2026-10-10-remote-toolbar-fixture.jsx').as_posix()
 html='<html><head><meta name="viewport" content="width=device-width,initial-scale=1"></head><body><div id="root"></div><script type="module" src="/@fs/'+fixture+'"></script></body></html>'
 script=temp/'vite.mjs'
 script.write_text('import {createServer} from '+json.dumps((ROOT/'node_modules/vite/dist/node/index.js').as_uri())+';const server=await createServer({configFile:'+json.dumps(str(ROOT/'vite.config.js'))+',server:{host:"127.0.0.1",port:18941,strictPort:true},plugins:[{name:"isolated-toolbar-qa",configureServer(s){s.middlewares.use(async(req,res,next)=>{if(!req.url.startsWith("/__remote-toolbar-qa__"))return next();res.setHeader("Content-Type","text/html");res.end(await s.transformIndexHtml(req.url,'+json.dumps(html)+'));});}}]});await server.listen();',encoding='utf-8')
 log=(temp/'vite.log').open('w',encoding='utf-8');logs.append(log)
 processes.append(subprocess.Popen(['rtk','proxy','node',str(script)],cwd=ROOT,stdout=log,stderr=log,creationflags=subprocess.CREATE_NO_WINDOW))
 waitfor(lambda:urlopen('http://127.0.0.1:18941/__remote-toolbar-qa__',timeout=2).status==200)
 profile=temp/'chrome';profile.mkdir()
 processes.append(subprocess.Popen(['C:/Program Files/Google/Chrome/Application/chrome.exe','--headless=new','--disable-gpu','--disable-extensions','--no-first-run','--mute-audio','--remote-debugging-port=0','--user-data-dir='+str(profile),'about:blank'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,creationflags=subprocess.CREATE_NO_WINDOW))
 waitfor(lambda:(profile/'DevToolsActivePort').exists());port=(profile/'DevToolsActivePort').read_text().splitlines()[0]
 target=next(t for t in json.load(urlopen(f'http://127.0.0.1:{port}/json/list')) if t['type']=='page')
 page=await Page().connect(target);await page.call('Runtime.enable');await page.call('Page.enable')
 async def click(selector):
  await page.js(f"(()=>{{const e=[...document.querySelectorAll({json.dumps(selector)})].find(e=>e.checkVisibility());if(!e)throw Error('Missing '+{json.dumps(selector)});e.click();}})()")
  await asyncio.sleep(.07)
 async def snapshot():
  # Equalize irrelevant screen pixels and subpixel crop origins; preserve actual controls.
  await page.js("""(()=>{document.activeElement?.blur();document.querySelectorAll('.ra-desktop,.ra-chat-panel').forEach(e=>e.style.visibility='hidden');document.querySelectorAll('.ra-canvas,.remote-video-viewport').forEach(e=>e.style.background='#242b30');const p=document.querySelector('.ra-floating-panel'),s=p.parentElement.getBoundingClientRect();p.style.left=(Math.ceil(s.x+8)-s.x)+'px';p.style.top=(Math.ceil(s.y+50)-s.y)+'px';p.parentElement.style.background='#242b30';})()""")
  await page.call('Input.dispatchMouseEvent',dict(type='mouseMoved',x=0,y=0))
  await asyncio.sleep(.05)
  info=await page.js("""(()=>{const p=document.querySelector('.ra-floating-panel'),r=p.getBoundingClientRect();return{rect:{x:r.x,y:r.y,width:r.width,height:r.height},elements:[p,...p.querySelectorAll('*')].filter(e=>e.checkVisibility()&&(!e.closest('svg')||e.tagName==='svg')).map(e=>{const s=getComputedStyle(e),b=e.getBoundingClientRect();return {tag:e.tagName,text:e.children.length?'':e.textContent,rect:{x:b.x-r.x,y:b.y-r.y,width:b.width,height:b.height},styles:Object.fromEntries(['fontFamily','fontSize','fontWeight','lineHeight','padding','gap','borderRadius','borderColor','backgroundColor','color','minHeight','display','alignItems','justifyContent','strokeWidth'].map(k=>[k,s[k]]))};})};})()""")
  r=info['rect'];shot=await page.call('Page.captureScreenshot',dict(format='png',captureBeyondViewport=False,clip=dict(x=r['x'],y=r['y'],width=r['width'],height=r['height'],scale=1)))
  return info,base64.b64decode(shot['data'])
 for width in [1280,860,390]:
  await page.call('Emulation.setDeviceMetricsOverride',dict(width=width,height=900,deviceScaleFactor=1,mobile=False))
  for theme in ['light','dark']:
   for role in ['host','viewer']:
    cases={}
    for kind in ['prototype','production']:
     url=f'http://127.0.0.1:18893/xchat-desktop-prototype.html?review=phase3&p3=remote&toolbar={role}' if kind=='prototype' else f'http://127.0.0.1:18941/__remote-toolbar-qa__?role={role}&theme={theme}'
     await page.call('Page.navigate',dict(url=url));await page.wait("!!document.querySelector('.ra-floating-panel')",20)
     await page.js(f'document.documentElement.dataset.theme={json.dumps(theme)}')
     if width==1280 and theme=='light':
      shot=await page.call('Page.captureScreenshot',dict(format='png'))
      (OUT/f'2026-10-10-remote-workspace-{kind}-{role}.png').write_bytes(base64.b64decode(shot['data']))
     for mode in ['full','compact']:
      if mode=='compact':await click('.ra-panel-hide')
      cases[(kind,mode)]=await snapshot()
    for mode in ['full','compact']:
     expected,a=cases[('prototype',mode)];actual,b=cases[('production',mode)]
     im1=Image.open(io.BytesIO(a)).convert('RGB');im2=Image.open(io.BytesIO(b)).convert('RGB')
     diff=ImageChops.difference(im1,im2) if im1.size==im2.size else None
     mismatch=None if diff is None else sum(1 for p in diff.getdata() if max(p)>8)/(im1.width*im1.height)
     differences=[]
     for i,(x,y) in enumerate(zip(expected['elements'],actual['elements'])):
      props={k:[x['styles'][k],y['styles'][k]] for k in x['styles'] if x['styles'][k]!=y['styles'][k]}
      geometry={k:[x['rect'][k],y['rect'][k]] for k in x['rect'] if abs(x['rect'][k]-y['rect'][k])>.1}
      if props or geometry:differences.append(dict(index=i,text=x['text'],styles=props,geometry=geometry))
     record=dict(width=width,theme=theme,role=role,mode=mode,prototypeSize=im1.size,productionSize=im2.size,mismatch=mismatch,differences=differences,elementCounts=[len(expected['elements']),len(actual['elements'])])
     comparisons.append(record)
     prefix=f'{role}-{theme}-{width}-{mode}'
     (temp/(prefix+'-prototype.png')).write_bytes(a);(temp/(prefix+'-production.png')).write_bytes(b)
     if width==1280 and theme=='light' and mode=='full':(OUT/f'2026-10-10-remote-toolbar-production-{role}.png').write_bytes(b)
     print(json.dumps({k:v for k,v in record.items() if k!='differences'}),flush=True)
 check('24 prototype comparisons match geometry and computed styles',all(not r['differences'] and r['elementCounts'][0]==r['elementCounts'][1] for r in comparisons))
 check('24 toolbar screenshots differ by at most 0.2 percent of pixels',all(r['prototypeSize']==r['productionSize'] and r['mismatch']<=.002 for r in comparisons))
 # Behavioral checks use actual React, real CDP pointer events and an in-memory fake session only.
 await page.call('Emulation.setDeviceMetricsOverride',dict(width=1280,height=900,deviceScaleFactor=1,mobile=False))
 await page.call('Page.navigate',dict(url='http://127.0.0.1:18941/__remote-toolbar-qa__?role=host'))
 await page.wait("!!document.querySelector('.ra-floating-panel')")
 async def state():
  return await page.js("""(()=>{const p=document.querySelector('.ra-floating-panel');if(!p)return null;const r=p.getBoundingClientRect(),s=p.parentElement.getBoundingClientRect();return{x:r.x,y:r.y,hidden:p.classList.contains('collapsed'),dragging:p.classList.contains('dragging'),fits:r.x>=s.x+7&&r.right<=s.right-7&&r.y>=s.y+7&&r.bottom<=s.bottom-7,grant:qa.session.grant,paused:qa.session.paused};})()""")
 async def drag(dx,dy):
  xy=await page.js("(()=>{const r=[...document.querySelectorAll('.ra-panel-grip')].find(e=>e.checkVisibility()).getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2};})()")
  await page.call('Input.dispatchMouseEvent',dict(type='mousePressed',**xy,button='left',clickCount=1))
  await page.call('Input.dispatchMouseEvent',dict(type='mouseMoved',x=xy['x']+dx,y=xy['y']+dy,button='left',buttons=1))
  await page.call('Input.dispatchMouseEvent',dict(type='mouseReleased',x=xy['x']+dx,y=xy['y']+dy,button='left',clickCount=1))
  await asyncio.sleep(.05)
 before=await state();await drag(-70,120);moved=await state();check('React panel follows pointer and releases captured remote input',abs(moved['y']-before['y']-120)<1 and not moved['dragging'] and await page.js('qaReleases>0'))
 await click('.ra-panel-hide');hidden=await state();check('fold preserves grant and sharing state',hidden['hidden'] and hidden['grant']=='qa-grant' and not hidden['paused'])
 await drag(35,40);await click('.ra-panel-restore');restored=await state();check('restoring after moving compact handle restores full coordinates',abs(restored['x']-moved['x'])<1 and abs(restored['y']-moved['y'])<1)
 await click('.ra-panel-hide');await click('.ra-panel-compact [aria-label="收回控制"]');check('compact revoke updates control state and stays folded',(await state())['hidden'] and not (await state())['grant'])
 await page.js('qa.setShow(false)');await asyncio.sleep(.05);await page.js('qa.setShow(true)');await asyncio.sleep(.05);check('unmount/remount retains session fold preference',(await state())['hidden'])
 await click('.ra-panel-restore');await drag(-3000,-3000);check('offscreen pointer movement clamps complete panel',(await state())['fits'])
 await page.call('Emulation.setDeviceMetricsOverride',dict(width=390,height=844,deviceScaleFactor=1,mobile=False));await asyncio.sleep(.1);check('window shrink keeps toolbar reachable',(await state())['fits'])
 await click('.ra-panel-hide');await click('.ra-panel-compact [aria-label="结束协助"]');check('compact end removes session UI',await state() is None)
 check('no uncaught browser exceptions',not page.errors,page.errors)
 await page.call('Browser.close')

try:asyncio.run(run())
finally:
 for p in reversed(processes):
  if p.poll() is None:subprocess.run(['rtk','proxy','taskkill','/PID',str(p.pid),'/T','/F'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
 for log in logs:log.close()
 report=dict(comparisons=comparisons,checks=checks,evidence=str(temp))
 (OUT/'2026-10-10-remote-toolbar-production.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8',newline='\n')
 print('Evidence:',temp,flush=True)
