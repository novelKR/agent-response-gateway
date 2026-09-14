import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createClient } from '../src/api.mjs';
import { validateTeamUsage, mergeTeamUsage } from '../src/usage-contract.mjs';
const vectors=JSON.parse(readFileSync(new URL('../../schemas/plugin-usage-vectors.json',import.meta.url),'utf8'));
const schemas=['gateway-usage-event/v1','gateway-usage-event/v2'];
function page(version) {
  return {schema:'gateway-team-http/v1',scope:'own_subject',from_ms:1,to_ms:2,window:'team_admitted_at',next_after:3,
    requests:[{record:{admission:{id:'synthetic',subject:'synthetic-subject'}},usage:{state:'observed',attempts:[structuredClone(vectors[`canonical_v${version}`].value)]}}],
    ...(version===2?{usage_event_schemas:[...schemas]}:{})};
}
const client = data => createClient('gateway',async()=>new Response(JSON.stringify({schema:'gateway-management-http/v1',data})));
test('team usage accepts both exact event versions and leaves raw records unchanged',async()=>{
  for(const version of [1,2]) {
    const value=page(version),before=JSON.stringify(value);
    assert.equal(validateTeamUsage(value),value);
    assert.equal(JSON.stringify(value),before);
    assert.deepEqual((await client(value).usage(1,2,'UTC')).data,value);
  }
});
test('missing forged and inappropriate event schema markers are rejected before display',async()=>{
  const bad=[];
  for(const marker of [undefined,[],[...schemas].reverse(),[schemas[1]],['unknown/v1']]) {
    const value=page(2);if(marker===undefined)delete value.usage_event_schemas;else value.usage_event_schemas=marker;bad.push(value);
  }
  const unnecessary=page(1);unnecessary.usage_event_schemas=[...schemas];bad.push(unnecessary);
  const unknown=page(2);unknown.requests[0].usage.attempts[0].schema='gateway-usage-event/v3';bad.push(unknown);
  for(const value of bad)await assert.rejects(client(value).usage(1,2,'UTC'),{code:'unsupported_contract'});
});
test('malformed provider event shape and invalid raw numeric evidence fail closed',()=>{
  for(const change of [e=>{e.interpretation.package_sha256='wrong';},e=>{e.profile='responses_v1';},
    e=>{e.usage.counters.input_tokens={source:'reported',value:-1};},e=>{delete e.interpretation;},
    e=>{e.usage.reported.secret={source:'reported',value:1};},e=>{e.usage.counters.input_tokens={source:'not_reported',value:0};}]) {
    const value=page(2);change(value.requests[0].usage.attempts[0]);assert.throws(()=>validateTeamUsage(value));
  }
  for(const attempts of [[],Array.from({length:17},()=>page(1).requests[0].usage.attempts[0])]) {
    const value=page(1);value.requests[0].usage.attempts=attempts;assert.throws(()=>validateTeamUsage(value));
  }
});
test('mixed pagination preserves explicit versions whichever page first carries v2',()=>{
  for(const versions of [[1,2],[2,1],[2,2]]) {
    const first=page(versions[0]),next=page(versions[1]),before=[JSON.stringify(first),JSON.stringify(next)];
    const merged=mergeTeamUsage(first,next);
    assert.deepEqual(merged.usage_event_schemas,schemas);
    assert.deepEqual(merged.requests,[...first.requests,...next.requests]);
    assert.equal(merged.requests[0],first.requests[0]);assert.equal(merged.requests[1],next.requests[0]);
    assert.deepEqual([JSON.stringify(first),JSON.stringify(next)],before);
  }
  const legacy=mergeTeamUsage(page(1),page(1));assert.equal(Object.hasOwn(legacy,'usage_event_schemas'),false);
  const other=page(2);other.scope='all_team_subjects';assert.throws(()=>mergeTeamUsage(page(1),other));
});

test('large event integers retain original JSON numeric evidence and reject quoted imitations',async()=>{
  const value=page(2),event=value.requests[0].usage.attempts[0];
  for(const name of Object.keys(event.usage.counters))event.usage.counters[name]={source:'not_reported',value:null};
  event.usage.counters.input_tokens={source:'reported',value:'__large__'};
  const body=JSON.stringify({schema:'gateway-management-http/v1',data:value}).replace('"__large__"','9007199254740993');
  const result=await createClient('gateway',async()=>new Response(body)).usage(1,2,'UTC');
  assert.equal(result.data.requests[0].usage.attempts[0].usage.counters.input_tokens.value,'9007199254740993');
  validateTeamUsage(result.data);
  event.usage.counters.input_tokens.value='9007199254740993';
  await assert.rejects(client(value).usage(1,2,'UTC'),{code:'unsupported_contract'});
});

test('event counters reject fractional exponent and negative-zero integer spellings',async()=>{
  const value=page(2);value.requests[0].usage.attempts[0].usage.counters.input_tokens={source:'reported',value:'__number__'};
  for(const source of ['1.0','1e0','-0','18446744073709551616']) {
    const body=JSON.stringify({schema:'gateway-management-http/v1',data:value}).replace('"__number__"',source);
    await assert.rejects(createClient('gateway',async()=>new Response(body)).usage(1,2,'UTC'),{code:'unsupported_contract'});
  }
});
