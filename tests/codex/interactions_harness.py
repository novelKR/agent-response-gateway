"""Synthetic Interactions provider and host setup shared with the real Codex suite."""
import json
import os
import secrets
import subprocess
import urllib.request
import urllib.error


def control(base_url, token, path, body=None):
    opener=urllib.request.build_opener(urllib.request.ProxyHandler({}))
    request=urllib.request.Request(base_url.removesuffix('/v1')+path,
        data=None if body is None else json.dumps(body).encode(),
        headers={'Authorization':'Bearer '+token,'Content-Type':'application/json'})
    with opener.open(request,timeout=10) as response:return json.load(response)


def setup(root, binary, config, env):
    directory=(root/'continuation-store').resolve();directory.mkdir(mode=0o700)
    result=subprocess.run([str(binary),'init-continuation','--directory',str(directory)],env=env,capture_output=True,text=True,check=True)
    identity=json.loads(result.stdout)['store_id'];token=secrets.token_urlsafe(32);key=secrets.token_hex(32)
    env.update(ARG_CONTINUATION_CONTROL=token,ARG_CONTINUATION_KEY=key)
    config.write_text(config.read_text()+f'\n[continuation]\ndirectory={json.dumps(str(directory))}\nstore_id="{identity}"\nrealm="synthetic"\ngeneration="1"\nkey_id="test-key"\nkey_env="ARG_CONTINUATION_KEY"\ncontrol_token_env="ARG_CONTINUATION_CONTROL"\nmax_store_bytes=1073741824\n')
    return token


def route(version):
    return f'''api="gemini_interactions"
auth="google_api_key"
capability_profile="synthetic-interactions"
[capability_profiles.synthetic-interactions]
version="1"
provider="mock"
upstream_model="synthetic-model"
api="gemini_interactions"
context_window=32768
max_output_tokens=1024
tested_codex_version="{version}"
[capability_profiles.synthetic-interactions.support]
instructions="native"
instruction_hierarchy="bridged_gemini_instruction_envelope"
function_tools="native"
custom_tools="bridged_custom_tool_json"
custom_grammar="bridged_codex_patch_grammar"
namespaced_tools="bridged_tool_namespace"
tool_choice="native"
parallel_tool_control="native"
max_output_tokens="native"
reasoning_effort="native"
structured_output="native"
strict_structured_output="native"
'''


def create_session(base_url,token,manifest):
    route={k:v for k,v in manifest['configuration']['routes'][0].items() if k!='api_key_env'}
    return control(base_url,token,'/__continuation/sessions',{'origin':{'route':route,'realm':'synthetic','generation':'1'}})


def event(kind,**values):
    return ('event: '+kind+'\ndata: '+json.dumps({'event_type':kind,**values})+'\n\n').encode()


def frames(steps,number):
    status='requires_action' if any(s['type']=='function_call' for s in steps) else 'completed'
    meta={'id':f'provider_{number}','object':'interaction','model':'synthetic-model'}
    out=[event('interaction.created',interaction={**meta,'status':'in_progress'})]
    for i,step in enumerate(steps):
        kind=step['type'];start={'type':kind}
        if kind=='function_call':start.update(id=step['id'],name=step['name'])
        out.append(event('step.start',index=i,step=start))
        if kind=='thought':
            value=step['signature'];delta='thought_signature';field='signature'
        elif kind=='function_call':
            value=json.dumps(step['arguments']);delta='arguments_delta';field='arguments'
        else:
            value=step['content'][0]['text'];delta='text';field='text'
        cut=len(value)//2
        for fragment in (value[:cut],value[cut:]):out.append(event('step.delta',index=i,delta={'type':delta,field:fragment}))
        out.append(event('step.stop',index=i))
    out.append(event('interaction.completed',interaction={**meta,'status':status,'usage':{'total_input_tokens':10,'total_output_tokens':3,'total_thought_tokens':2,'total_tokens':15,'total_cached_tokens':0}}))
    out.append(b'event: done\ndata: [DONE]\n\n');return out


def respond(state,body):
    import conformance as base
    state.requests+=1
    base.require(state.requests<=(3 if state.name=='multi_tool_turns' else 2),'unexpected Interactions retry')
    base.require(body.get('store') is False and body.get('background') is False and body.get('model')=='synthetic-model','Interactions wire policy differs')
    base.require(body.get('stream') is True,'Interactions streaming required')
    if state.name=='output_controls':
        base.require(body['generation_config'].get('thinking_level')=='high','thinking level changed')
        base.require(body.get('response_format',{}).get('schema')==base.CONTROL_SCHEMA,'output schema changed')
    input_steps=body['input']
    if state.name=='multi_tool_turns':
        if state.requests>1:
            expected={'call_round_1'} if state.requests==2 else {'call_round_1','call_round_2'}
            base.require({s['call_id'] for s in input_steps if s.get('type')=='function_result'}==expected,'multiple-turn results lost')
            base.require(any(s.get('type')=='thought' for s in input_steps),'multiple-turn thought lost')
            state.result_seen=True
        blocks=([{'type':'function_call','id':f'call_round_{state.requests}','name':'gateway_echo','arguments':{'text':'synthetic'}}]
                if state.requests<3 else [{'type':'model_output','content':[{'type':'text','text':'Synthetic complete.'}]}])
        return frames([{'type':'thought','signature':f'synthetic-signature-{state.requests}'}]+blocks,state.requests)

    if getattr(state,'editing',False) and not getattr(state,'normalization',False) and state.requests==2 and state.name in {'custom_patch','approval_denial'}:
        calls=[s for s in input_steps if s.get('type')=='function_call' and s.get('id')=='call_fixture']
        base.require(len(calls)==1 and calls[0]['name'].startswith('arg_edit_') and calls[0]['arguments']==__import__('editing_fixture').edit_input(getattr(state,'operations',False),file_conflict=getattr(state,'file_conflict',False)),'structured native history changed')
    if state.requests==2:
        base.require(any(s.get('type')=='thought' and s.get('signature')=='synthetic-signature-1' for s in input_steps),'provider thought not replayed')
        if state.name!='text_followup':
            results=[s for s in input_steps if s.get('type')=='function_result']
            expected={'call_fixture_a','call_fixture_b'} if state.name=='parallel_tools' else {'call_fixture'}
            base.require({s['call_id'] for s in results}==expected,'function results lost')
        state.result_seen=True
        blocks=[{'type':'model_output','content':[{'type':'text','text':'Synthetic complete.'}]}]
    elif state.name in {'function_tool','namespace_tool','parallel_tools','mixed_tool_text'}:
        if state.name=='namespace_tool':
            candidates=[]
            for t in body['tools']:
                try:d=json.loads(t.get('description',''))
                except (ValueError,TypeError):continue
                if isinstance(d,dict) and d.get('namespace')=='fixture':candidates.append(t)
        else:candidates=[t for t in body['tools'] if t['name']=='gateway_echo']
        base.require(len(candidates)==1,'dynamic tool missing')
        ids=['call_fixture_a','call_fixture_b'] if state.name=='parallel_tools' else ['call_fixture']
        blocks=[{'type':'function_call','id':ident,'name':candidates[0]['name'],'arguments':{'text':'synthetic'}} for ident in ids]
        if state.name=='mixed_tool_text':blocks.append({'type':'model_output','content':[{'type':'text','text':'Synthetic after tool.'}]})
    elif state.name in {'custom_patch','approval_denial','grammar_failure'}:
        candidates=[t for t in body['tools'] if list(t['parameters'].get('properties',{}))==['input']]
        base.require(len(candidates)==1,'custom wrapper missing')
        patch='*** Begin Patch\n*** Add File: fixture.txt\n+synthetic-content\n*** End Patch'
        if state.name=='grammar_failure':patch='*** Begin Patch\n*** End Patch'
        if getattr(state,'normalization',False) and state.name in {'custom_patch','approval_denial'}:patch=patch.replace('*** Begin Patch\n','*** Begin Patch ***\n')+' ***'
        blocks=[{'type':'function_call','id':'call_fixture','name':candidates[0]['name'],'arguments':{'input':patch}}]
    else:blocks=[{'type':'model_output','content':[{'type':'text','text':base.CONTROL_TEXT if state.name=='output_controls' else 'Synthetic complete.'}]}]
    if getattr(state,'editing',False) and not getattr(state,'normalization',False) and state.name in {'custom_patch','approval_denial','grammar_failure'} and state.requests==1:
        from editing_fixture import block
        edit=block([{'name':t['name'],'input_schema':t['parameters']} for t in body['tools']], invalid=state.name=='grammar_failure',operations=getattr(state,'operations',False),file_conflict=getattr(state,'file_conflict',False))
        blocks=[{'type':'function_call','id':edit['id'],'name':edit['name'],'arguments':edit['input']}]
    return frames([{'type':'thought','signature':f'synthetic-signature-{state.requests}'}]+blocks,state.requests)
