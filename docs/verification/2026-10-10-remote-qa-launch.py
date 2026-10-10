"""Start an isolated Tauri development app; never attaches to the installed app."""
import argparse
import json
import os
from pathlib import Path
import sqlite3
import socket
import subprocess
import tempfile
import uuid

root = Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser()
parser.add_argument('--resume', type=Path, help='Reuse an already stopped, isolated QA configuration and database')
parser.add_argument('--diagnostic-background-flags',action='store_true',help='QA comparison only; never used by production configuration')
args = parser.parse_args()
if args.resume:
    state = json.loads(args.resume.read_text(encoding='utf-8'))
    evidence = Path(state['evidence']).resolve()
    config = Path(state['config']).resolve()
    database = Path(state['db']).resolve()
    if config.parent != evidence or database.parent != evidence or not evidence.name.startswith('xchat-remote-followup-'):
        raise RuntimeError('Expected an isolated QA configuration')
    environment = dict(os.environ)
    environment['CARGO_BUILD_JOBS'] = '2'
    environment['WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS'] = f"--remote-debugging-port={state['cdp']} --autoplay-policy=no-user-gesture-required --use-fake-device-for-media-stream --use-fake-ui-for-media-stream"
    state['diagnostic_background_flags']=args.diagnostic_background_flags
    if args.diagnostic_background_flags:
        environment['WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS'] += ' --disable-background-timer-throttling --disable-renderer-backgrounding --disable-backgrounding-occluded-windows'
    with Path(state['log']).open('w', encoding='utf-8') as log:
        child = subprocess.Popen([
            'rtk', 'cargo', 'tauri', 'dev', '--no-watch', '--config', str(config),
            '--', '--', '--port', str(state['port']), '--db-path', str(database),
        ], cwd=root, env=environment, stdout=log, stderr=log, creationflags=subprocess.CREATE_NO_WINDOW)
    state['pid'] = child.pid
    args.resume.write_text(json.dumps(state, indent=2), encoding='utf-8')
    print(json.dumps(state, indent=2))
    raise SystemExit(0)
evidence = Path(tempfile.mkdtemp(prefix="xchat-remote-followup-"))
database = evidence / "db"
database.mkdir()
with sqlite3.connect(database / "xchat.db") as db:
    db.execute("CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT NOT NULL)")
    db.executemany("INSERT INTO settings VALUES(?,?)", [
        ("user_id", str(uuid.uuid4())), ("username", "Remote QA Windows"),
        ("username_source", "custom"), ("download_path", str(evidence / "downloads")),
        ("network.discovery.settings.v1", json.dumps({"local_discovery": False, "vpn_discovery": False, "interface_overrides": {}})),
    ])
config = evidence / "tauri-remote-qa.json"
config.write_text(json.dumps({
    "identifier": "com.xchat.remote.followup.qa",
    "app": {"windows": [{"label": "main", "title": "XChat Remote QA", "width": 1180, "height": 760, "minWidth": 860, "minHeight": 640}]},
}), encoding="utf-8")
environment = dict(os.environ)
environment['CARGO_BUILD_JOBS'] = '2'
with socket.socket() as probe:
    probe.bind(('127.0.0.1',0))
    cdp=probe.getsockname()[1]
environment["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"] = f"--remote-debugging-port={cdp} --autoplay-policy=no-user-gesture-required --use-fake-device-for-media-stream --use-fake-ui-for-media-stream"
with (evidence / "tauri.log").open("w", encoding="utf-8") as log:
    child = subprocess.Popen([
        "rtk", "cargo", "tauri", "dev", "--no-watch", "--config", str(config),
        "--", "--", "--port", "18888", "--db-path", str(database),
    ], cwd=root, env=environment, stdout=log, stderr=log, creationflags=subprocess.CREATE_NO_WINDOW)
state = {"pid": child.pid, "db": str(database), "port": 18888, "cdp": cdp, "config": str(config), "log": str(evidence / "tauri.log"), "evidence": str(evidence)}
(evidence / "state.json").write_text(json.dumps(state, indent=2), encoding="utf-8")
print(json.dumps({**state, "state": str(evidence / "state.json")}, indent=2))
