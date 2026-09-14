"""Independent numeric-event Recorder example. Stdlib only; no export or host imports."""
import hashlib
import json
import os
from pathlib import Path
import re
import sqlite3
import sys
import uuid

PROTOCOL = 'gateway-usage-recorder/v2'
CAPABILITIES = {'schema': 'gateway-plugin-capabilities/v1', 'apis': [],
                'features': ['usage_event_v1', 'usage_event_v2'], 'requires': ['usage_recorder_ipc_v2']}
MAX_EVENT = 65536
SOURCE = Path(__file__).resolve().parent


def encode(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(',', ':'), allow_nan=False).encode()


def decode(raw):
    def pairs(items):
        value = {}
        for key, item in items:
            if key in value:
                raise ValueError('duplicate_field')
            value[key] = item
        return value
    return json.loads(raw, object_pairs_hook=pairs,
                      parse_constant=lambda _: (_ for _ in ()).throw(ValueError('invalid_number')))


def matches(schema, value):
    """Closed structural subset used by bundled, reference-free event schemas."""
    if 'const' in schema and value != schema['const']:
        return False
    if 'enum' in schema and value not in schema['enum']:
        return False
    if 'type' in schema:
        types = schema['type'] if isinstance(schema['type'], list) else [schema['type']]
        actual = ('null' if value is None else 'boolean' if type(value) is bool else 'integer' if type(value) is int
                  else 'number' if type(value) is float else 'string' if isinstance(value, str)
                  else 'array' if isinstance(value, list) else 'object' if isinstance(value, dict) else 'invalid')
        if actual not in types and not (actual == 'integer' and 'number' in types):
            return False
    for key, test in [('allOf', all), ('anyOf', any)]:
        if key in schema and not test(matches(item, value) for item in schema[key]):
            return False
    if 'oneOf' in schema and sum(matches(item, value) for item in schema['oneOf']) != 1:
        return False
    if 'if' in schema:
        branch = 'then' if matches(schema['if'], value) else 'else'
        if not matches(schema.get(branch, {}), value):
            return False
    if isinstance(value, dict):
        if not set(schema.get('required', [])) <= value.keys():
            return False
        properties = schema.get('properties', {})
        extra = schema.get('additionalProperties', {})
        for name, item in value.items():
            if name in properties:
                if not matches(properties[name], item):
                    return False
            elif extra is False or (isinstance(extra, dict) and not matches(extra, item)):
                return False
        if len(value) > schema.get('maxProperties', len(value)):
            return False
    if isinstance(value, list):
        if not schema.get('minItems', 0) <= len(value) <= schema.get('maxItems', len(value)):
            return False
        if any(not matches(schema.get('items', {}), item) for item in value):
            return False
    if isinstance(value, str):
        if 'pattern' in schema and not re.search(schema['pattern'], value):
            return False
        if not schema.get('minLength', 0) <= len(value) <= schema.get('maxLength', len(value)):
            return False
    if type(value) in (int, float):
        if not schema.get('minimum', value) <= value <= schema.get('maximum', value):
            return False
    return True


def validate(raw):
    if len(raw) > MAX_EVENT:
        raise ValueError('event_limit')
    value = decode(raw)
    if not isinstance(value, dict):
        raise ValueError('invalid_event')
    version = {'gateway-usage-event/v1': 'v1', 'gateway-usage-event/v2': 'v2'}.get(value.get('schema'))
    if version is None or not matches(decode((SOURCE / f'event-{version}.schema.json').read_bytes()), value):
        raise ValueError('invalid_event')
    if encode(value) != raw or value['observed_at_ms'] < value['started_at_ms']:
        raise ValueError('invalid_event_bytes')
    if (value['kind'] == 'attempt_started') != (value['revision'] == 0):
        raise ValueError('invalid_revision')
    for counter in value['usage']['counters'].values():
        known = counter['source'] in ('reported', 'derived')
        if known != (type(counter['value']) is int):
            raise ValueError('invalid_counter')
    return value


def database(create=False):
    root = Path.cwd()
    if root.is_symlink() or root.stat().st_mode & 0o077:
        raise ValueError('private_state_required')
    path = root / 'events.sqlite3'
    if create:
        descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        os.close(descriptor)
    if path.is_symlink() or path.stat().st_nlink != 1 or path.stat().st_mode & 0o077:
        raise ValueError('private_state_required')
    db = sqlite3.connect(path, isolation_level=None)
    db.execute('PRAGMA busy_timeout=3000')
    db.execute('PRAGMA journal_mode=WAL')
    db.execute('PRAGMA synchronous=FULL')
    if create:
        db.executescript('BEGIN IMMEDIATE; CREATE TABLE metadata(producer TEXT NOT NULL); CREATE TABLE events(event_id TEXT PRIMARY KEY,attempt_id TEXT,revision INTEGER,identity TEXT,kind TEXT,sha256 TEXT,payload BLOB,UNIQUE(attempt_id,revision)); PRAGMA user_version=2; COMMIT;')
        db.execute('INSERT INTO metadata VALUES(?)', (str(uuid.uuid4()),))
    if db.execute('PRAGMA user_version').fetchone()[0] != 2:
        raise ValueError('unsupported_store')
    return db


def record(db, raw):
    value = validate(raw)
    producer = db.execute('SELECT producer FROM metadata').fetchone()[0]
    if value['producer_id'] != producer:
        raise ValueError('producer_mismatch')
    identity = encode([value[k] for k in ('schema', 'request_id', 'started_at_ms', 'provider', 'model_alias', 'upstream_model', 'configuration_sha256')]
                      + [value.get('interpretation', value.get('profile'))]).decode()
    checksum = hashlib.sha256(raw).hexdigest()
    db.execute('BEGIN IMMEDIATE')
    try:
        prior = db.execute('SELECT sha256 FROM events WHERE event_id=? OR (attempt_id=? AND revision=?)',
                           (value['event_id'], value['attempt_id'], value['revision'])).fetchone()
        if prior:
            if prior[0] != checksum:
                raise ValueError('conflict')
        else:
            rows = db.execute('SELECT revision,identity,kind FROM events WHERE attempt_id=?', (value['attempt_id'],)).fetchall()
            if any(identity != old_identity or (kind == 'attempt_finished' and (value['revision'] >= rev or value['kind'] == 'attempt_finished'))
                   or (value['kind'] == 'attempt_finished' and value['revision'] < rev) for rev, old_identity, kind in rows):
                raise ValueError('conflict')
            db.execute('INSERT INTO events VALUES(?,?,?,?,?,?,?)', (value['event_id'], value['attempt_id'], value['revision'], identity, value['kind'], checksum, raw))
        db.commit()
    except Exception:
        db.rollback()
        raise
    return {'type': 'committed', 'event_id': value['event_id'], 'sha256': checksum}


def main():
    if sys.argv[1:] == ['init']:
        with database(create=True) as db:
            print(db.execute('SELECT producer FROM metadata').fetchone()[0])
        return 0
    if sys.argv[1:] != ['serve-v2']:
        return 1
    with database() as db:
        producer = db.execute('SELECT producer FROM metadata').fetchone()[0]
        print(encode({'type': 'ready', 'protocol': PROTOCOL, 'producer_id': producer, 'capabilities': CAPABILITIES}).decode(), flush=True)
        while True:
            raw = sys.stdin.buffer.readline(MAX_EVENT + 2)
            if not raw:
                return 0
            if not raw.endswith(b'\n'):
                return 1
            print(encode(record(db, raw[:-1])).decode(), flush=True)


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (ValueError, OSError, sqlite3.Error, TypeError, KeyError, RecursionError):
        raise SystemExit(1)
