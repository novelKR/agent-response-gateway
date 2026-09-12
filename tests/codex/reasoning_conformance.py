#!/usr/bin/env python3
"""Full tool and cancellation acceptance for explicit managed reasoning contracts."""
import argparse
import json
from pathlib import Path
from unittest.mock import patch
import conformance as base
import reasoning_continuity as claude
import reasoning_chat_continuity as chat

CONTRACTS=('claude_adaptive','claude_manual','deep_seek','open_router')
SCENARIOS=('text','function_tool','namespace_tool','custom_patch','approval_denial','parallel_tools','multi_tool_turns','mixed_tool_text','text_followup','reasoning_only','grammar_failure','output_controls','cancellation','cancellation_heartbeat','transport_failure')

def route(contract):
    version=json.loads(base.runtime.LOCK.read_text())['version']
    return claude.route(version,contract=='claude_manual') if contract.startswith('claude_') else chat.route(version,contract=='open_router')


def view(body,state):
    return body if state.api=='messages' else base.chat_fixture_view(body,state.managed_contract)


def frames(state,blocks):
    number=state.requests
    if state.api=='messages':
        native=[{'type':'thinking','thinking':'SYNTHETIC_PUBLIC_REASONING','signature':f'synthetic_signature_{number}'}, {'type':'redacted_thinking','data':f'synthetic_redacted_{number}'}]+blocks
        if any(b['type']=='tool_use' for b in blocks):native.append({'type':'thinking','thinking':'SYNTHETIC_AFTER_TOOL_REASONING','signature':f'synthetic_signature_after_{number}'})
        state.native_history.append(native)
        return claude.frames(native,number)
    native=chat.assistant(number,state.managed_contract=='open_router')
    texts=[b['text'] for b in blocks if b['type']=='text'];native['content']=''.join(texts) if texts else None
    tools=[{'id':b['id'],'type':'function','function':{'name':b['name'],'arguments':json.dumps(b['input'])}} for b in blocks if b['type']=='tool_use']
    if tools:native['tool_calls']=tools
    state.native_history.append(native)
    output=[]
    for frame in chat.frames(native,number):
        if frame==b'data: [DONE]\n\n':output.append(frame);continue
        value=json.loads(frame.decode().removeprefix('data: '));value['id']=f'chat_fixture_{number}';value['created']=0
        output.append(('data: '+json.dumps(value)+'\n\n').encode())
    return output


def respond(state,body):
    contract=state.managed_contract
    if not hasattr(state,'native_history'):state.native_history=[]
    if state.api=='messages':
        expected={'type':'adaptive','display':'summarized'} if contract=='claude_adaptive' else {'type':'enabled','display':'summarized','budget_tokens':4096 if state.name=='output_controls' else 2048}
        base.require(body.get('thinking')==expected,'managed Claude controls differ')
        if contract=='claude_adaptive':base.require(body.get('output_config',{}).get('effort')==('high' if state.name=='output_controls' else 'medium'),'adaptive effort differs')
        if state.name=='output_controls':base.require(body.get('output_config',{}).get('format')=={'type':'json_schema','schema':base.CONTROL_SCHEMA},'managed output schema differs')
        originals=[b for m in body['messages'] if m['role']=='assistant' for b in m['content']]
        for old in state.native_history:
            base.require(sum(originals[i:i+len(old)]==old for i in range(len(originals)))==1,'Messages native state lost or reordered')
    else:
        if contract=='deep_seek':
            base.require(body.get('thinking')=={'type':'enabled'} and body.get('reasoning_effort')=='high','DeepSeek controls differ')
            base.require('parallel_tool_calls' not in body,'unsupported DeepSeek parallel field')
        else:
            base.require(body.get('provider')=={'only':['synthetic/exact'],'allow_fallbacks':False,'require_parameters':True},'OpenRouter routing differs')
            base.require(body.get('reasoning')=={'enabled':True,'exclude':False,'effort':'high'},'OpenRouter reasoning differs')
            if state.name=='output_controls':
                output=body.get('response_format',{}).get('json_schema',{})
                base.require(output.get('schema')==base.CONTROL_SCHEMA and output.get('strict') is True,'OpenRouter schema differs')
        originals=[m for m in body['messages'] if m['role']=='assistant']
        for old in state.native_history:base.require(sum(m==old for m in originals)==1,'Chat native assistant state lost')
    if state.name=='namespace_tool':
        candidates=[]
        for tool in (body.get('tools',[]) if state.api=='messages' else [t['function'] for t in body.get('tools',[])]):
            try:description=json.loads(tool.get('description',''))
            except (ValueError,TypeError):continue
            if isinstance(description,dict) and description.get('namespace') in {'fixture','second'}:candidates.append(tool['name'])
        base.require(len(candidates)==2 and len(set(candidates))==2,'namespace aliases collided')
    if state.name in {'multi_tool_turns','reasoning_only'}:
        state.requests+=1
        if state.name=='reasoning_only':base.require(state.requests==1,'reasoning-only retried');return frames(state,[])
        base.require(state.requests<=3,'tool-turn retry')
        converted=view(body,state)
        results=[b for m in converted['messages'] for b in m['content'] if b['type']=='tool_result']
        base.require({r['tool_use_id'] for r in results}=={f'call_round_{i}' for i in range(1,state.requests)},'tool turn results lost')
        if state.requests==3:state.result_seen=True;return frames(state,[{'type':'text','text':'Synthetic complete.'}])
        tools=[t for t in converted['tools'] if t['name']=='gateway_echo'];base.require(len(tools)==1,'tool declaration missing')
        return frames(state,[{'type':'tool_use','id':f'call_round_{state.requests}','name':tools[0]['name'],'input':{'text':'synthetic'}}])
    with patch.object(base,'converted_frames',frames):return base.converted_response(state,body)


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--gateway-bin',type=Path,default=base.ROOT/'target/debug/agent-response-gateway');p.add_argument('--codex-bundle',type=Path,default=base.runtime.BUNDLE)
    p.add_argument('--contract',choices=CONTRACTS,action='append');p.add_argument('--scenario',choices=SCENARIOS,action='append');args=p.parse_args()
    binary=base.runtime.verify_bundle(args.codex_bundle,json.loads(base.runtime.LOCK.read_text()));failed=0
    for contract in args.contract or CONTRACTS:
        for scenario in args.scenario or SCENARIOS:
            try:result=base.run_scenario(scenario,binary,args.gateway_bin.resolve(),'messages' if contract.startswith('claude_') else 'chat_completions',contract)
            except Exception as e:
                failed+=1;result={'status':'failed','contract':contract,'scenario':scenario,'error_class':type(e).__name__}
                if isinstance(e,AssertionError):result['check']=str(e)
            print(json.dumps(result,sort_keys=True),flush=True)
    raise SystemExit(1 if failed else 0)
if __name__=='__main__':main()
