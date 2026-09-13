#!/usr/bin/env python3
"""Synthetic managed reasoning + actual recorder admission/publication barriers."""
import argparse
import contextlib
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import threading
import urllib.error
from urllib.request import Request, ProxyHandler, build_opener
import extension_manager as manager
from extension_smoke import terminate, read_ready
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tests/codex'))
import embedded_contract as contract
import interactions_harness as gemini
import reasoning_continuity as claude
import reasoning_chat_continuity as chat


def encode(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':')).encode()


class Provider(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        self.server.calls += 1
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        streaming = body.get('stream', False)
        name = self.server.contract
        if name == 'gemini':
            blocks = [{'type': 'thought', 'signature': 'synthetic-private-signature'}, {'type': 'function_call', 'id': 'call_1', 'name': 'echo', 'arguments': {'text': 'safe'}}]
            value = {'id': 'provider_1', 'model': 'synthetic-model', 'object': 'interaction', 'status': 'requires_action', 'steps': blocks,
                     'usage': {'total_input_tokens': 10, 'total_output_tokens': 3, 'total_thought_tokens': 2, 'total_tokens': 15, 'total_cached_tokens': 0}}
            frames = gemini.frames(blocks, 1)
        elif name.startswith('claude'):
            blocks = [{'type': 'thinking', 'thinking': 'synthetic-public-reasoning', 'signature': 'synthetic-private-signature'},
                      {'type': 'redacted_thinking', 'data': 'synthetic-private-redacted'}, {'type': 'tool_use', 'id': 'call_1', 'name': 'echo', 'input': {'text': 'safe'}}]
            usage = {'input_tokens': 10, 'cache_read_input_tokens': 5, 'cache_creation_input_tokens': 2,
                     'cache_creation': {'ephemeral_5m_input_tokens': 2, 'ephemeral_1h_input_tokens': 0}, 'output_tokens': 12}
            value = {'id': 'synthetic_1', 'model': 'synthetic-model', 'type': 'message', 'role': 'assistant', 'content': blocks, 'stop_reason': 'tool_use', 'usage': usage}
            frames = claude.frames(blocks, 1)
            first = json.loads(frames[0].decode().split('data: ', 1)[1]); first['message']['usage'] = {k: v for k, v in usage.items() if k != 'output_tokens'}
            frames[0] = claude.frame('message_start', message=first['message'])
        else:
            assistant = chat.assistant(1, name == 'open_router')
            assistant['tool_calls'] = [{'id': 'call_1', 'type': 'function', 'function': {'name': 'echo', 'arguments': '{"text":"safe"}'}}]
            usage = {'prompt_tokens': 10, 'completion_tokens': 12, 'total_tokens': 22, 'completion_tokens_details': {'reasoning_tokens': 5}}
            if name == 'deep_seek': usage.update(prompt_cache_hit_tokens=4, prompt_cache_miss_tokens=6)
            else: usage['prompt_tokens_details'] = {'cached_tokens': 4}
            value = {'id': 'synthetic_1', 'model': 'synthetic-model', 'object': 'chat.completion', 'created': 1,
                     'choices': [{'index': 0, 'message': assistant, 'finish_reason': 'tool_calls'}], 'usage': usage}
            frames = chat.frames(assistant, 1)
            last = json.loads(frames[-2].decode().split('data: ', 1)[1]); last['usage'] = usage
            frames[-2] = b'data: ' + encode(last) + b'\n\n'
        if getattr(self.server,'editing',False):
            definitions=[t.get('function',t) for t in body['tools']]
            selected=[t for t in definitions if 'before_context' in t.get('parameters',t.get('input_schema',{})).get('properties',{})]
            assert len(selected)==1
            args={'path':'synthetic.txt','before_context':[],'old_lines':['old'],'new_lines':['new'],'after_context':[]}
            def replace(v):
                if isinstance(v,dict):
                    if v.get('name')=='echo':
                        v['name']=selected[0]['name']
                        if 'input' in v:v['input']=args
                        if 'arguments' in v:v['arguments']=json.dumps(args) if isinstance(v['arguments'],str) else args
                    for child in v.values():replace(child)
                elif isinstance(v,list):
                    for child in v:replace(child)
            replace(value)
            if name=='gemini':frames=gemini.frames(value['steps'],1)
            elif name.startswith('claude'):
                frames=claude.frames(value['content'],1)
                first=json.loads(frames[0].decode().split('data: ',1)[1]);first['message']['usage']={k:v for k,v in usage.items() if k!='output_tokens'}
                frames[0]=claude.frame('message_start',message=first['message'])
            else:
                frames=chat.frames(value['choices'][0]['message'],1)
                last=json.loads(frames[-2].decode().split('data: ',1)[1]);last['usage']=usage
                frames[-2]=b'data: '+encode(last)+b'\n\n'
        if getattr(self.server,'resume_editing',False):
            if name=='gemini':
                calls=[v for v in body['input'] if v.get('type')=='function_call']
                assert any(v['name'].startswith('arg_edit_') and v['arguments']['old_lines']==['old'] for v in calls)
                value['status']='completed';value['steps']=[{'type':'thought','signature':'synthetic-private-signature'},{'type':'model_output','content':[{'type':'text','text':'resumed'}]}]
            elif name.startswith('claude'):
                calls=[v for m in body['messages'] for v in m.get('content',[]) if isinstance(v,dict) and v.get('type')=='tool_use']
                assert any(v['name'].startswith('arg_edit_') and v['input']['old_lines']==['old'] for v in calls)
                value['content']=value['content'][:2]+[{'type':'text','text':'resumed'}];value['stop_reason']='end_turn'
            else:
                calls=[v for m in body['messages'] for v in m.get('tool_calls',[])]
                assert any(v['function']['name'].startswith('arg_edit_') and json.loads(v['function']['arguments'])['old_lines']==['old'] for v in calls)
                value['choices'][0]['message'].pop('tool_calls',None);value['choices'][0]['message']['content']='resumed';value['choices'][0]['finish_reason']='stop'
            assert not streaming
        if streaming and getattr(self.server, 'decreasing_usage', False):
            first = json.loads(frames[0].decode().split('data: ', 1)[1])
            if name == 'gemini': first['interaction']['usage'] = {'total_output_tokens': 20}
            elif name.startswith('claude'): first['message']['usage']['output_tokens'] = 20
            else: first['usage'] = {'completion_tokens': 20}
            frames[0] = frames[0].split(b'data: ', 1)[0] + b'data: ' + encode(first) + b'\n\n'
        raw = b''.join(frames) if streaming else encode(value)
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream' if streaming else 'application/json')
        self.send_header('Content-Length', str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)


def run(binary, recorder, compatibility_policy=False, profile_packs=False, codec_binary=None, editing=False, runtime_dir=None, code_mode=False):
    patch_formats=[]
    declarations=[]
    descriptor=None
    if editing:
        import editing_contract as ec
        lock=json.loads(ec.c.runtime.LOCK.read_text())
        codex=ec.c.runtime.verify_bundle(runtime_dir or ec.c.runtime.BUNDLE,lock)
        ec.run(codex,binary,'code_mode' if code_mode else 'direct',capture=patch_formats,declarations=declarations)
        assert len(declarations)==1
        if code_mode:
            descriptor=json.loads((ROOT/'tests/codex/editing-contract-lock.json').read_text())['code_mode_descriptors']['builtin']
            assert hashlib.sha256(declarations[0]['description'].encode()).hexdigest()==descriptor
        else:assert len(patch_formats)==1
    parent = ROOT / '.local/managed-usage-smoke' ; parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=parent) as temporary, contextlib.ExitStack() as cleanup:
        root = Path(temporary).resolve(); root.chmod(0o700)
        store = manager.open_store(root / 'extensions')
        ledger = manager.private_dir(manager.private_dir(store / 'usage', create=True) / 'primary', create=True)
        raw = encode({'schema': 'gateway-usage-recorder-config/v1', 'destinations': []})
        manager.write_new(ledger / 'recorder.json', raw)
        subprocess.run([str(recorder), 'init', '--store', str(ledger)], check=True, capture_output=True)
        binding = {'store_id': 'primary', 'mode': 'durable_local', 'queue_capacity': 256, 'ack_timeout_ms': 1000, 'config_sha256': hashlib.sha256(raw).hexdigest()}
        package = root / 'package'; sha = manager.package_binary(recorder, ROOT / 'LICENSE', package, 'usage-recorder', '0.1.0', 'usage_recorder')
        manager.install(store, package, sha); manager.enable(store, 'usage-recorder', '0.1.0', sha, manager.RECORDER_PERMISSIONS, binding)
        upstream = ThreadingHTTPServer(('127.0.0.1', 0), Provider); upstream.calls = 0
        threading.Thread(target=upstream.serve_forever, daemon=True).start()
        cleanup.callback(upstream.server_close); cleanup.callback(upstream.shutdown)
        client = build_opener(ProxyHandler({}))
        db_path = ledger / 'usage.sqlite3'
        for name in ('gemini', 'claude_adaptive', 'claude_manual', 'deep_seek', 'open_router'):
            folder = root / name; folder.mkdir(mode=0o700); upstream.contract = name
            route = gemini.route('0.154.0') if name == 'gemini' else claude.route('0.154.0', name == 'claude_manual') if name.startswith('claude') else chat.route('0.154.0', name == 'open_router')
            if compatibility_policy:
                route = 'compatibility_policy="checked"\n' + route + '\n[compatibility_policies.checked]\nversion=1\n'
            config = folder / 'gateway.toml'
            config.write_text(f'listen="127.0.0.1:0"\n[providers.mock]\nbase_url="http://127.0.0.1:{upstream.server_port}/v1"\napi_key_env="SYNTHETIC_KEY"\n[models.writer]\nprovider="mock"\nupstream_model="synthetic-model"\n'+route)
            if editing:
                from editing_fixture import configure
                config.write_text(configure(config.read_text(), code_mode=code_mode, descriptor=descriptor))
            upstream.editing=editing
            env = {**os.environ, 'ARG_LOCAL_TOKEN': 'L'*40, 'SYNTHETIC_KEY': 'synthetic-key'}
            control_token = gemini.setup(folder, binary, config, env)
            args = ['--config', str(config), '--extensions-lock', str(store / 'active.json')]
            if profile_packs:
                from profile_pack_fixture import activate
                packed, lock = activate(binary, folder / 'profiles', config.read_text())
                config.write_text(packed)
                args += ['--profile-packs-lock', str(lock)]
            if codec_binary:
                from codec_fixture import activate as activate_codec
                if name=='gemini':
                    encoded,unused=activate_codec(codec_binary,folder/'codec',config.read_text(),store,editing=editing)
                else:
                    encoded=config.read_text().replace('[models.writer]','[models.writer]\napi_codec="reference-codec"')
                config.write_text(encoded)
            manifest = contract.validate_extended_manifest(json.loads(subprocess.run([str(binary), 'manifest', *args], env=env, capture_output=True, check=True).stdout))
            assert manifest['schema'] == ('gateway-extended-manifest/v7' if editing else 'gateway-extended-manifest/v6' if codec_binary else 'gateway-extended-manifest/v5' if profile_packs else 'gateway-extended-manifest/v4' if compatibility_policy else 'gateway-extended-manifest/v3')
            def start():
                child = subprocess.Popen([str(binary), 'serve', *args], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                try:
                    ready = contract.parse_extended_ready_line(json.dumps(read_ready(child))+'\n', manifest)
                    return child, ready
                except Exception:
                    terminate(child)
                    raise
            process, ready = start()
            try:
                base_manifest = manifest['configuration']['gateway']
                saved_resume=[]
                def post(streaming):
                    session = gemini.create_session(ready['base_url'], control_token, base_manifest)
                    body = {'model': 'writer', 'input': 'synthetic', 'stream': streaming,
                            'tools': [{'type': 'function', 'name': 'echo', 'parameters': {'type': 'object', 'properties': {'text': {'type': 'string'}}, 'required': ['text']}}]}
                    if editing: body['tools'].append(declarations[0])
                    request = Request(ready['base_url']+'/responses' , encode(body), {'Authorization': 'Bearer '+'L'*40, 'Content-Type': 'application/json', 'x-gateway-session': session['id']})
                    try:
                        with client.open(request, timeout=15) as response:
                            try: data = response.read()
                            except Exception as error: data = getattr(error, 'partial', b'')
                            if editing and not streaming and response.status==200:
                                saved_resume[:] = [(session, json.loads(data))]
                            return response.status, data
                    except urllib.error.HTTPError as error: return error.code, error.read()
                for streaming in (False, True):
                    status, data = post(streaming)
                    assert status == 200 and b'arg-continuation-v2.' in data
                    if editing: assert b'custom_tool_call' in data and b'*** Begin Patch' in data
                if editing:
                    old_session, completed=saved_resume[0]
                    original={'type':'message','role':'user','content':[{'type':'input_text','text':'synthetic'}]}
                    body={'model':'writer','stream':False,'input':[original,*completed['output'],{'type':'custom_tool_call_output','call_id':'call_1','output':[{'type':'input_text','text':'synthetic metadata'},{'type':'input_text','text':'synthetic applied'}] if code_mode else 'synthetic applied'}],
                          'tools':[{'type':'function','name':'echo','parameters':{'type':'object','properties':{'text':{'type':'string'}},'required':['text']}},declarations[0]]}
                    terminate(process);process,ready=start();upstream.resume_editing=True
                    try:
                        request=Request(ready['base_url']+'/responses',encode(body),{'Authorization':'Bearer '+'L'*40,'Content-Type':'application/json','x-gateway-session':old_session['id']})
                        with client.open(request,timeout=15) as response:
                            data=response.read();assert response.status==200 and b'arg-continuation-v2.' in data and b'resumed' in data
                    finally:upstream.resume_editing=False
                if profile_packs and name == 'gemini':
                    # An old durable session cannot cross changed package bytes after restart.
                    stale_session = gemini.create_session(ready['base_url'], control_token, base_manifest)
                    old_entry = json.loads(lock.read_text())['packs'][0]
                    source_path = folder / 'profiles/source.json'
                    source = json.loads(source_path.read_text())
                    source['evidence'] = [{'description':'Changed synthetic package bytes', 'source_url':None, 'artifact_sha256':None}]
                    changed_source, changed_pack = folder / 'changed-source.json', folder / 'changed-pack.json'
                    changed_source.write_text(json.dumps(source))
                    def pack_command(*options):
                        return json.loads(subprocess.run([str(binary),'profile-pack',*map(str,options)],capture_output=True,check=True).stdout)
                    changed = pack_command('package','--source',changed_source,'--output',changed_pack)
                    pack_command('install','--package',changed_pack,'--store',lock.parent)
                    def select(sha):
                        pack_command('disable','--store',lock.parent,'--id',old_entry['id'])
                        pack_command('enable','--store',lock.parent,'--id',old_entry['id'],'--version',old_entry['version'],'--sha256',sha)
                    terminate(process)
                    select(changed['package_sha256'])
                    manifest = contract.validate_extended_manifest(json.loads(subprocess.run([str(binary),'manifest',*args],env=env,capture_output=True,check=True).stdout))
                    process, ready = start()
                    calls = upstream.calls
                    request = Request(ready['base_url']+'/responses' ,encode({'model':'writer','input':'synthetic'}),{'Authorization':'Bearer '+'L'*40,'Content-Type':'application/json','x-gateway-session':stale_session['id']})
                    try:
                        client.open(request,timeout=15)
                        raise AssertionError('Changed pack accepted old session')
                    except urllib.error.HTTPError as error:
                        assert error.code == 409 and upstream.calls == calls
                    terminate(process)
                    select(old_entry['package_sha256'])
                    manifest = contract.validate_extended_manifest(json.loads(subprocess.run([str(binary),'manifest',*args],env=env,capture_output=True,check=True).stdout))
                    process, ready = start()
                upstream.decreasing_usage = True
                status, data = post(True)
                assert status == 200 and b'arg-continuation-v2.' not in data and b'response.output_item.done' not in data and b'response.completed' not in data
                upstream.decreasing_usage = False
                # A durable accounting admission failure must precede both inference and continuation begin.
                with sqlite3.connect(db_path) as db:
                    db.execute("CREATE TRIGGER synthetic_fail BEFORE INSERT ON usage_events WHEN NEW.kind='attempt_started' BEGIN SELECT RAISE(ABORT,'synthetic'); END")
                calls = upstream.calls
                assert post(False)[0] == 503 and upstream.calls == calls
                terminate(process)
                with sqlite3.connect(db_path) as db:
                    db.execute('DROP TRIGGER synthetic_fail')
                    db.execute("CREATE TRIGGER synthetic_fail BEFORE INSERT ON usage_events WHEN NEW.kind='attempt_finished' BEGIN SELECT RAISE(ABORT,'synthetic'); END")
                for streaming in (False, True):
                    process, ready = start()
                    status, data = post(streaming)
                    assert status == (200 if streaming else 503), (name, streaming, status, upstream.calls)
                    assert b'arg-continuation-v2.' not in data and b'response.output_item.done' not in data and b'response.completed' not in data
                    terminate(process)
                with sqlite3.connect(db_path) as db: db.execute('DROP TRIGGER synthetic_fail')
            finally:
                terminate(process)
        with sqlite3.connect(db_path) as db:
            rows = [json.loads(r[0]) for r in db.execute("SELECT payload FROM usage_current WHERE kind='attempt_finished'")]
            assert len(rows) == (20 if editing else 15)
            assert sum(r['gateway'] == 'conversion_failed' for r in rows) == 5
            completed = [r for r in rows if r['gateway'] == 'completed']
            assert len(completed) == (15 if editing else 10)
            for row in completed:
                assert row['upstream'] == 'completed' and row['gateway'] == 'completed' and row['finality'] == 'final'
                assert row['provider_response_id'] in {'synthetic_1', 'provider_1'}
                counters = row['usage']['counters']; profile = row['profile']
                assert counters['output_tokens']['value'] == (5 if profile == 'gemini_interactions_v1' else 12)
                assert counters['input_tokens']['value'] == (17 if profile == 'messages_v1' else 10)
                if profile == 'messages_v1': assert len(row['usage']['cache_write_details']) == 2
                if profile == 'deep_seek_v1': assert counters['cache_read_input_tokens']['value'] == 4
                serialized = json.dumps(row)
                assert not any(text in serialized for text in ('synthetic-private', 'SYNTHETIC_PUBLIC', 'arg-continuation-', 'synthetic_signature', 'synthetic_encrypted'))
        assert upstream.calls == (30 if editing else 25)
        return {'schema': 'gateway-managed-usage-smoke/v1', 'contracts': 5, 'successful_requests': 15 if editing else 10, 'invalid_usage_failures': 5, 'admission_failures': 5, 'finalization_failures': 10, 'provider_requests': upstream.calls, 'status': 'passed'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--gateway-bin', type=Path, required=True); parser.add_argument('--recorder-bin', type=Path, required=True)
    parser.add_argument('--compatibility-policy', action='store_true', help='Bind the existing managed rules through a selected policy and v4 manifest')
    parser.add_argument('--profile-packs', action='store_true', help='Import the synthetic profiles and policies from an explicitly pinned data pack')
    parser.add_argument("--codec-bin",type=Path)
    parser.add_argument('--editing',action='store_true')
    parser.add_argument('--runtime-dir',type=Path)
    parser.add_argument('--code-mode',action='store_true')
    args = parser.parse_args(); print(json.dumps(run(args.gateway_bin.resolve(), args.recorder_bin.resolve(), args.compatibility_policy, args.profile_packs, args.codec_bin, args.editing or args.code_mode, args.runtime_dir, args.code_mode), sort_keys=True))


if __name__ == '__main__':
    main()
