#!/usr/bin/env python3
"""Create and stop a disposable loopback TLS database for the explicit recorder test."""
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def run():
    tools = {name: shutil.which(name) for name in ('initdb', 'pg_ctl', 'openssl')}
    if not all(tools.values()):
        raise RuntimeError('Disposable PostgreSQL test tools are required')
    parent = ROOT / '.local/usage-postgres'
    parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=parent) as temporary:
        root = Path(temporary).resolve()
        root.chmod(0o700)
        def command(args):
            subprocess.run(args, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        data = root / 'data'
        command([tools['initdb'], '-D', str(data), '-A', 'trust', '-U', 'usage_test', '--no-locale'])
        key, cert = root / 'server.key', root / 'server.crt'
        command([tools['openssl'], 'req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-keyout', str(key), '-out', str(cert), '-days', '1', '-subj', '/CN=localhost', '-addext', 'subjectAltName=DNS:localhost,IP:127.0.0.1'])
        key.chmod(0o600)
        cert.chmod(0o600)
        with socket.socket() as reservation:
            reservation.bind(('127.0.0.1', 0))
            port = reservation.getsockname()[1]
        with (data / 'postgresql.conf').open('a') as f:
            f.write(f"\nlisten_addresses='127.0.0.1'\nport={port}\nunix_socket_directories=''\nssl=on\nssl_cert_file='{cert}'\nssl_key_file='{key}'\n")
        connection = root / 'connection'
        connection.write_text(f'host=127.0.0.1 port={port} user=usage_test dbname=postgres sslmode=require')
        connection.chmod(0o600)
        try:
            try:
                # Only TCP is used; disable Unix sockets to avoid filesystem path-length limits.
                command([tools['pg_ctl'], '-D', str(data), '-l', str(root / 'postgres.log'), '-w', 'start'])
            except subprocess.CalledProcessError:
                # This is a disposable synthetic database, never a user/production log.
                print((root / 'postgres.log').read_text()[-8192:])
                raise
            subprocess.run(['cargo', 'test', '-p', 'gateway-usage-recorder', '--locked', '--test', 'postgres', '--', '--ignored'], cwd=ROOT, env={**os.environ, 'USAGE_TEST_PG_CONNECTION_FILE': str(connection), 'USAGE_TEST_PG_CA_FILE': str(cert)}, check=True)
        finally:
            if (data / 'postmaster.pid').exists():
                command([tools['pg_ctl'], '-D', str(data), '-m', 'immediate', '-w', 'stop'])
    print('usage-postgres: PASS; disposable TLS database stopped')


if __name__ == '__main__':
    run()
