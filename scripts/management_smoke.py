#!/usr/bin/env python3
"""Exercise actual optional management binaries with a synthetic loopback provider."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import queue
import secrets
import subprocess
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.error import HTTPError, URLError
from urllib.request import Request, ProxyHandler, build_opener

PHASE = 'setup'
SCHEMA = 'gateway-management-http/v1'
ACTIONS = ['read_state','read_usage','read_operations','reconcile','configuration_stage','configuration_select',
           'runtime_start','runtime_stop','runtime_restart','continuation_transition','package_install','package_enable','package_disable','package_select',
           'team_subject_register','team_permissions_change','team_credential_issue','team_credential_revoke','team_credential_rotate']
CLIENT = build_opener(ProxyHandler({}))


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write(path, value):
    path.write_text(json.dumps(value), encoding='utf-8')
    path.chmod(0o600)


def run(arguments, env, success=True):
    result = subprocess.run([str(a) for a in arguments], env=env, capture_output=True, timeout=45)
    assert (result.returncode == 0) == success
    return result.stdout


def http(base, path, token=None, body=None, method=None, headers=None):
    headers = dict(headers or {})
    if token:
        headers['Authorization'] = 'Bearer ' + token
    if body is not None:
        headers['Content-Type'] = 'application/json'
    request = Request(base + path, data=None if body is None else json.dumps(body).encode(), headers=headers, method=method)
    try:
        response = CLIENT.open(request, timeout=30)
    except HTTPError as error:
        response = error
    with response:
        raw = response.read(2 * 1024 * 1024 + 1)
        assert len(raw) <= 2 * 1024 * 1024
        return response.status, {k.lower():v for k,v in response.headers.items()}, raw


class Upstream(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        if self.headers.get('Authorization') != 'Bearer synthetic-provider-key' or self.path != '/v1/responses' or body['model'] != 'synthetic-model':
            self.server.failed = True
            self.send_error(500)
            return
        self.server.calls += 1
        value = {'id':'synthetic-response','object':'response','status':'completed','output':[],'usage':{'input_tokens':2,'output_tokens':1,'total_tokens':3}}
        raw = json.dumps(value).encode()
        if body.get('stream'):
            raw = b'data: ' + json.dumps({'type':'response.completed','response':value}).encode() + b'\n\n'
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream' if body.get('stream') else 'application/json')
        self.send_header('Content-Length', str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)


def verify(args):
    global PHASE
    root = args.state_dir.resolve()
    root.mkdir(parents=True, exist_ok=False)
    root.chmod(0o700)
    provider = ThreadingHTTPServer(('127.0.0.1', 0), Upstream)
    provider.calls, provider.failed = 0, False
    threading.Thread(target=provider.serve_forever, daemon=True).start()
    config = root / 'gateway.toml'
    config.write_text(f"listen='127.0.0.1:0'\nlocal_token_env='MODEL_KEY'\n[providers.mock]\nbase_url='http://127.0.0.1:{provider.server_port}/v1'\napi_key_env='PROVIDER_KEY'\n" + ''.join(f"[models.{alias}]\nprovider='mock'\nupstream_model='synthetic-model'\n" for alias in ['a','b']), encoding='utf-8')
    config.chmod(0o600)
    env = {'MANAGEMENT_KEY':secrets.token_hex(32),'READ_KEY':secrets.token_hex(32),'MODEL_KEY':secrets.token_hex(32),'PROVIDER_KEY':'synthetic-provider-key'}
    if os.name == 'nt':
        env['SYSTEMROOT'] = os.environ['SYSTEMROOT']
    if args.managed:
        continuation=root/'continuation';continuation.mkdir(mode=0o700)
        store_id=json.loads(run([args.gateway,'init-continuation','--directory',continuation],env))['store_id']
        env['CONTINUATION_KEY']=secrets.token_hex(32);env['CONTROL_KEY']=secrets.token_hex(32)
        with config.open('a',encoding='utf-8') as stream:
            stream.write("[models.m]\nprovider='mock'\nupstream_model='synthetic-model'\napi='messages'\nauth='api_key'\nmessages_version='2023-06-01'\ncapability_profile='managed'\ncontinuation_mode='managed'\n[capability_profiles.managed]\nversion='1'\nprovider='mock'\nupstream_model='synthetic-model'\napi='messages'\ncontext_window=32768\nmax_output_tokens=8192\ntested_codex_version='0.154.0'\n[capability_profiles.managed.reasoning_contract]\nkind='claude_adaptive'\nversion=1\nefforts=['low','medium','high']\ndefault_effort='medium'\n[capability_profiles.managed.support]\nfunction_tools='native'\nmax_output_tokens='native'\nreasoning_effort='native'\nreasoning_summary='native'\nreasoning_items='native'\ntool_choice='native'\n")
            stream.write(f"[continuation]\ndirectory={json.dumps(str(continuation))}\nstore_id='{store_id}'\nrealm='synthetic'\ngeneration='1'\nkey_id='synthetic'\nkey_env='CONTINUATION_KEY'\ncontrol_token_env='CONTROL_KEY'\nmax_store_bytes=16777216\n")
    # Existing Rust profile validator creates the exact package; fixture metadata is synthetic.
    profile_source, profile_package, profile_store = root/'profile-source.json', root/'profile-package.json', root/'profiles'
    profile_store.mkdir(mode=0o700)
    write(profile_source, {'schema':'gateway-profile-pack/v2','id':'synthetic','version':'1.0.0','capabilities':{'functions':{'api':'responses','context_window':8192,'max_output_tokens':2048,'tested_codex_version':'synthetic','support':{'function_tools':'native'}}},'policies':{},'editing':{},'evidence':[],'notices':{'LICENSE':'Synthetic fixture, no external material.'}})
    # The supported v1 data contract is sufficient to verify installed/selected/effective composition.
    source = json.loads(profile_source.read_text());source['schema']='gateway-profile-pack/v1';source.pop('editing');write(profile_source,source)
    packaged = json.loads(run([args.gateway,'profile-pack','package','--source',profile_source,'--output',profile_package],env))
    profile_digest = packaged['package_sha256']
    settings = {'schema':'gateway-management-registration/v1','target':'gateway','listen':'127.0.0.1:0','journal':str(root/'journal'),
                'runtime':{'directory':str(root/'runtime'),'executable':str(args.child.resolve()),'executable_sha256':sha(args.child),'credential_generation':'synthetic-v1',
                           'sources':{'candidate':{'configuration':str(config),'extensions_lock':None,'profile_packs_lock':str(profile_store/'active.json')}},
                           'environment':{name:name for name in env if name not in {'MANAGEMENT_KEY','READ_KEY'}}},
                'credentials':[{'subject':'local:operator','credential':'local:manage','token_env':'MANAGEMENT_KEY','read_only':False,'grants':[{'target':'gateway','action':a} for a in ACTIONS]},
                               {'subject':'local:reader','credential':'local:read','token_env':'READ_KEY','read_only':True,'grants':[{'target':'gateway','action':a} for a in ACTIONS[:3]]}],
                'native':None,'profile_packs':{'directory':str(root/'profile-manager'),'store':str(profile_store),'driver':None,'sources':{'synthetic':{'path':str(profile_package),'package_sha256':profile_digest}},'recorder_bindings':{}},
                'usage':None,'web':None,'team':None}
    if args.managed:
        settings['continuation']=str(root/'control')
    if args.web:
        settings['web']={'directory':str(args.web.resolve()),'manifest_sha256':sha(args.web/'web-manifest.json')}
    recorder_binding = None
    if args.recorder:
        import extension_manager as manager
        store=manager.open_store(root/'native')
        ledger=manager.private_dir(manager.private_dir(store/'usage',create=True)/'primary',create=True)
        recorder_config=manager.canonical({'schema':'gateway-usage-recorder-config/v1','destinations':[]})
        manager.write_new(ledger/'recorder.json',recorder_config)
        run([args.recorder,'init','--store',ledger],env)
        package=root/'recorder-package'
        recorder_digest=manager.package_binary(args.recorder.resolve(),Path(__file__).resolve().parents[1]/'LICENSE',package,'usage-recorder','0.1.0','usage_recorder')
        recorder_binding={'store_id':'primary','mode':'durable_local','queue_capacity':256,'ack_timeout_ms':5000,'config_sha256':hashlib.sha256(recorder_config).hexdigest()}
        interpreter=args.python.resolve();driver=(args.extension_manager or Path(__file__).resolve().parent/'extension_manager.py').resolve()
        settings['native']={'directory':str(root/'native-manager'),'store':str(store),'driver':{'python':str(interpreter),'python_sha256':sha(interpreter),'manager':str(driver),'manager_sha256':sha(driver),'timeout_ms':15000},'sources':{'recorder':{'path':str(package),'package_sha256':recorder_digest}},'recorder_bindings':{'primary':recorder_binding}}
        settings['usage']={'directory':str(ledger)}
        settings['runtime']['sources']['candidate']['extensions_lock']=str(store/'active.json')
    if args.team:
        settings['team']={'directory':str(root/'team'),'requests':str(root/'requests'),'listen':'127.0.0.1:0'}
    registration = root/'registration.json'
    write(registration,settings)
    prefix = [args.manager,'--registration',registration]
    for component in ['management','runtime','profile-pack'] + (['native'] if args.recorder else []) + (['continuation'] if args.managed else []) + (['team','team-requests'] if args.team else []):
        run(prefix+['init','--component',component],env)
    run(prefix+['init','--component','management'],env,False)
    process = subprocess.Popen([str(a) for a in prefix+['serve','--parent-stdin']], env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    messages=queue.Queue()
    threading.Thread(target=lambda:messages.put(process.stdout.readline()),daemon=True).start()
    try:
        PHASE='startup'
        listeners=json.loads(messages.get(timeout=15));base=listeners['management'];token=env['MANAGEMENT_KEY']
        assert (listeners['team'] is not None)==args.team
        def call(path,body=None,token=token,method=None,headers=None):
            code,headers,raw=http(base,'/management/v1'+path,token,body,method,headers)
            return code,headers,json.loads(raw)
        def prepare(command,key):
            code,_,response=call('/preflight',{'schema':SCHEMA,'target':'gateway','idempotency_key':key,'command':command})
            assert code==200, 'preflight_failed'
            return response['data']['submission']
        def apply(command,key):
            submission=prepare(command,key);code,_,response=call('/operations',submission);assert code==202
            operation=response['data']['operation_id']
            for _ in range(300):
                code,_,response=call(f'/operations/{operation}?target=gateway');assert code==200
                state=response['data']['operation']['state']
                if state not in ['queued','running']:
                    assert state=='succeeded', 'effect_failed'
                    return operation,submission
                time.sleep(.02)
            raise AssertionError('operation_timeout')
        def state():
            code,_,response=call('/state?target=gateway');assert code==200
            return {m['id']:m['observation'].get('data') for m in response['data']['modules']}
        PHASE='boundary'
        assert call('/state?target=gateway',token=env['MODEL_KEY'])[0]==401
        assert call('/state?target=gateway',headers={'Host':'untrusted.invalid'})[0]==400
        assert call('/state?target=gateway',headers={'Origin':'https://untrusted.invalid'})[0]==403
        if args.recorder:
            PHASE='native-packages'
            apply({'kind':'package_install','family':'native','source':'recorder'},'install-recorder')
            apply({'kind':'package_enable','family':'native','package':{'id':'usage-recorder','version':'0.1.0','package_sha256':recorder_digest},'grants':manager.RECORDER_PERMISSIONS,'recorder':'primary'},'enable-recorder')
        PHASE='packages'
        _,installed=apply({'kind':'package_install','family':'profile_pack','source':'synthetic'},'install')
        assert state()['profiles']['store']['inventory']['installed'][0]['verified']
        selection={'id':'synthetic','version':'1.0.0','package_sha256':profile_digest}
        apply({'kind':'package_enable','family':'profile_pack','package':selection,'grants':[],'recorder':None},'enable')
        assert state()['profiles']['effective'] is None
        PHASE='configuration-and-runtime'
        apply({'kind':'configuration_stage','source':'candidate','candidate':'saved','source_sha256':sha(config)},'stage')
        apply({'kind':'configuration_select','candidate':'saved'},'select')
        assert state()['runtime']['running'] is None
        operation,submission=apply({'kind':'runtime_start'},'start')
        current=state();assert current['runtime']['ownership']=='owned'
        assert current['profiles']['effective']['packages'][0]['id']=='synthetic'
        gateway=current['runtime']['running']['gateway']['base_url'].removesuffix('/v1')
        assert http(gateway,'/v1/responses',env['MODEL_KEY'],{'model':'a','input':'synthetic'})[0]==200
        assert call('/operations',submission)[2]['data']['operation_id']==operation
        apply({'kind':'package_disable','family':'profile_pack','package':'synthetic'},'disable')
        current=state();assert current['runtime']['restart_required']
        assert current['profiles']['effective']['packages'][0]['id']=='synthetic'
        assert current['profiles']['store']['inventory']['activation']['packs']==[]
        assert len(current['profiles']['store']['inventory']['installed'])==1
        if args.managed:
            PHASE='continuation-control'
            manifest=current['runtime']['running_manifest']
            manifest=manifest['configuration']['gateway'] if 'execution_sha256' in manifest else manifest
            route=next(r.copy() for r in manifest['configuration']['routes'] if r['alias']=='m');route.pop('api_key_env')
            origin={'route':route,'realm':'synthetic','generation':'1'}
            code,_,raw=http(gateway,'/__continuation/sessions',env['CONTROL_KEY'],{'origin':origin});assert code==200
            session=json.loads(raw)
            command={'kind':'continuation_transition','session':session['id'],'revision':session['revision'],'transition_kind':'recover','portable_sha256':hashlib.sha256(b'synthetic portable state').hexdigest(),'decision_reference':'synthetic-recovery','pending_tools':False,'pending_approvals':False}
            _,recovered=apply(command,'recover')
            code,_,observed=call('/continuations/'+session['id']+'?target=gateway');assert code==200
            assert observed['data']['session']['epoch']==session['epoch']+1
            assert observed['data']['session']['revision']==session['revision']+1
            assert call('/operations',recovered)[0]==202
        PHASE='web'
        if args.web:
            code,headers,raw=http(base,'/');assert code==200 and b'<html' in raw and 'content-security-policy' in headers
            assert call('/session',token=token,method='POST',headers={'Origin':base})[0]==403
            code,headers,response=call('/session',token=env['READ_KEY'],method='POST',headers={'Origin':base});assert code==200
            cookie=headers['set-cookie'].split(';')[0]
            assert call('/state?target=gateway',token=None,headers={'Cookie':cookie})[0]==200
            assert call('/operations',submission,token=None,headers={'Cookie':cookie,'Origin':base})[0]==401
            assert call('/session',token=None,method='DELETE',headers={'Cookie':cookie,'Origin':base})[0]==200
        PHASE='team'
        if args.team:
            team_base=listeners['team'];keys=[]
            for subject,alias in [('alice','a'),('bob','b')]:
                permissions={'enabled':True,'routes':[alias],'management':[{'target':'gateway','action':'read_usage'}],'read_all_usage':False}
                apply({'kind':'team_subject_register','subject':subject,'permissions':permissions},'register-'+subject)
                issue=prepare({'kind':'team_credential_issue','subject':subject,'credential':subject+'-model','purpose':'model'},'issue-'+subject)
                assert call('/operations',issue)[0]==400
                assert call('/credential-delivery',issue,headers={'Origin':base})[0]==403
                issue_file=root/(subject+'-issue.json');output=root/(subject+'-key');write(issue_file,issue)
                result=run([args.cli,'--endpoint',base,'--token-env','MANAGEMENT_KEY','deliver-credential','--file',issue_file,'--output',output],env)
                key=output.read_text();keys.append(key);assert key.startswith('gwt1_model_') and key.encode() not in result
                assert call('/credential-delivery',issue)[2]['data']['credential'] is None
                assert call('/state?target=gateway',token=key)[0]==401
                code,_,raw=http(team_base,'/v1/models',key);assert code==200 and [m['id'] for m in json.loads(raw)['data']]==[alias]
                assert http(team_base,'/v1/responses',key,{'model':alias,'input':'synthetic','stream':True})[0]==200
                assert http(team_base,'/v1/responses',key,{'model':'b' if alias=='a' else 'a','input':'synthetic'})[0]==403
                assert http(team_base,'/v1/models',token)[0]==401
            for subject,key in zip(['alice','bob'],keys):
                code,_,raw=http(team_base,f'/team/v1/usage?from_ms=0&to_ms={int(time.time()*1000)+1000}',key)
                # Query window remains bounded to one year; no timestamp guessing for attribution.
                if code==400:
                    end=int(time.time()*1000)+1000;code,_,raw=http(team_base,f'/team/v1/usage?from_ms={end-86400000}&to_ms={end}',key)
                report=json.loads(raw);assert code==200 and len(report['requests'])==1
                assert report['requests'][0]['record']['admission']['subject']==subject
                if args.recorder:
                    for _ in range(100):
                        if report['requests'][0]['usage']['state']=='observed':
                            break
                        time.sleep(.02);report=json.loads(http(team_base,f'/team/v1/usage?from_ms={end-86400000}&to_ms={end}',key)[2])
                    assert report['requests'][0]['usage']['state']=='observed'
                    attempts=report['requests'][0]['usage']['attempts'];assert len(attempts)==1
                    assert attempts[0]['usage']['counters']['input_tokens']['value']==2 and attempts[0]['usage']['counters']['output_tokens']['value']==1
                else:
                    assert report['requests'][0]['usage']['state']=='unattributed'
                read_issue=prepare({'kind':'team_credential_issue','subject':subject,'credential':subject+'-read','purpose':'read_only'},'read-'+subject)
                code,_,issued=call('/credential-delivery',read_issue);assert code==200
                read_key=issued['data']['credential'];end=int(time.time()*1000)+1000
                code,_,scoped=call(f'/usage?target=gateway&from_ms={end-86400000}&to_ms={end}&timezone=UTC',token=read_key)
                assert code==200 and scoped['data']['scope']=='own_subject'
                assert all(row['record']['admission']['subject']==subject for row in scoped['data']['requests'])
            apply({'kind':'team_credential_revoke','credential':'alice-model'},'revoke-alice')
            assert http(team_base,'/v1/models',keys[0])[0]==401
            raw=call('/operations?target=gateway&after=0&limit=100')[2]
            assert all(key not in json.dumps(raw) for key in keys)
        else:
            assert not (root/'team').exists() and not (root/'requests').exists()
        PHASE='owned-shutdown'
        process.stdin.close();assert process.wait(timeout=25)==0
        try:
            http(gateway,'/v1/models',env['MODEL_KEY'])
            raise AssertionError('owned_process_survived')
        except URLError:
            pass
        assert not provider.failed
        print(json.dumps({'status':'passed','management':True,'web':bool(args.web),'team':args.team,'profile_inventory':'installed_selected_effective','provider_probe':False,'parent_shutdown':'passed'}))
    finally:
        if process.poll() is None:
            process.kill();process.wait(timeout=5)
        process.stdout.close();process.stderr.close()
        provider.shutdown();provider.server_close()


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['manager','child','gateway','cli','state-dir']:
        parser.add_argument('--'+name,required=True,type=Path)
    parser.add_argument('--web',type=Path)
    parser.add_argument('--team',action='store_true')
    parser.add_argument('--recorder',type=Path)
    parser.add_argument('--managed',action='store_true')
    parser.add_argument('--extension-manager',type=Path)
    parser.add_argument('--python',type=Path,default=Path(__import__('sys').executable))
    args=parser.parse_args()
    try:
        verify(args)
    except Exception as error:
        import traceback
        line=traceback.extract_tb(error.__traceback__)[-1].lineno
        parser.exit(1,f'management-smoke: failed during {PHASE} at fixture line {line} ({type(error).__name__}); inspect disposable fixture state\n')
