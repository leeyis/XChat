"""Prototype-only Chrome UI checks. Uses a fresh profile and synthetic remote state.

Run from the repo: rtk proxy python docs/verification/2026-10-10-remote-toolbar-check.py
Requires the prototype HTTP server on 127.0.0.1:18893 and the existing websockets package.
"""
import asyncio
import base64
import json
from pathlib import Path
import subprocess
import tempfile
import time
from urllib.request import urlopen

import websockets


ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / 'docs/verification'
URL = 'http://127.0.0.1:18893/xchat-desktop-prototype.html?review=phase3&p3=remote&toolbar-review=1'
results, exceptions = [], []


def check(name, condition, detail=None):
    results.append(dict(name=name, passed=bool(condition), detail=detail))
    if not condition:
        raise AssertionError(f'{name}: {detail}')


async def run(ws):
    sequence = 0

    async def call(method, params=None):
        nonlocal sequence
        sequence += 1
        token = sequence
        await ws.send(json.dumps(dict(id=token, method=method, params=params or {})))
        while True:
            reply = json.loads(await asyncio.wait_for(ws.recv(), 30))
            if reply.get('method') == 'Runtime.exceptionThrown':
                exceptions.append(reply['params']['exceptionDetails'])
            if reply.get('id') == token:
                if 'error' in reply:
                    raise RuntimeError(reply['error'])
                return reply.get('result', {})

    async def js(source, wait=False):
        reply = await call('Runtime.evaluate', dict(expression=source, awaitPromise=wait, returnByValue=True))
        if 'exceptionDetails' in reply:
            raise RuntimeError(reply['exceptionDetails'])
        return reply.get('result', {}).get('value')

    async def wait_for(source):
        for _ in range(100):
            if await js(source):
                return
            await asyncio.sleep(.05)
        raise TimeoutError(source)

    async def click(selector):
        await js(f'''(()=>{{const el=[...document.querySelectorAll({json.dumps(selector)})].find(e=>e.checkVisibility()&&!e.disabled);if(!el)throw Error('Missing visible control: '+{json.dumps(selector)});el.click();}})()''')

    async def act(action, suffix=''):
        await click(f'[data-p3-action="{action}"]{suffix}')

    async def state():
        return await js('''(()=>{
          const p=document.querySelector('.ra-floating-panel'); if(!p)return null;
          const r=p.getBoundingClientRect(),s=p.parentElement.getBoundingClientRect();
          return {x:r.x,y:r.y,width:r.width,height:r.height,right:r.right,bottom:r.bottom,
            localX:r.x-s.x,localY:r.y-s.y,hidden:p.classList.contains('collapsed'),dragging:p.classList.contains('dragging'),
            fits:r.x>=s.x+7&&r.y>=s.y+7&&r.right<=s.right-7&&r.bottom<=s.bottom-7&&r.right<=innerWidth&&r.bottom<=innerHeight,
            status:p.innerText,permission:document.querySelector('.ra-session-permission')?.innerText,
            desktop:!!document.querySelector('.ra-desktop'),voice:!!document.querySelector('.ra-voice-bar.active'),focus:document.activeElement.className};
        })()''')

    async def drag(dx, dy):
        point = await js('''(()=>{const p=[...document.querySelectorAll('.ra-panel-grip')].find(e=>e.checkVisibility()).getBoundingClientRect();return {x:p.x+p.width/2,y:p.y+p.height/2};})()''')
        await call('Input.dispatchMouseEvent', dict(type='mousePressed', **point, button='left', clickCount=1))
        await call('Input.dispatchMouseEvent', dict(type='mouseMoved', x=point['x']+dx, y=point['y']+dy, button='left', buttons=1))
        await call('Input.dispatchMouseEvent', dict(type='mouseReleased', x=point['x']+dx, y=point['y']+dy, button='left', clickCount=1))
        await asyncio.sleep(.06)

    async def start():
        await act('reset')
        await act('remote-start', '[data-mode=control]')
        await act('remote-send')
        await act('role')
        await act('remote-accept', '[data-permission=control]')
        await wait_for("!!document.querySelector('.ra-floating-panel.host')")

    async def shot(name):
        reply = await call('Page.captureScreenshot', dict(format='png', captureBeyondViewport=False))
        (OUT / f'2026-10-10-remote-toolbar-{name}.png').write_bytes(base64.b64decode(reply['data']))

    await call('Runtime.enable')
    await call('Page.enable')
    await call('Emulation.setDeviceMetricsOverride', dict(width=1280, height=900, deviceScaleFactor=1, mobile=False))
    await call('Page.navigate', dict(url=URL))
    await wait_for("!!document.querySelector('[data-p3-action=remote-start]')")
    await start()
    initial = await state()
    check('共享方面板初始可见且在画面范围内', initial['fits'] and not initial['hidden'], initial)
    await drag(-90, 130)
    moved = await state()
    check('鼠标手柄可真实拖动，释放后结束拖动', abs(moved['y']-initial['y']-130)<2 and not moved['dragging'], moved)
    await shot('host-light')
    await act('remote-toolbar-hide')
    hidden = await state()
    check('隐藏仅收起面板，共享与控制保持，焦点进入浮标', hidden['hidden'] and hidden['desktop'] and hidden['permission']=='允许控制' and 'ra-panel-restore' in hidden['focus'], hidden)
    check('隐藏工具栏保留双向语音', hidden['voice'])
    await drag(60, 45)
    compact = await state()
    check('浮标支持独立拖动', abs(compact['y']-hidden['y']-45)<2, compact)
    await act('remote-toolbar-restore')
    restored = await state()
    check('展开回到完整面板原位置并恢复焦点', not restored['hidden'] and abs(restored['x']-moved['x'])<2 and abs(restored['y']-moved['y'])<2 and 'ra-panel-hide' in restored['focus'], restored)
    await act('remote-toolbar-hide')
    await click('.ra-panel-compact [data-p3-action=remote-revoke]')
    revoked = await state()
    check('隐藏时可直接撤销控制且浮标及时更新', revoked['hidden'] and revoked['permission']=='仅查看' and '屏幕共享中' in revoked['status'], revoked)
    await act('remote-toolbar-restore')
    await act('remote-pause')
    await act('remote-toolbar-hide')
    paused = await state()
    check('共享暂停后浮标保留暂停状态', paused['hidden'] and not paused['desktop'] and '共享已暂停' in paused['status'], paused)
    await act('remote-toolbar-restore')
    await act('remote-resume')
    resumed = await state()
    check('恢复共享保留面板位置且不继承控制', resumed['permission']=='仅查看' and abs(resumed['x']-moved['x'])<2 and abs(resumed['y']-moved['y'])<2, resumed)
    await drag(-3000, -3000)
    edge = await state()
    check('向屏幕外拖动自动限制到可见范围', edge['fits'] and abs(edge['localX']-8)<2 and abs(edge['localY']-8)<2, edge)
    await call('Input.dispatchKeyEvent', dict(type='keyDown', key='ArrowRight', code='ArrowRight', windowsVirtualKeyCode=39))
    await call('Input.dispatchKeyEvent', dict(type='keyUp', key='ArrowRight', code='ArrowRight', windowsVirtualKeyCode=39))
    keyboard = await state()
    check('手柄方向键移动八像素', abs(keyboard['x']-edge['x']-8)<2, keyboard)
    await call('Input.dispatchKeyEvent', dict(type='keyDown', key='Home', code='Home', windowsVirtualKeyCode=36))
    await call('Input.dispatchKeyEvent', dict(type='keyUp', key='Home', code='Home', windowsVirtualKeyCode=36))
    check('Home 复位到顶部居中', (await state())['localY']<15)
    await act('role')
    viewer = await state()
    check('查看方使用独立的浮动操作栏', viewer['fits'] and not viewer['hidden'] and '适应窗口' in viewer['status'], viewer)
    await drag(-40, 100)
    viewer_moved = await state()
    await act('remote-quality')
    check('普通工具按钮打开对话框且不触发拖动', await js("document.querySelector('#phase3Dialog').open&&!document.querySelector('.ra-floating-panel').classList.contains('dragging')"))
    await act('close')
    await act('remote-toolbar-hide')
    await act('role')
    check('切换视角互不污染折叠状态', not (await state())['hidden'])
    await act('role')
    check('返回查看方保留折叠状态', (await state())['hidden'])
    await act('remote-toolbar-restore')
    check('重新渲染保留查看方面板位置', abs((await state())['y']-viewer_moved['y'])<2)

    for width in [1280, 860, 390]:
        await call('Emulation.setDeviceMetricsOverride', dict(width=width, height=844, deviceScaleFactor=1, mobile=False))
        for theme in ['light', 'dark']:
            await js(f'document.documentElement.dataset.theme={json.dumps(theme)}')
            for role in ['viewer', 'host']:
                if role=='host':
                    await act('role')
                await asyncio.sleep(.08)
                full = await state()
                check(f'{width}px {theme} {role} 展开后边界完整', full['fits'], full)
                await act('remote-toolbar-hide')
                folded = await state()
                check(f'{width}px {theme} {role} 浮标边界完整', folded['fits'], folded)
                if width==1280 and theme=='dark' and role=='host':
                    await shot('compact-dark')
                await act('remote-toolbar-restore')
                if width==390 and theme=='light' and role=='viewer':
                    await shot('viewer-390')
                if role=='host':
                    await act('role')
    await act('remote-expand')
    await asyncio.sleep(.08)
    check('展开远程桌面后自动约束位置', (await state())['fits'])
    await act('remote-toolbar-hide')
    await call('Emulation.setDeviceMetricsOverride', dict(width=860, height=700, deviceScaleFactor=1, mobile=False))
    await asyncio.sleep(.08)
    check('浮标在窗口缩小和展开模式中持续可见', (await state())['fits'])
    await click('.ra-panel-compact [data-p3-action=remote-end]')
    check('隐藏状态仍可结束协助并移除浮标', await js("!document.querySelector('.ra-floating-panel')&&!document.body.classList.contains('p3-remote-live')"))
    await start()
    check('新会话重新显示面板', not (await state())['hidden'])
    await act('remote-toolbar-hide')
    await js("const s=document.querySelector('#p3Scenario');s.value='disconnect';s.dispatchEvent(new Event('change',{bubbles:true}));")
    check('断线清理隐藏浮标', await js("!document.querySelector('.ra-floating-panel')"))
    await call('Emulation.setDeviceMetricsOverride', dict(width=1280, height=900, deviceScaleFactor=1, mobile=False))
    await js("document.documentElement.dataset.theme='light'")
    regression = (OUT / '2026-10-10-remote-v2-interaction-check.js').read_text(encoding='utf-8')
    await js(regression, True)
    old = await js('window.__raV2QA')
    check('既有远程授权、语音及状态流程回归', old and old.get('failed')==0, old)
    for role in ['host', 'viewer']:
        await call('Page.navigate', dict(url=URL+'&toolbar='+role))
        await wait_for(f"!!document.querySelector('.ra-floating-panel.{role}')")
        preview = await state()
        check(f'{role} 原型直达入口显示模拟会话', preview['fits'] and preview['desktop'] and preview['voice'], preview)
        if role=='viewer':
            await shot('viewer-light')
    check('无浏览器未捕获异常', not exceptions, exceptions)
    await call('Browser.close')


async def main():
    profile = Path(tempfile.mkdtemp(prefix='xchat-toolbar-review-'))
    browser = subprocess.Popen([
        'C:/Program Files/Google/Chrome/Application/chrome.exe', '--headless=new', '--disable-gpu',
        '--no-first-run', '--no-default-browser-check', '--disable-extensions', '--mute-audio',
        '--remote-debugging-port=0', '--user-data-dir='+str(profile), 'about:blank'
    ], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        for _ in range(100):
            if (profile/'DevToolsActivePort').exists():
                break
            await asyncio.sleep(.1)
        port = (profile/'DevToolsActivePort').read_text().splitlines()[0]
        with urlopen('http://127.0.0.1:'+port+'/json/list', timeout=5) as response:
            target = next(p for p in json.load(response) if p['type']=='page')
        async with websockets.connect(target['webSocketDebuggerUrl'], max_size=16*1024*1024) as ws:
            await run(ws)
    finally:
        try:
            browser.wait(timeout=5)
        except subprocess.TimeoutExpired:
            browser.terminate()
        report = dict(method='Independent headless Chrome CDP, real pointer/keyboard input; prototype states only',
                      url=URL, time=time.strftime('%Y-%m-%d %H:%M:%S'), results=results, exceptions=exceptions,
                      passed=sum(r['passed'] for r in results), failed=sum(not r['passed'] for r in results))
        (OUT/'2026-10-10-remote-toolbar.json').write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
        print(json.dumps({k:v for k,v in report.items() if k not in ['results','exceptions']}, ensure_ascii=False))


if __name__=='__main__':
    asyncio.run(main())
