#!/usr/bin/env python3
"""Pinned Codex + durable Interactions replay; all traffic is synthetic loopback."""
import argparse
import contextlib
import hashlib
import json
import os
from pathlib import Path
import secrets
import sqlite3
import subprocess
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler
import conformance as base
import continuity
import interactions_harness as ih


def canonical(items):
    def item(v):
        v=dict(v);v.pop('id',None);v.pop('status',None)
        for key in ('phase','internal_chat_message_metadata_passthrough'):
            if v.get(key) is None:v.pop(key,None)
        if 'type' not in v and 'role' in v:v['type']='message'
        if isinstance(v.get('content'),list):
            v['content']=[{k:x for k,x in p.items() if not(k=='annotations' and x==[] and p.get('type')=='output_text')} for p in v['content']]
        return v
    return hashlib.sha256(json.dumps([item(v) for v in items],sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()).hexdigest()


class State:
    def __init__(self):self.requests=0;self.errors=[];self.phase='tool';self.last=[]
    def respond(self,body):
        self.requests+=1;self.last=body['input']
        base.require(self.requests<15,'unexpected continuity retry')
        base.require(body.get('store') is False and body.get('background') is False,'provider storage enabled')
        if self.requests>1 and self.phase not in {'after_compact','restart_compact','recover'}:
            base.require(any(s.get('type')=='thought' for s in self.last),'raw thought lost on resume')
        if self.phase in {'after_compact','recover'}:
            base.require(not any(s.get('type')=='thought' for s in self.last),'old signature survived epoch reset')
        if self.requests==1:
            candidates=[]
            for tool in body['tools']:
                try:description=json.loads(tool.get('description',''))
                except (ValueError,TypeError):continue
                if isinstance(description,dict) and description.get('namespace')=='fixture':candidates.append(tool)
            base.require(len(candidates)==1,'namespace tool missing')
            blocks=[{'type':'function_call','id':'continuity-tool-1','name':candidates[0]['name'],'arguments':{'text':continuity.SENTINEL}}]
        else:blocks=[{'type':'model_output','content':[{'type':'text','text':continuity.SENTINEL+' '+continuity.TOOL_RESULT}]}]
        return ih.frames([{'type':'thought','signature':f'synthetic-signature-{self.requests}'}]+blocks,self.requests)


class Handler(BaseHTTPRequestHandler):
    def log_message(self,*_):pass
    def do_POST(self):
        try:
            base.require(self.path=='/v1/interactions','unexpected provider endpoint')
            base.require(self.headers.get('x-goog-api-key')=='synthetic-key' and self.headers.get('Authorization') is None,'upstream authentication changed')
            frames=self.server.state.respond(json.loads(self.rfile.read(int(self.headers['Content-Length']))))
            self.send_response(200);self.send_header('Content-Type','text/event-stream');self.send_header('Connection','close');self.end_headers()
            for frame in frames:self.wfile.write(frame);self.wfile.flush()
        except Exception:self.server.state.errors.append('provider assertion failed')
        self.close_connection=True


def finish(rpc,tools=False,expected='completed'):
    calls=0;deadline=time.monotonic()+30
    while time.monotonic()<deadline:
        msg=rpc.pending.pop(0) if rpc.pending else rpc.next(max(.01,deadline-time.monotonic()))
        if msg.get('method')=='item/tool/call':
            base.require(tools and calls==0,'completed tool reexecuted');calls+=1
            rpc.send({'id':msg['id'],'result':{'contentItems':[{'type':'inputText','text':continuity.TOOL_RESULT}],'success':True}})
        elif msg.get('method')=='turn/completed':
            base.require(msg['params']['turn']['status']==expected,'continuation turn status differs');return calls
        elif 'id' in msg and msg.get('method'):raise AssertionError('unexpected host request')
    raise AssertionError('continuation timed out')


def run(binary,gateway_binary, resume_only=False):
    local=base.ROOT/'.local/interactions-continuity';local.mkdir(parents=True,exist_ok=True)
    with tempfile.TemporaryDirectory(dir=local) as temporary,contextlib.ExitStack() as cleanup:
        root=Path(temporary);home=root/'home';workspace=root/'workspace';home.mkdir(mode=0o700);workspace.mkdir()
        server=base.MockServer(('127.0.0.1',0),Handler);state=State();server.state=state
        threading.Thread(target=server.serve_forever,daemon=True).start();cleanup.callback(server.server_close);cleanup.callback(server.shutdown)
        env={k:v for k,v in os.environ.items() if k in {'PATH','LANG','TMPDIR'}}
        token=secrets.token_urlsafe(32);gateway_env={**env,'ARG_LOCAL_TOKEN':token,'ARG_MOCK_KEY':'synthetic-key'}
        codex_env={**env,'HOME':str(home),'CODEX_HOME':str(home),'ARG_CODEX_TEST_TOKEN':token}
        config=root/'gateway.toml';config.write_text(f'listen="127.0.0.1:0"\n[providers.mock]\nbase_url="http://127.0.0.1:{server.server_port}/v1"\napi_key_env="ARG_MOCK_KEY"\n[models."gpt-5.4"]\nprovider="mock"\nupstream_model="synthetic-model"\n'+ih.route(json.loads(base.runtime.LOCK.read_text())['version']))
        control_token=ih.setup(root,gateway_binary,config,gateway_env)
        manifest=base.embedded_contract.inspect_manifest(gateway_binary,config,env)
        gateway=codex=None;session=None;url=None
        def close():
            nonlocal codex,gateway
            if codex is not None:base.stop_process(codex);codex=None
            if gateway is not None:base.stop_process(gateway);gateway=None
        cleanup.callback(close)
        def start():
            nonlocal gateway,codex,session,url
            gateway=subprocess.Popen([str(gateway_binary),'serve','--config',str(config)],env=gateway_env,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,text=True)
            url=base.embedded_contract.read_ready(gateway,manifest)['base_url']
            if session is None:session=ih.create_session(url,control_token,manifest)
            (home/'config.toml').write_text(f'model="gpt-5.4"\nmodel_provider="gateway"\nweb_search="disabled"\nmodel_context_window=32768\nmodel_auto_compact_token_limit=24576\n[model_providers.gateway]\nname="Synthetic Interactions"\nbase_url="{url}"\nwire_api="responses"\nenv_key="ARG_CODEX_TEST_TOKEN"\nrequires_openai_auth=false\nsupports_websockets=false\nrequest_max_retries=0\nstream_max_retries=0\nhttp_headers={{"x-gateway-session"="{session["id"]}"}}\n')
            base.prepare_converted_profile(binary,home,codex_env)
            base.embedded_contract.validate_credential_split(manifest,gateway_env,codex_env,'ARG_CODEX_TEST_TOKEN',home)
            codex=subprocess.Popen([str(binary),'app-server'],cwd=workspace,env=codex_env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,text=True,bufsize=1)
            rpc=base.RpcClient(codex);rpc.call('initialize',{'clientInfo':{'name':'arg_interactions','version':'0.1.0'},'capabilities':{'experimentalApi':True}});rpc.send({'method':'initialized','params':{}});return rpc
        def transition(kind,portable=None):
            current=ih.control(url,control_token,'/__continuation/sessions/'+session['id'])
            return ih.control(url,control_token,'/__continuation/sessions/'+session['id']+'/transitions',{'revision':current['revision'],'kind':kind,'portable_sha256':None if portable is None else canonical(portable),'decision_reference':'synthetic-host-decision','pending_tools':False,'pending_approvals':False})
        tool={'type':'function','name':'echo','description':'Synthetic tool','inputSchema':{'type':'object','properties':{'text':{'type':'string'}},'required':['text'],'additionalProperties':False}}
        settings={'model':'gpt-5.4','modelProvider':'gateway','cwd':str(workspace),'sandbox':'read-only','approvalPolicy':'on-request','approvalsReviewer':'user','allowProviderModelFallback':False,'dynamicTools':[{'type':'namespace','name':'fixture','description':'Synthetic namespace','tools':[tool]}]}
        rpc=start();tid=rpc.call('thread/start',{**settings,'ephemeral':False})['thread']['id']
        def turn(phase,expected='completed'):
            state.phase=phase;rpc.call('turn/start',{'threadId':tid,'input':[{'type':'text','text':continuity.SENTINEL+' '+phase}]})
            try:return finish(rpc,tools=phase=='tool',expected=expected)
            except AssertionError:raise AssertionError('continuation failed in '+phase) from None
        base.require(turn('tool')==1 and state.requests==2,'initial tool roundtrip failed')
        close();rpc=start();rpc.call('thread/resume',{**settings,'threadId':tid});turn('restart')
        # Offline damage injection: authoritative finalization retained, only payload removed.
        close();db=root/'continuation-store/continuation.sqlite3'
        with sqlite3.connect(db) as conn:conn.execute('UPDATE records SET envelope=NULL')
        rpc=start();rpc.call('thread/resume',{**settings,'threadId':tid});turn('repair')
        close()
        with sqlite3.connect(db) as conn:base.require(conn.execute('SELECT count(*) FROM records WHERE envelope IS NULL').fetchone()[0]==0,'payload repair incomplete')
        if resume_only:
            return {'schema':'gateway-interactions-continuity/v1','status':'passed','scope':'restart-and-payload-repair','upstream_requests':state.requests,'tool_executions':1,'compaction_qualified':False,'provider_qualification':False}
        rpc=start();rpc.call('thread/resume',{**settings,'threadId':tid})
        transition('compact_begin');state.phase='compact';rpc.call('thread/compact/start',{'threadId':tid});finish(rpc)
        info=continuity.history(rpc,tid,home);history=home/info['history_reference']
        rows=[json.loads(line) for line in history.read_text().splitlines()]
        compacted=[r['payload'] for r in rows if r.get('type')=='compacted']
        base.require(bool(compacted),'compaction record missing')
        portable=compacted[-1].get('replacement_history')
        base.require(isinstance(portable,list) and portable,'portable compaction history missing')
        base.require(continuity.SENTINEL in json.dumps(portable) and continuity.TOOL_RESULT in json.dumps(portable),'portable summary lost required state')
        base.require(all(v.get('type')=='message' and v.get('role') in {'user','assistant'} for v in portable),'nonportable compaction item')
        portable_text=json.dumps({'schema':'gateway-portable-context/v1','history':portable,'completed_tools':[{'call_id':'continuity-tool-1','result':continuity.TOOL_RESULT}]},sort_keys=True,separators=(',',':'))
        portable=[{'type':'message','role':'user','content':[{'type':'input_text','text':portable_text}]}]
        old_tid=tid
        tid=rpc.call('thread/start',{**settings,'ephemeral':False})['thread']['id']
        base.require(tid!=old_tid and history.exists(),'host transition did not preserve prior thread')
        transition('compact_commit',portable)
        state.phase='after_compact';rpc.call('turn/start',{'threadId':tid,'input':[{'type':'text','text':portable_text}]});finish(rpc)
        close();rpc=start();rpc.call('thread/resume',{**settings,'threadId':tid});turn('restart_compact')
        # Missing execution authority must block, even though Codex retained valid ciphertext.
        close()
        with sqlite3.connect(db) as conn:
            current=json.loads(conn.execute('SELECT value FROM sessions WHERE id=?',(session['id'],)).fetchone()[0])
            conn.execute('DELETE FROM records WHERE id=?',(current['head'],))
        rpc=start();rpc.call('thread/resume',{**settings,'threadId':tid});before=state.requests;turn('missing_record','failed')
        base.require(state.requests==before,'missing execution record triggered inference')
        # A crashed pending attempt is not proof of completion, even for real Codex.
        close()
        with sqlite3.connect(db) as conn:
            current=json.loads(conn.execute('SELECT value FROM sessions WHERE id=?',(session['id'],)).fetchone()[0])
            conn.execute("INSERT INTO attempts VALUES(?,?,?,?,'pending',?)",('resp_synthetic_pending',session['id'],current['epoch'],'synthetic-pending',16777216))
            current['status']='pending';current['revision']+=1
            conn.execute('UPDATE sessions SET value=? WHERE id=?',(json.dumps(current),session['id']))
        rpc=start();rpc.call('thread/resume',{**settings,'threadId':tid});turn('pending_attempt','failed')
        base.require(state.requests==before,'pending attempt triggered inference')
        # Host explicitly abandons prior provider context and starts a portable new thread/epoch.
        portable=[{'type':'message','role':'user','content':[{'type':'input_text','text':continuity.SENTINEL+' '+continuity.TOOL_RESULT}]}]
        transition('recover',portable)
        tid=rpc.call('thread/start',{**settings,'ephemeral':False})['thread']['id']
        state.phase='recover';rpc.call('turn/start',{'threadId':tid,'input':[{'type':'text','text':continuity.SENTINEL+' '+continuity.TOOL_RESULT}]});finish(rpc)
        base.require(not state.errors,'mock provider validation failed')
        return {'schema':'gateway-interactions-continuity/v1','status':'passed','upstream_requests':state.requests,'tool_executions':1,'restart':True,'payload_repair':True,'compaction_restart':True,'missing_record_blocked':True,'pending_attempt_blocked':True,'explicit_recovery':True,'provider_qualification':False}


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--gateway-bin',type=Path,default=base.ROOT/'target/debug/agent-response-gateway');p.add_argument('--codex-bundle',type=Path,default=base.runtime.BUNDLE);p.add_argument('--resume-only',action='store_true');a=p.parse_args()
    binary=base.runtime.verify_bundle(a.codex_bundle,json.loads(base.runtime.LOCK.read_text()))
    try:result=run(binary,a.gateway_bin.resolve(),a.resume_only)
    except Exception as e:
        result={'schema':'gateway-interactions-continuity/v1','status':'failed','error_class':type(e).__name__}
        if isinstance(e,AssertionError):result['check']=str(e)
        print(json.dumps(result));raise SystemExit(1) from None
    print(json.dumps(result))
if __name__=='__main__':main()
