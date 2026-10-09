"""Launch isolated Tauri dev and headless Edge; leave the installed app untouched."""
import argparse
import json
import os
from pathlib import Path
import socket
import sqlite3
import subprocess
import tempfile
import time
import urllib.request


def free_port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        return sock.getsockname()[1]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--output-directory', required=True, type=Path)
    args = parser.parse_args()
    root = Path(tempfile.mkdtemp(prefix='xchat-phase2-frontend-'))
    args.output_directory.mkdir(parents=True, exist_ok=True)
    processes = []
    logs = []
    env = os.environ.copy()
    native_port, web_port, vite_port, native_debug, web_debug = [free_port() for _ in range(5)]
    directories = {mode: root / mode for mode in ['tauri', 'web']}
    for directory in directories.values():
        directory.mkdir()
        (directory / 'downloads').mkdir()
        with sqlite3.connect(directory / 'xchat.db') as db:
            db.execute('CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)')
            db.executemany('INSERT INTO settings VALUES (?,?)', [
                ('download_path', str(directory / 'downloads')),
                ('network.discovery.settings.v1', json.dumps({'local_discovery': False, 'vpn_discovery': False, 'interface_overrides': {}})),
            ])
    def launch(name, command, child_env=None):
        log = (root / f'{name}.log').open('w', encoding='utf-8')
        logs.append(log)
        process = subprocess.Popen(command, stdout=log, stderr=subprocess.STDOUT,
                                   env=child_env or env, creationflags=subprocess.CREATE_NO_WINDOW)
        processes.append(process)
        return process
    def ready(port, process, timeout):
        deadline = time.monotonic() + timeout
        next_update = 0
        while time.monotonic() < deadline:
            if process.poll() is not None:
                raise RuntimeError(f'child {process.pid} exited {process.returncode}; logs: {root}')
            try:
                with urllib.request.urlopen(f'http://127.0.0.1:{port}/api/health', timeout=2) as response:
                    health = json.load(response)
                    if health['state'] == 'ready':
                        assert health['eligible_discovery_interfaces'] == 0, health
                        return
            except OSError:
                pass
            if time.monotonic() >= next_update:
                print(f'Waiting for isolated process {process.pid}; logs: {root}', flush=True)
                next_update = time.monotonic() + 20
            time.sleep(.5)
        raise RuntimeError(f'child startup timeout; logs: {root}')
    try:
        web = launch('web', [str(args.binary.resolve()), '--port', str(web_port), '--db-path', str(directories['web'])])
        ready(web_port, web, 30)
        edge = Path('C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe')
        launch('edge', [str(edge), '--headless=new', '--no-first-run', '--disable-gpu',
                       f'--user-data-dir={root / "edge-profile"}', f'--remote-debugging-port={web_debug}', f'http://127.0.0.1:{web_port}'])
        subprocess.run(['rtk', 'proxy', 'node', 'scripts/smoke_workspace_sync.mjs', str(web_debug), str(web_port),
                        str(directories['web']), 'web', str(args.output_directory / '2026-10-09-phase2-web-ui.json')], check=True, timeout=90)
        config = root / 'tauri-smoke.json'
        config.write_text(json.dumps({'identifier': 'com.xchat.phase2-smoke',
            'build': {'beforeDevCommand': '', 'devUrl': f'http://127.0.0.1:{vite_port}'},
            'app': {'windows': [{'label': 'main', 'title': 'XChat Phase2 Smoke', 'width': 1280, 'height': 800, 'visible': False}]}}), encoding='utf-8')
        launch('vite', ['rtk', 'npm', 'run', 'dev', '--', '--host', '127.0.0.1', '--port', str(vite_port), '--strictPort'])
        native_env = env.copy()
        native_env['WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS'] = f'--remote-debugging-port={native_debug}'
        native_env['WEBVIEW2_USER_DATA_FOLDER'] = str(root / 'native-profile')
        native = launch('tauri', ['rtk', 'cargo', 'tauri', 'dev', '--no-watch', '--config', str(config),
                       '--', '--', '--port', str(native_port), '--db-path', str(directories['tauri'])], native_env)
        ready(native_port, native, 600)
        subprocess.run(['rtk', 'proxy', 'node', 'scripts/smoke_workspace_sync.mjs', str(native_debug), str(native_port),
                        str(directories['tauri']), 'tauri', str(args.output_directory / '2026-10-09-phase2-tauri-ui.json')], check=True, timeout=90)
        print(f'Both frontend transports passed; isolated logs: {root}', flush=True)
    finally:
        for process in reversed(processes):
            if process.poll() is None:
                # Only descendants of processes created by this test are stopped.
                subprocess.run(['rtk', 'proxy', 'taskkill', '/PID', str(process.pid), '/T', '/F'], capture_output=True)
        for log in logs:
            log.close()


if __name__ == '__main__':
    main()
