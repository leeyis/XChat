"""Isolated real-process v4 receive load + text/ACK latency; standard library only.

No broadcast, installed database, or installed application is used. Large payloads
are generated as a bounded stream and removed after verification. The sender's
Rust retry path has separate fault-injection tests; this harness measures receive.
"""
import argparse
import concurrent.futures
import ctypes
import hashlib
import http.client
import json
import math
import os
from pathlib import Path
import shutil
import socket
import sqlite3
import statistics
import subprocess
import tempfile
import threading
import time
import urllib.parse
import urllib.request
import uuid

BLOCK = b'Z' * (256 * 1024)


def process_metrics(process):
    if os.name != 'nt':
        return {}
    from ctypes import wintypes
    class Memory(ctypes.Structure):
        _fields_ = [('cb', wintypes.DWORD), ('faults', wintypes.DWORD)] + [
            (name, ctypes.c_size_t) for name in ['peak_working', 'working', 'peak_paged', 'paged',
                                               'peak_nonpaged', 'nonpaged', 'pagefile', 'peak_pagefile', 'private']]
    class IO(ctypes.Structure):
        _fields_ = [(name, ctypes.c_ulonglong) for name in ['reads', 'writes', 'other', 'read_bytes', 'write_bytes', 'other_bytes']]
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    psapi = ctypes.WinDLL('psapi', use_last_error=True)
    handle = wintypes.HANDLE(int(process._handle))
    memory, io = Memory(), IO()
    memory.cb = ctypes.sizeof(memory)
    creation, exit_time, system, user = [wintypes.FILETIME() for _ in range(4)]
    if not psapi.GetProcessMemoryInfo(handle, ctypes.byref(memory), memory.cb):
        raise ctypes.WinError(ctypes.get_last_error())
    if not kernel.GetProcessTimes(handle, ctypes.byref(creation), ctypes.byref(exit_time), ctypes.byref(system), ctypes.byref(user)):
        raise ctypes.WinError(ctypes.get_last_error())
    if not kernel.GetProcessIoCounters(handle, ctypes.byref(io)):
        raise ctypes.WinError(ctypes.get_last_error())
    ticks = sum((value.dwHighDateTime << 32) + value.dwLowDateTime for value in [system, user])
    return {'cpu_seconds': round(ticks / 10**7, 3),
            'peak_working_set_mib': round(memory.peak_working / 1024**2, 2),
            'private_mib': round(memory.private / 1024**2, 2),
            'process_io_read_bytes': io.read_bytes, 'process_io_write_bytes': io.write_bytes}


def request(port, path, payload=None):
    data = None if payload is None else json.dumps(payload).encode()
    req = urllib.request.Request(f'http://127.0.0.1:{port}' + path, data=data,
                                 headers={'Content-Type': 'application/json'})
    with urllib.request.urlopen(req, timeout=300) as response:
        return json.load(response)


def stats(samples):
    values = sorted(samples)
    return {'count': len(values), 'mean_ms': round(statistics.mean(values), 3),
            'p95_ms': round(values[math.ceil(len(values) * .95) - 1], 3),
            'max_ms': round(max(values), 3)} if values else {'count': 0}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--large', action='store_true', help='include 1 GiB and 4 GiB + 1 byte')
    parser.add_argument('--profile', choices=['debug', 'release'], default='debug')
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    root = Path(tempfile.mkdtemp(prefix='xchat-phase2-load-'))
    if args.large and shutil.disk_usage(root).free < 10 * 1024**3:
        raise RuntimeError('large matrix requires at least 10 GiB of temporary free space')
    nodes = []
    for name in ['sender', 'receiver']:
        directory = root / name
        directory.mkdir()
        downloads = directory / 'downloads'
        downloads.mkdir()
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            port = sock.getsockname()[1]
        with sqlite3.connect(directory / 'xchat.db') as db:
            db.execute('CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)')
            db.executemany('INSERT INTO settings VALUES (?,?)', [
                ('download_path', str(downloads)), ('auto_download', 'true'),
                ('network.discovery.settings.v1', json.dumps({'local_discovery': False, 'vpn_discovery': False, 'interface_overrides': {}})),
            ])
        nodes.append({'directory': directory, 'port': port})

    def start(node):
        log = (node['directory'] / 'server.log').open('a', encoding='utf-8')
        node['log'] = log
        process = subprocess.Popen([str(binary), '--port', str(node['port']), '--db-path', str(node['directory'])],
            stdout=log, stderr=subprocess.STDOUT, creationflags=subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0)
        node['process'] = process
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            if process.poll() is not None:
                raise RuntimeError(f'server exited: {process.returncode}; log {node["directory"]}')
            try:
                health = request(node['port'], '/api/health')
                if health['state'] == 'ready':
                    assert health['eligible_discovery_interfaces'] == 0, health
                    return
            except OSError:
                pass
            time.sleep(.1)
        raise RuntimeError('server startup timeout')

    def stop(node):
        process = node.get('process')
        if process is not None:
            if process.poll() is None:
                process.terminate()
            process.wait(timeout=10)
            node['log'].close()

    messages = []
    failures = []
    stopped = threading.Event()
    message_thread = None
    report = {'root': str(root), 'profile': args.profile, 'cases': [], 'limits': 'Windows loopback; real receiver v4 HTTP/disk/hash/finalize and real Rust text/ACK; synthetic bounded file sender, not Wi-Fi or native-send throughput. OS process I/O includes network and is not physical-disk-only.'}
    try:
        for node in nodes:
            start(node)
            stop(node)
            with sqlite3.connect(node['directory'] / 'xchat.db') as db:
                node['id'] = db.execute("SELECT value FROM settings WHERE key='user_id'").fetchone()[0]
        for index, node in enumerate(nodes):
            peer = nodes[1 - index]
            with sqlite3.connect(node['directory'] / 'xchat.db') as db:
                db.execute('INSERT INTO users(id,name,addr,last_seen,is_offline) VALUES (?,?,?,?,0)',
                           (peer['id'], 'Phase2 peer', f'127.0.0.1:{peer["port"]}', int(time.time())))
            start(node)
            request(node['port'], '/api/workspace')
        sender, receiver = nodes
        # With discovery disabled the connection policy requires an explicitly
        # verified fixed address; a users-table candidate alone is insufficient.
        for index, node in enumerate(nodes):
            peer = nodes[1 - index]
            request(node['port'], '/api/add_custom_peer', {'peer': f'127.0.0.1:{peer["port"]}', 'expected_device_id': peer['id']})
            verified = request(node['port'], '/api/peers/' + peer['id'] + '/refresh', {})
            assert verified['status'] in ('ready', 'updated'), verified
        conversation = 'direct:' + ':'.join(sorted([sender['id'], receiver['id']]))
        conversation_path = urllib.parse.quote(conversation, safe='')

        def send_messages():
            with sqlite3.connect(sender['directory'] / 'xchat.db') as db:
                while not stopped.is_set():
                    client_id = 'phase2-text-' + uuid.uuid4().hex
                    began = time.perf_counter()
                    try:
                        request(sender['port'], f'/api/conversations/{conversation_path}/messages',
                                {'client_message_id': client_id, 'content': 'text while files are active', 'msg_type': 'text', 'mention_ids': []})
                        deadline = time.monotonic() + 5
                        while time.monotonic() < deadline:
                            ack = db.execute('SELECT delivered_at FROM message_receipts WHERE message_client_id=? AND reader_id=?', (client_id, receiver['id'])).fetchone()
                            if ack and ack[0] is not None:
                                messages.append((time.perf_counter() - began) * 1000)
                                break
                            time.sleep(.01)
                        else:
                            raise RuntimeError('delivery ACK timed out')
                    except Exception as error:
                        failures.append(str(error))
                    stopped.wait(.1)

        message_thread = threading.Thread(target=send_messages, daemon=True)
        message_thread.start()
        cases = [(0, 4), (1024, 4), (64 * 1024**2, 4), (64 * 1024**2, 8), (64 * 1024**2, 16)]
        if args.large:
            cases += [(1024**3, 8), (4 * 1024**3 + 1, 16)]
        for size, channels in cases:
            client_id = 'phase2-file-' + uuid.uuid4().hex
            transfer = client_id + ':' + receiver['id']
            encoded = urllib.parse.quote(transfer, safe='')
            count = 1 if size <= 4 * 1024**2 else min(4096, size, max(math.ceil(size / (4 * 1024**2)), channels * 4))
            chunks = []
            offset = 0
            for index in range(count):
                length = size // count + (index < size % count)
                chunks.append({'index': index, 'offset': offset, 'length': length})
                offset += length
            payload = {'sender_id': sender['id'], 'conversation_id': conversation,
                       'client_message_id': client_id, 'transfer_id': transfer, 'sender_msg_id': client_id,
                       'file_name': 'load.bin', 'file_size': size, 'file_sha256': '', 'chunks': chunks}
            digest = hashlib.sha256()
            for _ in range(size // len(BLOCK)):
                digest.update(BLOCK)
            digest.update(BLOCK[:size % len(BLOCK)])
            expected = digest.hexdigest()
            metrics_before = process_metrics(receiver['process'])
            begin = time.perf_counter()
            prepared = request(receiver['port'], '/api/uploads/v4/prepare', payload)
            assert prepared['status'] == 'ready', prepared
            assert prepared.get('max_parallel_channels') == 4, prepared

            def send_chunk(chunk):
                connection = http.client.HTTPConnection('127.0.0.1', receiver['port'], timeout=300)
                try:
                    connection.putrequest('POST', f'/api/uploads/v4/{encoded}/{chunk["index"]}')
                    connection.putheader('Content-Length', str(chunk['length']))
                    connection.endheaders()
                    remaining = chunk['length']
                    while remaining:
                        block = BLOCK[:min(remaining, len(BLOCK))]
                        connection.send(block)
                        remaining -= len(block)
                    response = connection.getresponse()
                    body = json.loads(response.read())
                    assert response.status == 200, (response.status, body)
                    return body
                finally:
                    connection.close()

            with concurrent.futures.ThreadPoolExecutor(max_workers=channels) as executor:
                list(executor.map(send_chunk, chunks))
            uploaded_at = time.perf_counter()
            if size == 1024:
                duplicate = send_chunk(chunks[0])
                assert duplicate['received'] == size, duplicate
            payload['file_sha256'] = expected
            complete = request(receiver['port'], '/api/uploads/v4/complete', payload)
            assert complete['status'] in ('completed', 'already_exists'), complete
            finished = time.perf_counter()
            repeated = request(receiver['port'], '/api/uploads/v4/complete', payload)
            assert repeated['status'] in ('completed', 'already_exists'), repeated
            with sqlite3.connect(receiver['directory'] / 'xchat.db') as db:
                saved = db.execute('SELECT file_path,file_status FROM messages WHERE client_message_id=?', (client_id,)).fetchone()
                state = db.execute('SELECT status,bytes_transferred FROM transfers WHERE id=?', (transfer,)).fetchone()
            saved_path = saved[0]
            if os.name == 'nt' and saved_path.startswith('\\\\?\\'):
                saved_path = saved_path[4:]
            final = Path(saved_path).resolve(strict=True)
            assert final.is_relative_to(root.resolve()), final
            assert saved[1] == 'accepted' and state == ('completed', size), (saved, state)
            assert final.stat().st_size == size
            with final.open('rb') as stream:
                assert hashlib.file_digest(stream, 'sha256').hexdigest() == expected
            elapsed = finished - begin
            case = {'bytes': size, 'client_channels': channels, 'parts': count,
                    'upload_s': round(uploaded_at - begin, 3), 'finalize_s': round(finished - uploaded_at, 3),
                    'total_s': round(elapsed, 3), 'mib_per_second': round(size / 1024**2 / elapsed, 2),
                    'sha256': expected, 'verified': True, 'repeated_completion': True}
            metrics_after = process_metrics(receiver['process'])
            if metrics_after:
                case['receiver_cpu_seconds'] = round(metrics_after['cpu_seconds'] - metrics_before['cpu_seconds'], 3)
                case['receiver_peak_working_set_mib'] = metrics_after['peak_working_set_mib']
                case['receiver_process_io_write_bytes'] = metrics_after['process_io_write_bytes'] - metrics_before['process_io_write_bytes']
            report['cases'].append(case)
            print(json.dumps(case), flush=True)
            # Only unlink the exact verified payload beneath this run's temp root.
            final.unlink()
        for node in nodes:
            health = request(node['port'], '/api/health')
            assert health['state'] == 'ready' and health['generation'] == 1, health
        report['health'] = 'both ready, generation=1, discovery interfaces=0'
        report['process_metrics'] = {node['directory'].name: process_metrics(node['process']) for node in nodes}
    except Exception as error:
        report['error'] = str(error)
        raise
    finally:
        stopped.set()
        if message_thread:
            message_thread.join(timeout=10)
        for node in nodes:
            stop(node)
        report['text_ack_latency'] = stats(messages)
        report['text_failures'] = failures
        report['text_latency_samples_ms'] = [round(value, 3) for value in messages]
        with binary.open('rb') as stream:
            report['binary_sha256'] = hashlib.file_digest(stream, 'sha256').hexdigest()
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2, ensure_ascii=False) + '\n', encoding='utf-8')
        print(json.dumps({key: value for key, value in report.items() if key not in ('cases', 'text_latency_samples_ms')}, ensure_ascii=False), flush=True)
    assert messages and not failures, failures


if __name__ == '__main__':
    main()
