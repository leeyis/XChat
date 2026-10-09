"""Isolated, repeatable workspace-poll baseline. Standard library only.

Run from the repository root through rtk proxy python. This never opens the
installed application's database or advertises on the LAN.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import socket
import sqlite3
import statistics
import subprocess
import tempfile
import time
import urllib.error
import urllib.request


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--label', required=True)
    parser.add_argument('--peers', type=int, default=300)
    parser.add_argument('--files', type=int, default=100)
    parser.add_argument('--samples', type=int, default=20)
    args = parser.parse_args()
    if args.samples < 1 or args.peers < 1 or not 0 <= args.files <= args.peers:
        parser.error('samples and peers must be positive; files must be between zero and peers')
    binary = args.binary.resolve(strict=True)
    root = Path(tempfile.mkdtemp(prefix='xchat-phase2-benchmark-'))
    downloads = root / 'downloads'
    downloads.mkdir()
    with sqlite3.connect(root / 'xchat.db') as db:
        db.execute('CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)')
        db.executemany('INSERT INTO settings VALUES (?, ?)', [
            ('download_path', str(downloads)),
            ('network.discovery.settings.v1', json.dumps({'local_discovery': False, 'vpn_discovery': False, 'interface_overrides': {}})),
        ])
    with socket.socket() as reservation:
        reservation.bind(('127.0.0.1', 0))
        port = reservation.getsockname()[1]
    base = f'http://127.0.0.1:{port}'

    def request(path):
        with urllib.request.urlopen(base + path, timeout=60) as response:
            body = response.read()
            return json.loads(body), len(body)

    def start(log):
        process = subprocess.Popen([str(binary), '--port', str(port), '--db-path', str(root)],
            stdout=log, stderr=subprocess.STDOUT,
            creationflags=subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0)
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            if process.poll() is not None:
                raise RuntimeError(f'benchmark server exited: {process.returncode}')
            try:
                health, _ = request('/api/health')
                if health['state'] == 'ready':
                    assert health['eligible_discovery_interfaces'] == 0, health
                    return process
            except OSError:
                pass
            time.sleep(.1)
        process.terminate()
        process.wait(timeout=10)
        raise RuntimeError('benchmark server did not become ready')

    def stop(process):
        if process.poll() is None:
            process.terminate()
        process.wait(timeout=10)

    with (root / 'initialize.log').open('w', encoding='utf-8') as log:
        stop(start(log))
    with sqlite3.connect(root / 'xchat.db') as db:
        user_id = db.execute("SELECT value FROM settings WHERE key='user_id'").fetchone()[0]
        for index in range(args.peers):
            peer = f'bench-peer-{index:05}'
            db.execute('INSERT INTO users (id,name,addr,last_seen,is_offline) VALUES (?,?,?,?,1)',
                       (peer, peer, '127.0.0.1:9', 1000))
            # Match the production deterministic direct-conversation identity.
            ids = sorted([user_id, peer])
            conversation = 'direct:' + ':'.join(ids)
            db.execute("INSERT INTO conversations (id,kind,peer_id,created_at,updated_at) VALUES (?,'direct',?,1000,1000)", (conversation, peer))
            db.executemany('INSERT INTO conversation_members VALUES (?,?,?,?,?)',
                           [(conversation, user_id, 'Benchmark', 'member', 1000), (conversation, peer, peer, 'member', 1000)])
            db.execute("INSERT INTO messages (sender_id,receiver_id,content,msg_type,timestamp,status,conversation_id,client_message_id) VALUES (?,?,?,'text',1000,'read',?,?)",
                       (user_id, peer, 'benchmark history', conversation, f'bench-text-{index}'))
            if index < args.files:
                db.execute("INSERT INTO messages (sender_id,receiver_id,content,msg_type,timestamp,status,conversation_id,client_message_id,file_path,file_status,file_size) VALUES (?,?,?,'file',1001,'read',?, ?,?,'completed',1024)",
                           (user_id, peer, f'file-{index}.bin', conversation, f'bench-file-{index}', str(downloads / f'file-{index}.bin')))
    samples = []
    cursor = None
    mode = 'snapshot'
    with (root / 'benchmark.log').open('w', encoding='utf-8') as log:
        process = start(log)
        try:
            try:
                payload, _ = request('/api/workspace/sync')
                cursor = payload['cursor']
                mode = 'incremental'
            except urllib.error.HTTPError as error:
                if error.code != 404:
                    raise
            except (KeyError, json.JSONDecodeError):
                # Older servers may serve the frontend fallback for unknown routes.
                pass
            for index in range(args.samples + 2):
                path = '/api/workspace' if mode == 'snapshot' else f'/api/workspace/sync?cursor={cursor}'
                started = time.perf_counter()
                payload, size = request(path)
                elapsed = (time.perf_counter() - started) * 1000
                if mode == 'incremental':
                    cursor = payload['cursor']
                if index >= 2:
                    samples.append({'elapsed_ms': round(elapsed, 3), 'response_bytes': size})
                time.sleep(.1)
            health, _ = request('/api/health')
            assert health['state'] == 'ready' and health['generation'] == 1, health
        finally:
            stop(process)
    elapsed = sorted(sample['elapsed_ms'] for sample in samples)
    with binary.open('rb') as executable:
        binary_hash = hashlib.file_digest(executable, 'sha256').hexdigest()
    report = {
        'label': args.label, 'platform': platform.platform(), 'cpu_count': os.cpu_count(),
        'binary_sha256': binary_hash,
        'fixture': {'offline_peers': args.peers, 'historical_files': args.files, 'samples': args.samples},
        'mode': mode, 'database_directory': str(root), 'port': port,
        'elapsed_ms': {'mean': round(statistics.mean(elapsed), 3), 'p50': statistics.median(elapsed),
                       'p95': elapsed[math.ceil(len(elapsed) * .95) - 1], 'max': max(elapsed)},
        'response_bytes_mean': round(statistics.mean(sample['response_bytes'] for sample in samples)),
        'samples': samples,
        'limits': 'Local Windows loopback workspace polling; not a LAN or file-throughput claim.',
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({key: value for key, value in report.items() if key != 'samples'}, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()
