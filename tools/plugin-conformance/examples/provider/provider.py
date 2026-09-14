"""Synthetic provider v1, stdlib only. Trusted native process, never HTTP transport."""
import base64
import json
import struct
import sys

PROTOCOL = 'gateway-provider/v1'
VENDOR = 'synthetic-provider/v1'
CAPABILITIES = {'schema': 'gateway-plugin-capabilities/v1', 'apis': [],
                'features': ['json', 'managed_continuation', 'streaming'],
                'requires': ['provider_ipc_v1', 'responses_output_validation']}
LIMIT = 1024 * 1024
COUNTERS = ('input_tokens', 'output_tokens', 'total_tokens', 'input_regular_tokens',
            'cache_read_input_tokens', 'cache_write_input_tokens', 'reasoning_output_tokens')


def encode(value):
    return json.dumps(value, separators=(',', ':'), ensure_ascii=True, allow_nan=False).encode()


def decode(raw):
    def pairs(items):
        value = {}
        for key, item in items:
            if key in value:
                raise ValueError('duplicate')
            value[key] = item
        return value
    return json.loads(raw, object_pairs_hook=pairs,
                      parse_constant=lambda _: (_ for _ in ()).throw(ValueError('number')))


def exact(stream, size):
    raw = bytearray()
    while len(raw) < size:
        part = stream.read(size - len(raw))
        if not part:
            raise EOFError()
        raw.extend(part)
    return bytes(raw)


def read():
    size = struct.unpack('>I', exact(sys.stdin.buffer, 4))[0]
    if not 0 < size <= LIMIT:
        raise ValueError('limit')
    return decode(exact(sys.stdin.buffer, size))


def write(sequence, value):
    raw = encode({'protocol': PROTOCOL, 'sequence': sequence, 'value': value})
    if len(raw) > LIMIT:
        raise ValueError('limit')
    sys.stdout.buffer.write(struct.pack('>I', len(raw)) + raw)
    sys.stdout.buffer.flush()


def usage(native):
    if 'meter' not in native:
        return {'kind': 'unobserved'}
    meter = native['meter']
    if not isinstance(meter, dict):
        raise ValueError('meter')
    counters = {}
    for name in COUNTERS:
        if name not in meter:
            counters[name] = {'source': 'not_reported'}
        elif type(meter[name]) is int and 0 <= meter[name] < 2**64:
            counters[name] = {'source': 'reported', 'value': meter[name]}
        else:
            counters[name] = {'source': 'invalid'}
    return {'kind': 'observed', 'counters': counters}


class Provider:
    def __init__(self):
        self.phase = 'new'
        self.sequence = 0
        self.prepared = None
        self.response_id = None
        self.text = ''
        self.native = None
        self.counter = 0
        self.started_text = False

    def completed(self, native):
        output = []
        for index, item in enumerate(native['answer']):
            if set(item) == {'text'} and isinstance(item['text'], str):
                output.append({'id': f'msg_{index}', 'type': 'message', 'role': 'assistant',
                               'status': 'completed', 'content': [{'type': 'output_text',
                               'text': item['text'], 'annotations': []}]})
            elif set(item) == {'call', 'name', 'arguments'} and all(isinstance(item[k], str) for k in item):
                decode(item['arguments'])
                output.append({'id': f'fc_{index}', 'type': 'function_call', 'status': 'completed',
                               'call_id': item['call'], 'name': item['name'], 'arguments': item['arguments']})
            else:
                raise ValueError('answer')
        state = {'kind': 'none'}
        if self.prepared['continuation']['mode'] == 'managed':
            state = {'kind': 'opaque', 'value': {'format': 'synthetic-counter', 'version': 1,
                     'data_base64': base64.b64encode(encode({'counter': self.counter + 1})).decode()}}
        observation = usage(native)
        response = {'id': self.response_id, 'object': 'response', 'status': 'completed',
                    'model': self.prepared['route']['model'], 'created_at': 0, 'output': output}
        if observation['kind'] == 'observed':
            counters = observation['counters']
            public = {k: counters[k].get('value') for k in ('input_tokens', 'output_tokens', 'total_tokens')}
            if (counters['total_tokens']['source'] == 'not_reported'
                    and public['input_tokens'] is not None and public['output_tokens'] is not None
                    and public['input_tokens'] + public['output_tokens'] < 2**64):
                public['total_tokens'] = public['input_tokens'] + public['output_tokens']
            for detail, names in [('input_tokens_details', [('cached_tokens', 'cache_read_input_tokens'),
                                                            ('cache_write_tokens', 'cache_write_input_tokens')]),
                                  ('output_tokens_details', [('reasoning_tokens', 'reasoning_output_tokens')])]:
                values = {name: counters[source]['value'] for name, source in names if counters[source]['source'] == 'reported'}
                if values:
                    public[detail] = values
            response['usage'] = public
        if len(encode(response)) > self.prepared['max_output_bytes']:
            return {'result': 'rejected', 'code': 'resource_limit'}
        return {'result': 'completed', 'value': {'response': response,
                'outcome': 'awaiting_tools' if any(i['type'] == 'function_call' for i in output) else 'completed',
                'usage': observation, 'state': state}}

    def call(self, message):
        if set(message) != {'protocol', 'sequence', 'operation'} or message['protocol'] != PROTOCOL:
            raise ValueError('envelope')
        if type(message['sequence']) is not int or message['sequence'] != self.sequence + 1 or message['sequence'] >= 2**64:
            raise ValueError('sequence')
        self.sequence = message['sequence']
        op = message['operation']
        kind = op['operation']
        if kind == 'prepare' and self.phase == 'new' and set(op) == {'operation', 'value'}:
            value = op['value']
            if value['route']['provider_protocol'] != VENDOR or value['route']['editing'] != {'kind': 'none'}:
                return {'result': 'rejected', 'code': 'unsupported_request'}
            history = value['continuation'].get('history', [])
            if history:
                state = history[-1]['state']
                if (set(state) != {'format', 'version', 'data_base64'} or state['format'] != 'synthetic-counter'
                        or type(state['version']) is not int or state['version'] != 1):
                    return {'result': 'rejected', 'code': 'unsupported_state'}
                raw = base64.b64decode(state['data_base64'], validate=True)
                if len(raw) > LIMIT or base64.b64encode(raw).decode() != state['data_base64']:
                    return {'result': 'rejected', 'code': 'unsupported_state'}
                restored = decode(raw)
                if set(restored) != {'counter'} or type(restored['counter']) is not int or not 0 <= restored['counter'] < 2**64 - 1:
                    return {'result': 'rejected', 'code': 'unsupported_state'}
                self.counter = restored['counter']
            self.prepared = value
            payload = {'query': value['request'], 'cursor': self.counter}
            if len(encode(payload)) > value['max_request_bytes']:
                return {'result': 'rejected', 'code': 'resource_limit'}
            self.phase = 'prepared'
            return {'result': 'prepared', 'payload': payload}
        if kind == 'json' and self.phase == 'prepared' and set(op) == {'operation', 'body', 'response_id'}:
            self.response_id = op['response_id']
            self.phase = 'done'
            return self.completed(decode(op['body']))
        if kind == 'stream' and self.phase == 'prepared' and set(op) == {'operation', 'response_id'}:
            self.response_id = op['response_id']
            self.phase = 'streaming'
            return {'result': 'progress', 'events': [], 'complete': False, 'usage': {'kind': 'unobserved'}}
        if kind == 'event' and self.phase == 'streaming' and set(op) == {'operation', 'event', 'data'}:
            native = decode(op['data'])
            events = []
            if op['event'] == 'piece' and set(native) == {'text'} and isinstance(native['text'], str):
                if not self.started_text:
                    events = [{'type': 'response.created', 'response': {'id': self.response_id, 'object': 'response',
                              'created_at': 0, 'model': self.prepared['route']['model'], 'status': 'in_progress', 'output': []}},
                              {'type': 'response.output_item.added', 'output_index': 0, 'item': {
                                  'id': 'msg_0', 'type': 'message', 'role': 'assistant', 'status': 'in_progress', 'content': []}},
                              {'type': 'response.content_part.added', 'output_index': 0, 'item_id': 'msg_0',
                               'content_index': 0, 'part': {'type': 'output_text', 'text': '', 'annotations': []}}]
                    self.started_text = True
                events.append({'type': 'response.output_text.delta', 'output_index': 0, 'item_id': 'msg_0',
                               'content_index': 0, 'delta': native['text']})
                self.text += native['text']
                if len(self.text.encode()) > self.prepared['max_output_bytes']:
                    return {'result': 'rejected', 'code': 'resource_limit'}
                return {'result': 'progress', 'events': events, 'complete': False, 'usage': {'kind': 'unobserved'}}
            if op['event'] == 'end':
                if self.started_text and native['answer'][0] != {'text': self.text}:
                    raise ValueError('text')
                self.native = native
                self.phase = 'complete'
                return {'result': 'progress', 'events': [], 'complete': True, 'usage': usage(native)}
        if kind == 'finish' and self.phase == 'complete' and set(op) == {'operation'}:
            self.phase = 'done'
            return self.completed(self.native)
        raise ValueError('state')


def main():
    provider = Provider()
    write(0, {'result': 'ready', 'provider_protocol': VENDOR, 'capabilities': CAPABILITIES})
    while provider.phase != 'done':
        try:
            result = provider.call(read())
            write(provider.sequence, result)
            if result['result'] == 'rejected':
                return 1
        except EOFError:
            return 1
        except (ValueError, TypeError, KeyError, IndexError, RecursionError):
            write(provider.sequence, {'result': 'rejected', 'code': 'invalid_upstream'})
            return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
