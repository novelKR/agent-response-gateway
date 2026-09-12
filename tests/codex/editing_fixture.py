"""Synthetic structured-edit admission and real-Codex execution fixture."""
import argparse
import json
from pathlib import Path
import conformance as c


def configure(raw, code_mode=False, descriptor=None):
    raw = raw.replace('capability_profile=', 'editing_policy="synthetic-edit"\ncapability_profile=', 1)
    configured = raw + '''
[editing_policies.synthetic-edit]
version=1
client_contract="codex-direct-custom/v1"
representation="context-lines/v1"
patch_dialect="codex-patch/1"
normalization="none"
'''
    if code_mode:
        descriptor=descriptor or json.loads(Path(__file__).with_name('editing-contract-lock.json').read_text())['code_mode_descriptors']['dynamic_echo']
        return configured.replace('codex-direct-custom/v1','codex-code-mode/v1') + '\nclient_descriptor_sha256='+json.dumps(descriptor)+'\n'
    return configured


def edit_input(operations=False, invalid=False, file_conflict=False):
    edit={"path":"fixture.txt", "before_context":[], "old_lines":["missing-context" if file_conflict else "synthetic-old"],
          "new_lines":["synthetic-old" if invalid else "synthetic-content"], "after_context":[]}
    if not operations: return edit
    result={"operations":[{"operation":"create","path":"created.txt","lines":["synthetic-created"]},
                          {"operation":"delete","path":"deleted.txt"},
                          {"operation":"move","source":"source.txt","destination":"moved.txt","context":["synthetic-moved"]},
                          {"operation":"update","edit":edit}]}
    if invalid: result["operations"].append({"operation":"delete","path":"moved.txt"})
    return result


def block(tools, invalid=False, operations=False, file_conflict=False):
    key="operations" if operations else "before_context"
    choices = [t for t in tools if key in t.get("input_schema", {}).get("properties", {})]
    c.require(len(choices) == 1, "synthetic edit missing")
    return {"type":"tool_use", "id":"call_fixture", "name":choices[0]["name"], "input":edit_input(operations,invalid,file_conflict)}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--gateway-bin',type=Path,required=True)
    parser.add_argument('--runtime-dir',type=Path,default=c.runtime.BUNDLE)
    parser.add_argument('--codec-bin',type=Path)
    parser.add_argument('--profile-packs',action='store_true')
    parser.add_argument('--operations',action='store_true')
    parser.add_argument('--code-mode',action='store_true')
    parser.add_argument('--normalize-envelope',action='store_true')
    args=parser.parse_args()
    if args.code_mode and args.normalize_envelope: parser.error('Envelope normalization is direct-patch only')
    binary=c.runtime.verify_bundle(args.runtime_dir.resolve(),json.loads(c.runtime.LOCK.read_text()))
    for api in ['messages','chat_completions','responses_checked','gemini_interactions']:
        for scenario in (['custom_patch','approval_denial','grammar_failure','contract_failure','cancellation','cancellation_heartbeat','transport_failure'] if args.code_mode else ['custom_patch','approval_denial','grammar_failure']):
            print(json.dumps(c.run_scenario(scenario,binary,args.gateway_bin.resolve(),api,editing=True,codec_binary=args.codec_bin,profile_packs=args.profile_packs,code_mode=args.code_mode,normalization=args.normalize_envelope,operations=args.operations)),flush=True)

    for contract in ['claude_adaptive','claude_manual','deep_seek','open_router']:
        api='messages' if contract.startswith('claude_') else 'chat_completions'
        for scenario in (['custom_patch','approval_denial','grammar_failure','contract_failure','cancellation','cancellation_heartbeat','transport_failure'] if args.code_mode else ['custom_patch','approval_denial','grammar_failure']):
            print(json.dumps(c.run_scenario(scenario,binary,args.gateway_bin.resolve(),api,managed_contract=contract,editing=True,codec_binary=args.codec_bin,profile_packs=args.profile_packs,code_mode=args.code_mode,normalization=args.normalize_envelope,operations=args.operations)),flush=True)


    if args.operations:
        for api in ['messages','chat_completions','responses_checked','gemini_interactions']:
            print(json.dumps(c.run_scenario('custom_patch',binary,args.gateway_bin.resolve(),api,editing=True,codec_binary=args.codec_bin,profile_packs=args.profile_packs,code_mode=args.code_mode,operations=True,file_conflict=True)),flush=True)


if __name__=='__main__': main()
