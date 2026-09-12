#!/usr/bin/env python3
"""Real pinned Codex reasoning notifications, native replay and host-managed recovery.

Synthetic loopback providers only. Never print provider bodies or replay envelopes.
"""
import argparse
import json
from http.server import BaseHTTPRequestHandler
from pathlib import Path
from unittest.mock import patch
import conformance as base
import continuity
import interactions_continuity as durable
import interactions_harness as host
import interactions_http as http_tests


def route(version, manual=False):
    contract = ('kind="claude_manual"\nversion=1\nbudget_tokens=2048\neffort_budgets={low=1024,medium=2048,high=4096}\ninterleaved_beta=true\n' if manual else
                'kind="claude_adaptive"\nversion=1\nefforts=["low","medium","high"]\ndefault_effort="medium"\n')
    return f'''api="messages"
auth="api_key"
messages_version="2023-06-01"
continuation_mode="managed"
capability_profile="synthetic-reasoning"
[capability_profiles.synthetic-reasoning]
version="1"
provider="mock"
upstream_model="synthetic-model"
api="messages"
context_window=32768
max_output_tokens=8192
tested_codex_version="{version}"
[capability_profiles.synthetic-reasoning.reasoning_contract]
{contract}
[capability_profiles.synthetic-reasoning.support]
instructions="native"
instruction_hierarchy="bridged_instruction_envelope"
function_tools="native"
custom_tools="bridged_custom_tool_json"
custom_grammar="bridged_codex_patch_grammar"
namespaced_tools="bridged_tool_namespace"
tool_choice="native"
parallel_tool_control="native"
max_output_tokens="native"
reasoning_effort="native"
reasoning_summary="native"
reasoning_items="native"
structured_output="native"
strict_structured_output="native"
'''


def frame(kind, **fields):
    return ('event: '+kind+'\ndata: '+json.dumps({'type':kind,**fields})+'\n\n').encode()


def frames(blocks, number):
    stop='tool_use' if any(b['type']=='tool_use' for b in blocks) else 'end_turn'
    meta={'id':f'synthetic_{number}','type':'message','role':'assistant','model':'synthetic-model','stop_reason':None,'stop_sequence':None,'content':[], 'usage':{'input_tokens':10}}
    out=[frame('message_start',message=meta)]
    for i,block in enumerate(blocks):
        first=dict(block)
        fields={'thinking':[('thinking','thinking_delta'),('signature','signature_delta')], 'text':[('text','text_delta')], 'tool_use':[('input','input_json_delta')]}.get(block['type'],[])
        for field,_ in fields:first[field]={} if field=='input' else ''
        out.append(frame('content_block_start',index=i,content_block=first))
        for field,kind in fields:
            value=json.dumps(block[field]) if field=='input' else block[field]
            key='partial_json' if field=='input' else field
            for piece in (value[:len(value)//2],value[len(value)//2:]):
                out.append(frame('content_block_delta',index=i,delta={'type':kind,key:piece}))
        out.append(frame('content_block_stop',index=i))
    out.append(frame('message_delta',delta={'stop_reason':stop,'stop_sequence':None},usage={'output_tokens':12}))
    out.append(frame('message_stop'))
    return out


class State:
    def __init__(self, manual=False, visible=True):
        self.requests=0;self.errors=[];self.phase='tool';self.manual=manual;self.visible=visible;self.saved=[]

    def respond(self, body):
        self.requests+=1
        base.require(self.requests<15,'unexpected reasoning retry')
        expected={'type':'enabled','display':'summarized','budget_tokens':2048} if self.manual else {'type':'adaptive','display':'summarized'}
        base.require(body.get('thinking')==expected,'thinking controls changed')
        if not self.manual:base.require(body.get('output_config',{}).get('effort')=='medium','adaptive effort changed')
        original=[block for message in body['messages'] if message['role']=='assistant' for block in message['content']]
        if self.phase in {'after_compact','recover'}:
            base.require(not any(b['type'] in {'thinking','redacted_thinking'} for b in original),'old provider state survived epoch reset')
            self.saved=[]
        else:
            for old in self.saved:
                base.require(sum(b==old for b in original)==1,'native reasoning lost, changed or duplicated')
        blocks=[{'type':'thinking','thinking':'SYNTHETIC_PUBLIC_REASONING' if self.visible else '', 'signature':f'synthetic_signature_{self.requests}'},
                {'type':'redacted_thinking','data':f'synthetic_redacted_{self.requests}'}]
        if self.requests==1:
            candidates=[]
            for tool in body['tools']:
                try:desc=json.loads(tool.get('description',''))
                except (ValueError,TypeError):continue
                if isinstance(desc,dict) and desc.get('namespace')=='fixture':candidates.append(tool)
            base.require(len(candidates)==1,'namespace tool missing')
            blocks.append({'type':'tool_use','id':'continuity-tool-1','name':candidates[0]['name'],'input':{'text':continuity.SENTINEL}})
        else:blocks.append({'type':'text','text':continuity.SENTINEL+' '+continuity.TOOL_RESULT})
        self.saved.extend(blocks[:2])
        return frames(blocks,self.requests)


class Handler(BaseHTTPRequestHandler):
    def log_message(self,*_):pass
    def do_POST(self):
        try:
            base.require(self.path=='/v1/messages','unexpected reasoning endpoint')
            base.require(self.headers.get('x-api-key')=='synthetic-key' and self.headers.get('Authorization') is None,'reasoning authentication changed')
            base.require(self.headers.get('anthropic-version')=='2023-06-01','Messages version changed')
            expected='interleaved-thinking-2025-05-14' if self.server.state.manual else None
            base.require(self.headers.get('anthropic-beta')==expected,'thinking beta header changed')
            output=self.server.state.respond(json.loads(self.rfile.read(int(self.headers['Content-Length']))))
            self.send_response(200);self.send_header('Content-Type','text/event-stream');self.send_header('Connection','close');self.end_headers()
            for chunk in output:self.wfile.write(chunk);self.wfile.flush()
        except Exception:self.server.state.errors.append('synthetic reasoning assertion failed')
        self.close_connection=True


def run(binary, gateway, manual=False, opaque_only=False):
    notifications=set();original=base.RpcClient.next
    def observe(rpc,*args,**kwargs):
        value=original(rpc,*args,**kwargs)
        if value.get('method') in {'item/reasoning/summaryPartAdded','item/reasoning/summaryTextDelta'}:notifications.add(value['method'])
        return value
    with patch.object(host,'route',lambda version:route(version,manual)), patch.object(durable,'State',lambda:State(manual,not opaque_only)), patch.object(durable,'Handler',Handler), patch.object(base.RpcClient,'next',observe):
        result=durable.run(binary,gateway)
    base.require(notifications==(set() if opaque_only else {'item/reasoning/summaryPartAdded','item/reasoning/summaryTextDelta'}),'reasoning notifications differ')
    return {**result,'schema':'gateway-reasoning-continuity/v1','contract':'claude_manual' if manual else 'claude_adaptive','reasoning_notifications':sorted(notifications),'opaque_only':opaque_only}


class HttpHandler(BaseHTTPRequestHandler):
    def log_message(self,*_):pass
    def do_POST(self):
        self.server.calls+=1
        body=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        blocks=[{'type':'thinking','thinking':'SYNTHETIC_PUBLIC_REASONING','signature':'synthetic-signature'},
                {'type':'redacted_thinking','data':'synthetic-redacted'}]
        blocks.append({'type':'tool_use','id':'call_1','name':'echo','input':{'text':'safe'}} if self.server.tool else {'type':'text','text':'Synthetic text.'})
        if body.get('stream'):data=b''.join(frames(blocks,self.server.calls));content='text/event-stream'
        else:
            data=json.dumps({'id':'synthetic_json','type':'message','role':'assistant','model':'synthetic-model','content':blocks,
                'stop_reason':'tool_use' if self.server.tool else 'end_turn','usage':{'input_tokens':10,'output_tokens':12}}).encode();content='application/json'
        self.send_response(200);self.send_header('Content-Type',content);self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)


def http_run(gateway, manual=False):
    with patch.object(host,'route',lambda version:route(version,manual)),patch.object(http_tests,'Handler',HttpHandler):
        result=http_tests.run(gateway)
    return {**result,'schema':'gateway-reasoning-http/v1','contract':'claude_manual' if manual else 'claude_adaptive'}


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--gateway-bin',type=Path,default=base.ROOT/'target/debug/agent-response-gateway')
    p.add_argument('--codex-bundle',type=Path,default=base.runtime.BUNDLE)
    p.add_argument('--manual',action='store_true');p.add_argument('--http-only',action='store_true');p.add_argument('--opaque-only',action='store_true');a=p.parse_args()
    binary=base.runtime.verify_bundle(a.codex_bundle,json.loads(base.runtime.LOCK.read_text()))
    try:result=http_run(a.gateway_bin.resolve(),a.manual) if a.http_only else run(binary,a.gateway_bin.resolve(),a.manual,a.opaque_only)
    except Exception:
        print(json.dumps({'schema':'gateway-reasoning-continuity/v1','status':'failed','provider_qualification':False}));raise SystemExit(1) from None
    print(json.dumps(result,sort_keys=True))
if __name__=='__main__':main()
