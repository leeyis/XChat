import asyncio,base64,json,subprocess,tempfile,time,sqlite3,uuid,hashlib
from pathlib import Path
from urllib.request import urlopen,Request
import websockets
import argparse
parser=argparse.ArgumentParser()
parser.add_argument("--binary",required=True)
args=parser.parse_args()

root=Path.cwd().resolve();temp=Path(tempfile.mkdtemp(prefix='xchat-p3-e2e-')); processes=[]; logs=[]
def api(port,path,body=None):
    req=Request(f'http://127.0.0.1:{port}'+path,None if body is None else json.dumps(body).encode(),{'Content-Type':'application/json'})
    with urlopen(req,timeout=20) as r: return json.load(r)
def waitfor(f,seconds=40):
    end=time.time()+seconds
    while time.time()<end:
        try:
            value=f()
            if value:return value
        except Exception:pass
        time.sleep(.2)
    raise RuntimeError('Timed out waiting for '+str(f))

async def run():
    identities=[str(uuid.uuid4()),str(uuid.uuid4())];ports=[18921,18922]
    for index,port in enumerate(ports):
        dbdir=temp/str(port);dbdir.mkdir();(dbdir/'downloads').mkdir()
        with sqlite3.connect(dbdir/'xchat.db') as db:
            db.execute('CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT NOT NULL)')
            db.executemany('INSERT INTO settings VALUES(?,?)',[
                ('user_id',identities[index]),('username','语音验证 '+str(index+1)),('username_source','custom'),
                ('download_path',str(dbdir/'downloads')),('auto_download','false'),
                ('network.discovery.settings.v1',json.dumps(dict(local_discovery=False,vpn_discovery=False,interface_overrides={})))])
            db.execute('CREATE TABLE users(id TEXT PRIMARY KEY,name TEXT,addr TEXT,last_seen INTEGER,is_offline INTEGER DEFAULT 0,available_memory_mb INTEGER DEFAULT 0,hostname TEXT,mac_address TEXT,remark TEXT,discovery_source TEXT,app_version TEXT)')
            db.execute('INSERT INTO users(id,name,addr,last_seen,is_offline,discovery_source) VALUES(?,?,?,?,0,?)',(identities[1-index],'语音验证 '+str(2-index),f'127.0.0.1:{ports[1-index]}',int(time.time()),'manual'))
        log=(temp/f'{port}.log').open('w',encoding='utf-8');logs.append(log)
        processes.append(subprocess.Popen([str(Path(args.binary).resolve()),'--port',str(port),'--db-path',str(dbdir)],stdout=log,stderr=log))
        waitfor(lambda:api(port,'/api/health').get('http_ready'))
    for index,port in enumerate(ports):
        other=ports[1-index];api(port,'/api/add_custom_peer',dict(peer=f'127.0.0.1:{other}',expected_device_id=identities[1-index]))
        api(port,f'/api/peers/{identities[1-index]}/refresh',{})
    conversation='direct:'+':'.join(sorted(identities))
    waitfor(lambda:any(c['id']==conversation for c in api(ports[0],'/api/workspace')['conversations']))
    profile=temp/'chrome';profile.mkdir()
    browser=subprocess.Popen([r'C:/Program Files/Google/Chrome/Application/chrome.exe','--headless=new','--disable-gpu','--disable-extensions','--no-first-run','--no-default-browser-check','--autoplay-policy=no-user-gesture-required','--use-fake-ui-for-media-stream','--use-fake-device-for-media-stream','--remote-debugging-port=0','--user-data-dir='+str(profile),'about:blank'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL);processes.append(browser)
    waitfor(lambda:(profile/'DevToolsActivePort').exists());cdp=(profile/'DevToolsActivePort').read_text().splitlines()[0]
    target=next(t for t in json.load(urlopen(f'http://127.0.0.1:{cdp}/json/list')) if t['type']=='page')
    async with websockets.connect(target['webSocketDebuggerUrl'],max_size=16*1024*1024) as ws:
        seq=0;exceptions=[]
        async def call(method,params=None):
            nonlocal seq
            seq+=1;token=seq;await ws.send(json.dumps(dict(id=token,method=method,params=params or {})))
            while True:
                result=json.loads(await asyncio.wait_for(ws.recv(),50))
                if result.get('method')=='Runtime.exceptionThrown':exceptions.append(result['params'])
                if result.get('id')==token:
                    if 'error' in result:raise RuntimeError(result['error'])
                    return result.get('result',{})
        async def js(source):
            result=await call('Runtime.evaluate',dict(expression=source,awaitPromise=True,returnByValue=True))
            if 'exceptionDetails' in result:raise RuntimeError(result['exceptionDetails'])
            return result.get('result',{}).get('value')
        await call('Runtime.enable');await call('Page.enable')
        await call('Emulation.setDeviceMetricsOverride',dict(width=1280,height=850,deviceScaleFactor=1,mobile=False))
        await call('Page.navigate',dict(url=f'http://127.0.0.1:{ports[0]}/'))
        await asyncio.sleep(2)
        # Use the production recorder and send button. Only the browser's microphone source is synthetic.
        for _ in range(60):
            if await js("!!document.querySelector('[aria-label=\"录制语音消息\"]')"):break
            await js("[...document.querySelectorAll('button')].find(b=>b.innerText.includes('语音验证 2'))?.click()")
            await asyncio.sleep(.3)
        print('sender ready',await js('document.body.innerText.slice(0,700)'),flush=True)
        assert await js("!!document.querySelector('[aria-label=\"录制语音消息\"]')")
        await js("document.querySelector('[aria-label=\"录制语音消息\"]').click()")
        await asyncio.sleep(2.2)
        assert await js("!document.querySelector('[aria-label=\"发送语音\"]').disabled")
        await js("document.querySelector('[aria-label=\"发送语音\"]').click()")
        sent=waitfor(lambda: next((m for m in api(ports[0],f'/api/conversations/{conversation}/messages')['messages'] if m.get('msg_type')=='voice'),None))
        received=waitfor(lambda: next((m for m in api(ports[1],f'/api/conversations/{conversation}/messages')['messages'] if m.get('msg_type')=='voice' and m.get('file_status')=='accepted'),None))
        assert sent['client_message_id']==received['client_message_id']
        assert sent['voice']==received['voice']
        assert hashlib.sha256(Path(sent['file_path']).read_bytes()).digest()==hashlib.sha256(Path(received['file_path']).read_bytes()).digest()
        await call('Page.navigate',dict(url=f'http://127.0.0.1:{ports[1]}/'));await asyncio.sleep(2)
        for _ in range(60):
            if await js("!!document.querySelector('.voice-bubble')"):break
            await js("[...document.querySelectorAll('button')].find(b=>b.innerText.includes('语音验证 1'))?.click()")
            await asyncio.sleep(.3)
        assert await js("!!document.querySelector('.voice-bubble')")
        playback=await js("(async()=>{const a=document.querySelector('.voice-message audio');a.muted=true;document.querySelector('.voice-bubble').click();await new Promise(r=>setTimeout(r,700));const result={currentTime:a.currentTime,error:a.error?.code,paused:a.paused};a.pause();return result})()")
        assert playback['currentTime']>0 and not playback.get('error'),playback
        shot=await call('Page.captureScreenshot',dict(format='png'));(temp/'voice-received.png').write_bytes(base64.b64decode(shot['data']))
        report=dict(result='passed',id=sent['client_message_id'],duration_ms=sent['voice']['duration_ms'],mime=sent['voice']['mime_type'],bytes=received['file_size'],playback=playback,exceptions=exceptions,evidence_dir=str(temp),receiver_auto_download=False)
        (temp/'result.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8');print(json.dumps(report,ensure_ascii=False),flush=True)
        assert not exceptions
        await call('Browser.close')
try:asyncio.run(run())
finally:
    for p in reversed(processes):
        if p.poll() is None:p.terminate()
    for p in processes:
        try:p.wait(timeout=5)
        except subprocess.TimeoutExpired:p.kill()
    for log in logs:log.close()
    print('evidence',temp,flush=True)
