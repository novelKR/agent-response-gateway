#!/usr/bin/env python3
"""Pinned Codex + explicit synthetic DeepSeek/OpenRouter reasoning and durable replay."""
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
import reasoning_continuity as claude


def route(version, router=False):
    text=claude.route(version).replace('api="messages"','api="chat_completions"').replace('auth="api_key"','auth="bearer"').replace('messages_version="2023-06-01"\n','')
    start=text.index('[capability_profiles.synthetic-reasoning.reasoning_contract]\n')
    end=text.index('[capability_profiles.synthetic-reasoning.support]',start)
    contract=('kind="open_router"\nversion=1\nprovider_endpoint="synthetic/exact"\nefforts=["low","high"]\ndefault_effort="high"\nformats=["anthropic-claude-v1"]\n' if router else
              'kind="deep_seek"\nversion=1\nefforts=["low","high","max"]\ndefault_effort="high"\n')
    text=text[:start]+'[capability_profiles.synthetic-reasoning.reasoning_contract]\n'+contract+text[end:]
    text=text.replace('instruction_hierarchy="bridged_instruction_envelope"','instruction_hierarchy="native"' if router else 'instruction_hierarchy="bridged_chat_instruction_envelope"')
    if not router:
        text=text.replace('parallel_tool_control="native"','parallel_tool_control="bridged_parallel_permission"').replace('strict_structured_output="native"\n','').replace('structured_output="native"\n','')
    return text


def chunk(delta,number,finish=None,usage=None):
    value={'id':f'synthetic_{number}','object':'chat.completion.chunk','created':number,'model':'synthetic-model',
           'choices':[{'index':0,'delta':delta,'finish_reason':finish}]}
    if usage is not None:value['usage']=usage
    return ('data: '+json.dumps(value)+'\n\n').encode()


def usage():return {'prompt_tokens':10,'completion_tokens':12,'total_tokens':22}


def frames(assistant,number):
    result=[chunk({'role':'assistant','content':None},number)]
    for field in ('reasoning','reasoning_content'):
        if field in assistant:
            text=assistant[field];cut=len(text)//2
            for value in (text[:cut],text[cut:]):result.append(chunk({field:value},number))
    if 'reasoning_details' in assistant:
        for detail in assistant['reasoning_details']:
            field={'reasoning.text':'text','reasoning.summary':'summary','reasoning.encrypted':'data'}[detail['type']]
            initial={k:v for k,v in detail.items() if k not in {field,'signature'}}
            initial[field]=''
            if 'signature' in detail:initial['signature']=''
            result.append(chunk({'reasoning_details':[initial]},number))
            for key in (field,'signature'):
                if key not in detail:continue
                text=detail[key];cut=len(text)//2
                for value in (text[:cut],text[cut:]):result.append(chunk({'reasoning_details':[{'index':detail['index'],key:value}]},number))
    if assistant.get('content') is not None:
        result.append(chunk({'content':assistant['content']},number))
    for i,tool in enumerate(assistant.get('tool_calls',[])):
        result.append(chunk({'tool_calls':[{'index':i,'id':tool['id'],'type':'function','function':{'name':tool['function']['name'],'arguments':''}}]},number))
        text=tool['function']['arguments'];cut=len(text)//2
        for value in (text[:cut],text[cut:]):result.append(chunk({'tool_calls':[{'index':i,'function':{'arguments':value}}]},number))
    result.append(chunk({},number,'tool_calls' if assistant.get('tool_calls') else 'stop',usage()))
    result.append(b'data: [DONE]\n\n');return result


def assistant(number,router=False,visible=True):
    result={'role':'assistant','content':continuity.SENTINEL+' '+continuity.TOOL_RESULT+' '+str(number)}
    if router:
        result['reasoning']='SYNTHETIC_MIRROR_MUST_NOT_DISPLAY'
        details=[]
        if visible:details.append({'type':'reasoning.text','text':'SYNTHETIC_PUBLIC_REASONING','signature':f'synthetic_signature_{number}', 'id':f'detail_{number}','index':0,'format':'anthropic-claude-v1'})
        details.append({'type':'reasoning.encrypted','data':f'synthetic_encrypted_{number}','id':f'encrypted_{number}','index':len(details),'format':'anthropic-claude-v1'})
        result['reasoning_details']=details
    else:result['reasoning_content']='SYNTHETIC_PUBLIC_REASONING '+str(number) if visible else ''
    return result


class State:
    def __init__(self,router=False,visible=True):self.requests=0;self.errors=[];self.phase='tool';self.router=router;self.visible=visible;self.saved=[]
    def respond(self,body):
        self.requests+=1;base.require(self.requests<15,'unexpected reasoning retry')
        if self.router:
            base.require(body.get('provider')=={'only':['synthetic/exact'],'allow_fallbacks':False,'require_parameters':True},'router endpoint policy changed')
            base.require(body.get('reasoning')=={'enabled':True,'exclude':False,'effort':'high'} and 'reasoning_effort' not in body,'router reasoning controls changed')
        else:
            base.require(body.get('thinking')=={'type':'enabled'} and body.get('reasoning_effort')=='high','DeepSeek reasoning controls changed')
            base.require('parallel_tool_calls' not in body and not any(m['role']=='developer' for m in body['messages']),'unsupported DeepSeek wire field')
        originals=[m for m in body['messages'] if m['role']=='assistant']
        if self.phase in {'after_compact','recover'}:
            base.require(not originals,'old assistant state survived epoch reset');self.saved=[]
        for old in self.saved:base.require(sum(m==old for m in originals)==1,'native assistant reasoning lost, changed or duplicated')
        result=assistant(self.requests,self.router,self.visible)
        if self.requests==1:
            candidates=[]
            for tool in body['tools']:
                tool=tool['function']
                try:desc=json.loads(tool.get('description',''))
                except (ValueError,TypeError):continue
                if isinstance(desc,dict) and desc.get('namespace')=='fixture':candidates.append(tool)
            base.require(len(candidates)==1,'namespace tool missing')
            result['content']=None;result['tool_calls']=[{'id':'continuity-tool-1','type':'function','function':{'name':candidates[0]['name'],'arguments':json.dumps({'text':continuity.SENTINEL})}}]
        self.saved.append(result)
        return frames(result,self.requests)


class Handler(BaseHTTPRequestHandler):
    def log_message(self,*_):pass
    def do_POST(self):
        try:
            base.require(self.path=='/v1/chat/completions','unexpected Chat endpoint')
            base.require(self.headers.get('Authorization')=='Bearer synthetic-key' and self.headers.get('x-api-key') is None,'Chat authentication changed')
            output=self.server.state.respond(json.loads(self.rfile.read(int(self.headers['Content-Length']))))
            self.send_response(200);self.send_header('Content-Type','text/event-stream');self.send_header('Connection','close');self.end_headers()
            for value in output:self.wfile.write(value);self.wfile.flush()
        except Exception:self.server.state.errors.append('synthetic Chat reasoning assertion failed')
        self.close_connection=True


def run(binary,gateway,router=False,opaque_only=False):
    notifications=set();original=base.RpcClient.next
    def observe(rpc,*args,**kwargs):
        value=original(rpc,*args,**kwargs)
        if value.get('method') in {'item/reasoning/summaryPartAdded','item/reasoning/summaryTextDelta'}:
            notifications.add(value['method'])
            base.require('SYNTHETIC_MIRROR_MUST_NOT_DISPLAY' not in json.dumps(value),'duplicate mirror displayed')
        return value
    with patch.object(host,'route',lambda version:route(version,router)),patch.object(durable,'State',lambda:State(router,not opaque_only)),patch.object(durable,'Handler',Handler),patch.object(base.RpcClient,'next',observe):result=durable.run(binary,gateway)
    base.require(notifications==(set() if opaque_only else {'item/reasoning/summaryPartAdded','item/reasoning/summaryTextDelta'}),'reasoning notifications differ')
    return {**result,'schema':'gateway-reasoning-continuity/v1','contract':'open_router' if router else 'deep_seek','opaque_only':opaque_only,'reasoning_notifications':sorted(notifications)}


class HttpHandler(BaseHTTPRequestHandler):
    router=False
    def log_message(self,*_):pass
    def do_POST(self):
        self.server.calls+=1;body=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        result=assistant(self.server.calls,self.router)
        if self.server.tool:result['tool_calls']=[{'id':'call_1','type':'function','function':{'name':'echo','arguments':json.dumps({'text':'safe'})}}]
        if body.get('stream'):data=b''.join(frames(result,self.server.calls));content='text/event-stream'
        else:data=json.dumps({'id':'synthetic_json','created':1,'object':'chat.completion','model':'synthetic-model','choices':[{'index':0,'message':result,'finish_reason':'tool_calls' if self.server.tool else 'stop'}],'usage':usage()}).encode();content='application/json'
        self.send_response(200);self.send_header('Content-Type',content);self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)


def http_run(gateway,router=False):
    with patch.object(host,'route',lambda version:route(version,router)),patch.object(http_tests,'Handler',HttpHandler),patch.object(HttpHandler,'router',router):result=http_tests.run(gateway)
    return {**result,'schema':'gateway-reasoning-http/v1','contract':'open_router' if router else 'deep_seek'}


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--gateway-bin',type=Path,default=base.ROOT/'target/debug/agent-response-gateway');p.add_argument('--codex-bundle',type=Path,default=base.runtime.BUNDLE)
    p.add_argument('--router',action='store_true');p.add_argument('--opaque-only',action='store_true');p.add_argument('--http-only',action='store_true');a=p.parse_args()
    binary=base.runtime.verify_bundle(a.codex_bundle,json.loads(base.runtime.LOCK.read_text()))
    try:result=http_run(a.gateway_bin.resolve(),a.router) if a.http_only else run(binary,a.gateway_bin.resolve(),a.router,a.opaque_only)
    except Exception:
        print(json.dumps({'schema':'gateway-reasoning-continuity/v1','status':'failed','provider_qualification':False}));raise SystemExit(1) from None
    print(json.dumps(result,sort_keys=True))
if __name__=='__main__':main()
