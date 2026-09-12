#!/usr/bin/env python3
"""Synthetic HTTP failure acceptance; never reports payloads or cryptographic material."""
import argparse
import contextlib
import copy
import json
import os
from pathlib import Path
import secrets
import sqlite3
import subprocess
import tempfile
import threading
import urllib.request
import urllib.error
from http.server import BaseHTTPRequestHandler
import conformance as base
import interactions_harness as ih

class Handler(BaseHTTPRequestHandler):
    def log_message(self,*_):pass
    def do_POST(self):
        self.server.calls+=1
        body=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        steps=[{'type':'thought','signature':'synthetic-secret-signature'},{'type':'model_output','content':[{'type':'text','text':'Synthetic text.'}]}]
        if self.server.tool:steps=[{'type':'function_call','id':'call_1','name':'echo','arguments':{'text':'safe'}}]
        if body.get('stream'):
            data=b''.join(ih.frames(steps,self.server.calls));content='text/event-stream'
        else:
            data=json.dumps({'id':'provider_test','object':'interaction','model':'synthetic-model','status':'requires_action' if self.server.tool else 'completed','steps':steps}).encode();content='application/json'
        self.send_response(200);self.send_header('Content-Type',content);self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)

def run(binary):
    local=base.ROOT/'.local/interactions-http';local.mkdir(parents=True,exist_ok=True)
    with tempfile.TemporaryDirectory(dir=local) as temporary,contextlib.ExitStack() as cleanup:
        root=Path(temporary);server=base.MockServer(('127.0.0.1',0),Handler);server.calls=0;server.tool=False
        threading.Thread(target=server.serve_forever,daemon=True).start();cleanup.callback(server.server_close);cleanup.callback(server.shutdown)
        env={k:v for k,v in os.environ.items() if k in {'PATH','LANG','TMPDIR'}};token=secrets.token_urlsafe(32);env.update(ARG_LOCAL_TOKEN=token,ARG_MOCK_KEY='synthetic-key')
        config=root/'gateway.toml';config.write_text(f'listen="127.0.0.1:0"\n[providers.mock]\nbase_url="http://127.0.0.1:{server.server_port}/v1"\napi_key_env="ARG_MOCK_KEY"\n[models."gpt-5.4"]\nprovider="mock"\nupstream_model="synthetic-model"\n'+ih.route('0.154.0'))
        control=ih.setup(root,binary,config,env);manifest=base.embedded_contract.inspect_manifest(binary,config,env)
        child=None;url=None
        def stop():
            nonlocal child
            if child is not None:base.stop_process(child);child=None
        cleanup.callback(stop)
        def start():
            nonlocal child,url
            child=subprocess.Popen([str(binary),'serve','--config',str(config)],env=env,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,text=True)
            url=base.embedded_contract.read_ready(child,manifest)['base_url']
        def post(body,session=None,credential=None,path='/responses'):
            headers={'Authorization':'Bearer '+(credential or token),'Content-Type':'application/json'}
            if session is not None:headers['x-gateway-session']=session
            request=urllib.request.Request(url+path,json.dumps(body).encode(),headers)
            try:
                with urllib.request.build_opener(urllib.request.ProxyHandler({})).open(request,timeout=10) as r:
                    try:data=r.read()
                    except Exception:data=b''
                    return r.status,data
            except urllib.error.HTTPError as e:return e.code,e.read()
        start();s=ih.create_session(url,control,manifest);sid=s['id']
        body={'model':'gpt-5.4','input':[{'type':'message','role':'user','content':[{'type':'input_text','text':'synthetic'}]}]}
        for ident in (None,'missing_session'):
            base.require(post(body,ident)[0]==409,'missing session accepted')
        base.require(post(body,sid,control)[0]==401,'host token authorized model request')
        try:ih.control(url,token,'/__continuation/sessions',{'origin':s['origin']})
        except urllib.error.HTTPError as e:base.require(e.code==401,'control authentication differs')
        else:raise AssertionError('Codex token authorized host control')
        for key,value in [('temperature',0.3),('previous_response_id','resp_unknown')]:
            invalid={**body,key:value};base.require(post(invalid,sid)[0]==400,'unsupported input accepted')
        base.require(server.calls==0,'admission failure called provider')
        status,data=post(body,sid);base.require(status==200,'JSON route failed');out=json.loads(data)['output']
        replay={**body,'input':body['input']+out+[{'type':'message','role':'user','content':[{'type':'input_text','text':'next'}]}]}
        changed=copy.deepcopy(replay);changed['input'][0]['content'][0]['text']='altered'
        base.require(post(changed,sid)[0]==409,'history mutation accepted')
        public=[i for i,x in enumerate(replay['input']) if x.get('type')=='reasoning' and x.get('summary')]
        if public:
            changed=copy.deepcopy(replay);changed['input'][public[0]]['summary'][0]['text']='changed display'
            base.require(post(changed,sid)[0]==409,'public reasoning mutation accepted')
        changed=copy.deepcopy(replay);opaque=next(x for x in changed['input'] if x.get('encrypted_content'));opaque['encrypted_content']=opaque['encrypted_content'][:-1]+('1' if opaque['encrypted_content'][-1]=='0' else '0')
        base.require(post(changed,sid)[0]==409,'tampered ciphertext accepted')
        other=ih.create_session(url,control,manifest)
        base.require(post(replay,other['id'])[0]==409,'cross-session replay accepted')
        base.require(server.calls==1,'invalid replay called provider')
        base.require(post(replay,sid)[0]==200,'JSON replay failed')
        stop();db=root/'continuation-store/continuation.sqlite3'
        # Test damage is installed offline. The running store retains exclusive ownership.
        with sqlite3.connect(db) as c:c.execute("CREATE TRIGGER deny_attempt BEFORE INSERT ON attempts BEGIN SELECT RAISE(ABORT,'synthetic'); END")
        start();fresh=ih.create_session(url,control,manifest);before=server.calls
        base.require(post(body,fresh['id'])[0]==409 and server.calls==before,'failed attempt commit dispatched inference')
        stop()
        with sqlite3.connect(db) as c:
            c.execute('DROP TRIGGER deny_attempt');c.execute("CREATE TRIGGER deny_finalize BEFORE INSERT ON records BEGIN SELECT RAISE(ABORT,'synthetic'); END")
        start();fresh=ih.create_session(url,control,manifest);server.tool=True
        tool={'type':'function','name':'echo','parameters':{'type':'object','properties':{'text':{'type':'string'}}}}
        status,data=post({**body,'tools':[tool],'stream':True},fresh['id'])
        base.require(b'response.output_item.done' not in data and b'encrypted_content' not in data and b'response.completed' not in data,'uncommitted executable output escaped')
        before=server.calls;stop();start()
        base.require(post({**body,'tools':[tool]},fresh['id'])[0]==409 and server.calls==before,'unknown attempt replayed after restart')
        stop()
        old=env['ARG_CONTINUATION_KEY'];env['ARG_CONTINUATION_KEY']=secrets.token_hex(32)
        bad=subprocess.run([str(binary),'serve','--config',str(config)],env=env,capture_output=True,timeout=10)
        base.require(bad.returncode!=0 and b'gateway-ready' not in bad.stdout,'changed key accepted')
        env['ARG_CONTINUATION_KEY']=old
        return {'schema':'gateway-interactions-http/v1','status':'passed','upstream_requests':server.calls,'admission_zero_calls':True,'tamper_and_history_rejected':True,'commit_before_dispatch':True,'commit_before_tool_output':True,'unknown_restart_blocked':True,'changed_key_rejected':True}

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--gateway-bin',type=Path,default=base.ROOT/'target/debug/agent-response-gateway');a=p.parse_args()
    try:result=run(a.gateway_bin.resolve())
    except Exception as e:
        result={'schema':'gateway-interactions-http/v1','status':'failed','error_class':type(e).__name__}
        if isinstance(e,AssertionError):result['check']=str(e)
        print(json.dumps(result));raise SystemExit(1) from None
    print(json.dumps(result))
if __name__=='__main__':main()
