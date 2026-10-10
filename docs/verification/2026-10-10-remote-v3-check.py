"""Remote v3 prototype review; isolated headless Chrome, no user browser/profile.

Run: rtk proxy python -X utf8 docs/verification/2026-10-10-remote-v3-check.py
Requires the prototype server on 127.0.0.1:18942 and the installed websockets package.
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

OUT = Path(__file__).resolve().parent
URL = 'http://127.0.0.1:18942/xchat-desktop-prototype.html?review=remote-v3'
results, exceptions = [], []


def check(name, value, detail=None):
    results.append(dict(name=name, passed=bool(value), detail=detail))
    if not value:
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

    async def js(source, wait=False, gesture=False):
        reply = await call('Runtime.evaluate', dict(expression=source, awaitPromise=wait,
                                                   returnByValue=True, userGesture=gesture))
        if 'exceptionDetails' in reply:
            raise RuntimeError(reply['exceptionDetails'])
        return reply.get('result', {}).get('value')

    async def ready(source):
        for _ in range(100):
            if await js(source):
                return
            await asyncio.sleep(.05)
        raise TimeoutError(source)

    async def click(selector):
        await js(f"document.querySelector({json.dumps(selector)}).click()", gesture=True)

    async def action(name, prefix=''):
        await click(prefix + f'[data-rv-action="{name}"]')

    async def surface(name):
        await click(f'[data-rv-surface="{name}"]')
        await asyncio.sleep(.12)

    async def screenshot(name):
        await asyncio.sleep(.15)
        result = await call('Page.captureScreenshot', dict(format='png', captureBeyondViewport=False))
        (OUT / f'2026-10-10-remote-v3-{name}.png').write_bytes(base64.b64decode(result['data']))

    async def pill():
        return await js("""(()=>{const p=document.querySelector('.rv-sharing-pill'),r=p.getBoundingClientRect();
          return {x:r.x,y:r.y,right:r.right,bottom:r.bottom,width:r.width,height:r.height,
          fits:r.x>=0&&r.right<=innerWidth&&r.y>=45&&r.bottom<=innerHeight,text:p.innerText,
          compact:p.classList.contains('rv-compact')};})()""")

    await call('Runtime.enable')
    await call('Page.enable')
    await call('Emulation.setDeviceMetricsOverride', dict(width=1360, height=900, deviceScaleFactor=1, mobile=False))
    await call('Page.navigate', dict(url=URL))
    await ready("!!document.querySelector('#remoteV3Review .rv-viewer')")
    check('远程窗口不包含聊天或会话导航', await js("!document.querySelector('#remoteV3Review .rv-chat')&&!document.querySelector('#remoteV3Review .rv-rail')"))
    check('性能状态明确标识示例非实测', await js("document.querySelector('.rv-sample').textContent.includes('非实测')"))
    await screenshot('viewer')
    await action('info')
    check('性能弹层标识目标技术尚非实测', await js("document.querySelector('.rv-info-panel').innerText.includes('不代表当前应用已达到')&&document.querySelector('.rv-info-panel').innerText.includes('DXGI')"))
    await screenshot('metrics')
    await click('[data-rv-info-tab="host"]')
    check('主机信息展示系统及显示器', await js("document.querySelector('.rv-info-panel').innerText.includes('Windows 11')&&document.querySelector('.rv-info-panel').innerText.includes('1920 × 1080')"))
    await action('info')
    await action('fullscreen')
    await ready('!!document.fullscreenElement')
    check('浏览器原生全屏可用', await js("document.fullscreenElement.classList.contains('rv-viewer')"))
    await action('control')
    check('全屏中释放控制不会退出全屏', await js("!!document.fullscreenElement&&document.querySelector('.rv-control-label').innerText.includes('仅查看')"))
    await action('fullscreen')
    await ready('!document.fullscreenElement')
    await action('control')
    await surface('chat')
    check('主窗口只保留聊天和远程入口', await js("!!document.querySelector('.rv-chat')&&!document.querySelector('#remoteV3Review .rv-desktop')&&!!document.querySelector('[data-rv-action=popup]')"))
    await action('voice')
    check('语音状态独立留在聊天窗口', await js("document.querySelector('.rv-voice-line').innerText.includes('双方已接通')"))
    await js("document.querySelector('[data-rv-compose] textarea').value='原型消息验证';document.querySelector('[data-rv-compose]').requestSubmit()")
    check('聊天发送可用', await js("document.querySelector('.rv-messages').innerText.includes('原型消息验证')"))
    await screenshot('main-chat')
    await action('popup')
    await asyncio.sleep(.3)
    targets = await call('Target.getTargets')
    popups = [target for target in targets['targetInfos'] if 'detached=1' in target['url']]
    check('打开独立远程窗口生成单独浏览器窗口', len(popups) == 1, popups)
    await call('Target.closeTarget', dict(targetId=popups[0]['targetId']))
    await surface('sharer')
    check('被控方不渲染自己的屏幕预览', await js("!document.querySelector('#remoteV3Review .rv-desktop')&&!!document.querySelector('.rv-sharing-pill')"))
    check('胶囊直接放在桌面无白色外包容器', await js("document.querySelector('.rv-sharing-pill').parentElement.classList.contains('rv-sharer-desktop')"))
    await screenshot('host')
    initial = await pill()
    point = await js("(()=>{const r=document.querySelector('[data-rv-grip]').getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2};})()")
    await call('Input.dispatchMouseEvent', dict(type='mousePressed', **point, button='left', clickCount=1))
    await call('Input.dispatchMouseEvent', dict(type='mouseMoved', x=point['x']-70, y=point['y']+100, button='left', buttons=1))
    await call('Input.dispatchMouseEvent', dict(type='mouseReleased', x=point['x']-70, y=point['y']+100, button='left', clickCount=1))
    moved = await pill()
    check('胶囊可通过真实指针拖动', abs(moved['y']-initial['y']-100) < 2, moved)
    await action('hide-pill')
    check('隐藏后保留可恢复浮标和结束入口', (await pill())['compact'] and await js("!!document.querySelector('.rv-compact [data-rv-action=end]')"))
    await action('control', '.rv-sharing-pill ')
    check('折叠时可收回控制且保留屏幕共享', await js("document.querySelector('.rv-session-copy').innerText.includes('仅可查看')&&document.querySelector('.rv-sharing-pill').classList.contains('rv-compact')"))
    await action('show-pill', '.rv-sharing-pill ')
    await action('pause')
    check('暂停状态明确且控制授权已收回', await js("document.querySelector('.rv-sharing-pill').innerText.includes('共享已暂停')&&document.querySelector('.rv-sharing-pill').innerText.includes('允许控制')"))
    await action('pause')
    for width in [860, 390]:
        await call('Emulation.setDeviceMetricsOverride', dict(width=width, height=844, deviceScaleFactor=1, mobile=False))
        await asyncio.sleep(.15)
        current = await pill()
        check(f'{width}px 胶囊缩放后保持可见', current['fits'], current)
        check(f'{width}px 主体无横向溢出', await js('document.documentElement.scrollWidth<=innerWidth'))
    await screenshot('host-narrow')
    await surface('viewer')
    await action('info')
    check('窄屏信息面板完整位于视口内', await js("(()=>{const r=document.querySelector('.rv-info-panel').getBoundingClientRect();return r.left>=0&&r.right<=innerWidth;})()"))
    await call('Emulation.setDeviceMetricsOverride', dict(width=1360, height=900, deviceScaleFactor=1, mobile=False))
    await surface('chat')
    await action('end')
    check('结束远程协助保留聊天与语音', await js("document.querySelector('.rv-session-copy').innerText.includes('已结束')&&!!document.querySelector('.rv-composer')&&!!document.querySelector('.rv-voice-line')"))
    check('无浏览器未捕获异常', not exceptions, exceptions)
    await call('Browser.close')


async def main():
    profile = Path(tempfile.mkdtemp(prefix='xchat-remote-v3-review-'))
    browser = subprocess.Popen([
        'C:/Program Files/Google/Chrome/Application/chrome.exe', '--headless=new', '--disable-gpu',
        '--no-first-run', '--no-default-browser-check', '--disable-extensions', '--mute-audio',
        '--remote-debugging-port=0', '--user-data-dir=' + str(profile), 'about:blank'
    ], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        creationflags=subprocess.CREATE_NO_WINDOW)
    try:
        for _ in range(100):
            if (profile/'DevToolsActivePort').exists():
                break
            await asyncio.sleep(.1)
        port = (profile/'DevToolsActivePort').read_text().splitlines()[0]
        with urlopen('http://127.0.0.1:' + port + '/json/list', timeout=5) as response:
            target = next(item for item in json.load(response) if item['type'] == 'page')
        async with websockets.connect(target['webSocketDebuggerUrl'], max_size=16*1024*1024) as ws:
            await run(ws)
    finally:
        try:
            browser.wait(timeout=5)
        except subprocess.TimeoutExpired:
            browser.terminate()
        report = dict(method='Isolated headless Chrome CDP; synthetic UI only, no remote stream or microphone',
                      url=URL, time=time.strftime('%Y-%m-%d %H:%M:%S'), results=results, exceptions=exceptions,
                      passed=sum(item['passed'] for item in results), failed=sum(not item['passed'] for item in results))
        (OUT/'2026-10-10-remote-v3-prototype.json').write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
        print(json.dumps({key: value for key, value in report.items() if key not in ['results', 'exceptions']}, ensure_ascii=False))


if __name__ == '__main__':
    asyncio.run(main())
