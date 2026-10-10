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
        await call('Page.navigate',dict(url=f'http://127.0.0.1:{ports[0]}/'));await asyncio.sleep(1)
        with sqlite3.connect(temp/str(ports[0])/'xchat.db',timeout=20) as db:
            offline='qa-offline';offline_conversation='direct:'+':'.join(sorted([identities[0],offline]))
            db.execute("INSERT INTO conversations(id,kind,peer_id,title,created_at,updated_at) VALUES(?,'direct',?,'离线测试',1,1)",(offline_conversation,offline))
            for member in (identities[0],offline):db.execute("INSERT INTO conversation_members(conversation_id,peer_id,display_name,role,joined_at) VALUES(?,?,?,'member',1)",(offline_conversation,member,member))
            db.execute("INSERT INTO messages(sender_id,receiver_id,content,msg_type,timestamp,status,conversation_id,client_message_id) VALUES(?,?,'待确认测试消息','text',?,'sent',?,?)",(identities[0],offline,int(time.time()),offline_conversation,str(uuid.uuid4())))
            text_id=db.execute('SELECT last_insert_rowid()').fetchone()[0]
            client_id=db.execute('SELECT client_message_id FROM messages WHERE id=?',(text_id,)).fetchone()[0]
            db.execute("INSERT INTO message_receipts(message_client_id,reader_id,updated_at) VALUES(?,?,1)",(client_id,offline))
            db.execute("INSERT INTO message_delivery_attempts(message_client_id,reader_id,state,attempt_count,last_written_at) VALUES(?,?,'unconfirmed',2,1) ON CONFLICT(message_client_id,reader_id) DO UPDATE SET state='unconfirmed',attempt_count=2,last_written_at=1",(client_id,offline))
        def upload(path,data,name='source.bin',port=ports[0]):
            boundary='qa-'+uuid.uuid4().hex
            body=(f'--{boundary}\r\nContent-Disposition: form-data; name="file"; filename="{name}"\r\nContent-Type: application/octet-stream\r\n\r\n').encode()+data+(f'\r\n--{boundary}--\r\n').encode()
            return json.load(urlopen(Request(f'http://127.0.0.1:{port}'+path,body,{'Content-Type':'multipart/form-data; boundary='+boundary}),timeout=30))
        queued=upload(f'/api/conversations/{offline_conversation}/files',b'original content')
        file_id=queued['message']['id']
        await js("document.querySelector('[aria-label=\"任务中心\"]').click()")
        await asyncio.sleep(.5)
        assert await js("document.querySelectorAll('.task-row').length>=3")
        async def row_action(title,button):
            await js("[...document.querySelectorAll('.task-row')].find(r=>r.innerText.includes("+json.dumps(title)+")).querySelectorAll('button') && [...[...document.querySelectorAll('.task-row')].find(r=>r.innerText.includes("+json.dumps(title)+")).querySelectorAll('button')].find(b=>b.textContent==="+json.dumps(button)+").click()")
            await asyncio.sleep(.6)
        await row_action('待确认测试消息','取消')
        page=api(ports[0],'/api/tasks');text_task=next(t for t in page['tasks'] if t['message']['id']==text_id)
        assert text_task['recipients'][0]['state']=='cancelled'
        await row_action('source.bin','取消')
        try:upload(f'/api/tasks/{file_id}/source',b'modified content')
        except Exception as e:assert getattr(e,'code',0)>=400
        else:raise AssertionError('wrong source accepted')
        upload(f'/api/tasks/{file_id}/source',b'original content')
        page=api(ports[0],'/api/tasks');file_task=next(t for t in page['tasks'] if t['message']['id']==file_id)
        assert file_task['message']['client_message_id']==queued['message']['client_message_id']
        await row_action('source.bin','重试')
        page=api(ports[0],'/api/tasks');file_task=next(t for t in page['tasks'] if t['message']['id']==file_id)
        assert file_task['recipients'][0]['state']=='waiting_peer'
        diagnostic=api(ports[0],'/api/diagnostics',dict(peer_id=identities[1],include_addresses=False))
        assert next(c for c in diagnostic['checks'] if c['key']=='identity')['state']=='ok'
        assert diagnostic['endpoint'] is None
        assert all(i['addresses'] is None and i['name'] is None for i in diagnostic['interfaces'])
        await js("[...document.querySelectorAll('.task-title button')].find(b=>b.innerText.includes('待确认测试消息')).click()")
        layouts=[]
        for width in (1280,860,390):
            await call('Emulation.setDeviceMetricsOverride',dict(width=width,height=850,deviceScaleFactor=1,mobile=False));await asyncio.sleep(.3)
            layout=await js("(()=>{const main=document.querySelector('.task-center').getBoundingClientRect(),nav=document.querySelector('[aria-label=\"任务中心\"]').getBoundingClientRect();return {width:innerWidth,left:main.left,right:main.right,height:main.height,navBottom:nav.bottom,scroll:document.documentElement.scrollWidth}})()")
            assert layout['right']<=width+1 and layout['height']>250 and layout['scroll']<=width+1,layout
            assert layout['navBottom']<=850,layout
            layouts.append(layout)
            shot=await call('Page.captureScreenshot',dict(format='png'));(temp/f'tasks-{width}.png').write_bytes(base64.b64decode(shot['data']))
        await call('Emulation.setDeviceMetricsOverride',dict(width=1280,height=850,deviceScaleFactor=1,mobile=False))
        await js("[...document.querySelectorAll('.task-sidebar button')].find(b=>b.textContent==='打开连接诊断').click()")
        await asyncio.sleep(.4)
        await js("document.querySelector('.diagnostic-controls button').click()")
        await asyncio.sleep(1)
        assert await js("document.querySelectorAll('.diagnostic-checks article').length>=3")
        assert await js("document.querySelector('.settings-nav-row[aria-current=location]')?.innerText.includes('连接诊断')")
        shot=await call('Page.captureScreenshot',dict(format='png'));(temp/'diagnostics.png').write_bytes(base64.b64decode(shot['data']))
        task_report=dict(result='passed',checks=['real durable task rows','text cancel','file cancel','wrong source blocked','same source restored','stable message ID','file retry waits for offline peer','peer identity check','default address redaction','diagnostic production UI'],layouts=layouts,exceptions=exceptions,evidence_dir=str(temp))
        (temp/'tasks-result.json').write_text(json.dumps(task_report,ensure_ascii=False,indent=2),encoding='utf-8');print('tasks',json.dumps(task_report,ensure_ascii=False),flush=True)
        assert not exceptions
        await js("[...document.querySelectorAll('.settings-nav-row')].find(b=>b.innerText.includes('备份与恢复')).click()")
        await asyncio.sleep(.7)
        await js("document.querySelector('.backup-hero button').click()")
        assert await js("document.querySelector('[role=dialog]').innerText.includes('创建本地备份')")
        await js("document.querySelectorAll('.backup-option input')[2].click()")
        await js("[...document.querySelectorAll('[role=dialog] button')].find(b=>b.textContent==='开始备份').click()")
        for _ in range(100):
            if await js("document.querySelector('[role=dialog]')?.innerText.includes('备份已完成')"):break
            await asyncio.sleep(.2)
        assert await js("document.querySelector('[role=dialog]')?.innerText.includes('备份已完成')"),await js('document.body.innerText.slice(-3000)')
        backup=api(ports[0],'/api/backups')['backups'][0]
        payload=urlopen(f'http://127.0.0.1:{ports[0]}/api/backups/{backup["id"]}/download').read()
        with sqlite3.connect(temp/str(ports[0])/'xchat.db') as db:
            expected=db.execute('SELECT sha256 FROM backup_records WHERE id=?',(backup['id'],)).fetchone()[0]
        assert hashlib.sha256(payload).hexdigest()==expected
        target_db=temp/str(ports[1])/'xchat.db'
        with sqlite3.connect(target_db) as db:before_count=db.execute('SELECT COUNT(*) FROM messages').fetchone()[0]
        def terminal(port,job_id):
            j=api(port,f'/api/backups/jobs/{job_id}')
            return j if j['status']!='running' else None
        corrupt=bytearray(payload);corrupt[-1]^=1
        failed=upload('/api/backups/import',corrupt,'broken.xchatbackup',ports[1])
        failed=waitfor(lambda:terminal(ports[1],failed['id']))
        assert failed['status']=='failed',failed
        with sqlite3.connect(target_db) as db:assert db.execute('SELECT COUNT(*) FROM messages').fetchone()[0]==before_count
        restoring=upload('/api/backups/import',payload,'valid.xchatbackup',ports[1])
        preview=waitfor(lambda:terminal(ports[1],restoring['id']))
        assert preview['status']=='preview',preview
        assert preview['result']['new_messages']==2 and preview['result']['duplicates']==1,preview
        await call('Page.navigate',dict(url=f'http://127.0.0.1:{ports[1]}/'));await asyncio.sleep(1)
        await js("document.querySelector('[aria-label=\"设置\"]').click()")
        for _ in range(40):
            if await js("document.querySelector('[role=dialog]')?.innerText.includes('确认恢复范围')"):break
            await asyncio.sleep(.2)
        assert await js("document.querySelector('[role=dialog]')?.innerText.includes('确认恢复范围')"),await js('document.body.innerText.slice(-3000)')
        assert await js("document.querySelector('[role=dialog] .backup-option input').checked===false")
        backup_layouts=[]
        for width in (1280,860,390):
            await call('Emulation.setDeviceMetricsOverride',dict(width=width,height=850,deviceScaleFactor=1,mobile=False));await asyncio.sleep(.2)
            layout=await js("(()=>{const r=document.querySelector('[role=dialog]').getBoundingClientRect();return{width:innerWidth,left:r.left,right:r.right,bottom:r.bottom,scroll:document.documentElement.scrollWidth}})()")
            assert layout['left']>=0 and layout['right']<=width and layout['bottom']<=850 and layout['scroll']<=width,layout
            backup_layouts.append(layout)
            shot=await call('Page.captureScreenshot',dict(format='png'));(temp/f'backup-{width}.png').write_bytes(base64.b64decode(shot['data']))
        await js("[...document.querySelectorAll('[role=dialog] button')].find(b=>b.textContent==='确认合并恢复').click()")
        merged=waitfor(lambda:terminal(ports[1],restoring['id']))
        assert merged['status']=='done',merged
        assert merged['result']['added']==2 and Path(merged['result']['pre_restore_copy']).is_file()
        with sqlite3.connect(target_db) as db:
            assert db.execute("SELECT value FROM settings WHERE key='user_id'").fetchone()[0]==identities[1]
            assert db.execute('SELECT COUNT(*) FROM messages').fetchone()[0]==before_count+2
            historical=db.execute("SELECT client_message_id,file_path FROM messages WHERE status='restored'").fetchall()
            assert len(historical)==2
            for client_id,file_path in historical:
                assert db.execute('SELECT COUNT(*) FROM message_delivery_attempts WHERE message_client_id=?',(client_id,)).fetchone()[0]==0
                if file_path:assert Path(file_path).read_bytes()==b'original content'
        repeated=upload('/api/backups/import',payload,'valid.xchatbackup',ports[1])
        repeated=waitfor(lambda:terminal(ports[1],repeated['id']))
        assert repeated['result']['new_messages']==0 and repeated['result']['duplicates']==3
        cancelled=api(ports[1],f'/api/backups/jobs/{repeated["id"]}/cancel',{})
        assert cancelled['status']=='cancelled'
        backup_report=dict(result='passed',checks=['production backup dialog and attachment option','completed archive download hash','corrupt payload refused without mutation','import validation and preview','preferences unchecked by default','transactional cross-device merge','existing duplicate preserved','device identity preserved','attachments restored','no historical delivery attempts','pre-restore snapshot retained','repeat import deduplicated','preview cancellation'],layouts=backup_layouts,exceptions=exceptions,evidence_dir=str(temp))
        (temp/'backup-result.json').write_text(json.dumps(backup_report,ensure_ascii=False,indent=2),encoding='utf-8');print('backup',json.dumps(backup_report,ensure_ascii=False),flush=True)
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
