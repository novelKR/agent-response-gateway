"""Synthetic checked-Responses fixtures; no provider qualification or live calls."""
import json


def route(native_custom=False):
    value = '''api="responses"
auth="bearer"
capability_profile="synthetic-responses"
compatibility_policy="tools"
[compatibility_policies.tools]
version=1
[compatibility_policies.tools.tools]
custom_input="function_json"
namespaces="flatten"
grammar="registered_output_validation"
[capability_profiles.synthetic-responses]
version="1"
provider="mock"
upstream_model="synthetic-model"
api="responses"
context_window=32768
max_output_tokens=1024
tested_codex_version="0.154.0"
[capability_profiles.synthetic-responses.support]
instructions="native"
instruction_hierarchy="native"
function_tools="native"
tool_choice="native"
parallel_tool_control="native"
max_output_tokens="native"
reasoning_effort="native"
structured_output="native"
strict_structured_output="native"
strict_tool_arguments="native"
'''
    if native_custom:
        value = value.replace('custom_input="function_json"', 'custom_input="preserve"').replace('function_tools="native"', 'function_tools="native"\ncustom_tools="native"')
    return value


def fixture_view(body, state):
    # Shared scenario assertions use Messages-shaped fixtures only in this harness.
    from conformance import require
    require(body.get("store") is False, "checked Responses storage changed")
    tools = []
    for tool in body.get("tools", []):
        if tool["type"] == "custom":
            require(getattr(state,"native_custom",False), "unexpected native custom tool")
            require(tool.get("format") == {"type":"text"}, "native custom grammar was not removed")
            state.native_custom_names.add(tool["name"])
            description = json.loads(tool["description"])
            grammar = description["registered_grammar"]
            schema = {"type":"object","properties":{"input":{"type":"string","description":"The exact text must match this grammar: " + json.dumps(grammar)}}}
            tools.append({"name":tool["name"],"input_schema":schema})
        else:
            require(tool["type"] == "function", "checked tool declaration was not lowered")
            tools.append({"name":tool["name"],"description":tool.get("description",""),"input_schema":tool.get("parameters",{})})
    messages = []
    for item in body.get("input", []):
        kind = item.get("type")
        if kind in {"function_call","custom_tool_call"}:
            block = {"type":"tool_use","id":item["call_id"],"name":item["name"],"input":{"input":item["input"]} if kind == "custom_tool_call" else json.loads(item["arguments"])}
            if messages and messages[-1]["role"] == "assistant":
                messages[-1]["content"].append(block)
            else:
                messages.append({"role":"assistant","content":[block]})
        elif kind in {"function_call_output","custom_tool_call_output"}:
            messages.append({"role":"user","content":[{"type":"tool_result","tool_use_id":item["call_id"],"content":item["output"]}]})
        elif item.get("role") in {"user","assistant"}:
            content = item["content"]
            parts = [{"type":"text","text":content}] if isinstance(content,str) else [{"type":"text","text":p["text"]} for p in content]
            if item["role"] == "assistant" and messages and messages[-1]["role"] == "assistant":
                messages[-1]["content"].extend(parts)
            else:
                messages.append({"role":item["role"],"content":parts})
    return {**body,"tools":tools,"messages":messages}


def frames(blocks, number, state):
    from conformance import event
    response = {"id":f"resp_fixture_{number}","object":"response","model":"synthetic-model","created_at":0,"status":"completed","output":[],"usage":{"input_tokens":10,"output_tokens":3,"total_tokens":13}}
    values = [{"type":"response.created","response":{**response,"status":"in_progress","usage":None}}]
    for index, block in enumerate(blocks):
        tool = block["type"] == "tool_use"
        ident = f"item_fixture_{index}"
        if tool and block["name"] in getattr(state,"native_custom_names",set()):
            item = {"id":ident,"type":"custom_tool_call","call_id":block["id"],"name":block["name"],"input":block["input"]["input"],"status":"completed"}
            values.extend([
                {"type":"response.output_item.added","output_index":index,"item":{**item,"status":"in_progress","input":""}},
                {"type":"response.custom_tool_call_input.delta","output_index":index,"item_id":ident,"delta":item["input"]},
                {"type":"response.custom_tool_call_input.done","output_index":index,"item_id":ident,"input":item["input"]},
            ])
        elif tool:
            item = {"id":ident,"type":"function_call","call_id":block["id"],"name":block["name"],"arguments":json.dumps(block["input"],ensure_ascii=False),"status":"completed"}
            values.append({"type":"response.output_item.added","output_index":index,"item":{**item,"status":"in_progress","arguments":""}})
            args = item["arguments"]
            cut = len(args)//2
            for chunk in (args[:cut],args[cut:]):
                values.append({"type":"response.function_call_arguments.delta","output_index":index,"item_id":ident,"delta":chunk})
            values.append({"type":"response.function_call_arguments.done","output_index":index,"item_id":ident,"arguments":args})
        else:
            part = {"type":"output_text","text":block["text"],"annotations":[]}
            item = {"id":ident,"type":"message","role":"assistant","status":"completed","content":[part]}
            values.extend([
                {"type":"response.output_item.added","output_index":index,"item":{**item,"status":"in_progress","content":[]}},
                {"type":"response.content_part.added","output_index":index,"item_id":ident,"content_index":0,"part":{**part,"text":""}},
                {"type":"response.output_text.delta","output_index":index,"item_id":ident,"content_index":0,"delta":block["text"]},
                {"type":"response.output_text.done","output_index":index,"item_id":ident,"content_index":0,"text":block["text"]},
                {"type":"response.content_part.done","output_index":index,"item_id":ident,"content_index":0,"part":part},
            ])
        values.append({"type":"response.output_item.done","output_index":index,"item":item})
        response["output"].append(item)
    # The start must not share the mutable terminal output array.
    values[0]["response"]["output"] = []
    values.append({"type":"response.completed","response":response})
    return [event(v["type"],sequence_number=n,**{k:x for k,x in v.items() if k != "type"}) for n,v in enumerate(values)]
