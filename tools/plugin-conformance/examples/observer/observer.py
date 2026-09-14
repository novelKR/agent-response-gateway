"""Independent numeric observer; no gateway imports or third-party packages."""
import json
import sys


def unique_fields(items):
    value = {}
    for key, item in items:
        if key in value:
            raise ValueError('Duplicate field')
        value[key] = item
    return value


def main():
    print('{"type":"ready","protocol":"gateway-observer/v1"}', flush=True)
    sequence = 0
    while True:
        raw = sys.stdin.buffer.readline(4097)
        if not raw:
            return 0
        if len(raw) > 4096 or not raw.endswith(b'\n'):
            return 1
        event = json.loads(raw, object_pairs_hook=unique_fields)
        if (not isinstance(event, dict) or set(event) != {'type', 'sequence', 'status', 'headers_ms'}
                or event['type'] != 'http' or type(event['sequence']) is not int
                or event['sequence'] != sequence + 1 or not 0 < event['sequence'] < 2**64
                or type(event['status']) is not int
                or not 100 <= event['status'] <= 599 or type(event['headers_ms']) is not int
                or not 0 <= event['headers_ms'] < 2**64):
            return 1
        sequence = event['sequence']
        print(json.dumps({'type': 'ack', 'sequence': sequence}, separators=(',', ':')), flush=True)


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (ValueError, OSError):
        raise SystemExit(1)
