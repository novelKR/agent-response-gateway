import test from 'node:test';
import assert from 'node:assert/strict';
import {createClient} from '../src/api.mjs';
import {baseTime,scenarios,syntheticTransport} from '../devdemo/scenarios.mjs';
import {productionBoundary} from '../scripts/production-boundary.mjs';

test('all synthetic scenarios use the real client and deterministic response contracts',async()=>{
  for(const scenario of scenarios) {
    const client=createClient('gateway',syntheticTransport(scenario.id));
    const caps=await client.capabilities();assert.equal(caps.observed_at_ms,baseTime);
    if(!['offline','expired'].includes(scenario.id))assert.deepEqual(await client.capabilities(),caps);
    if(!['loading','forbidden'].includes(scenario.id)&&caps.data.allowed_operations.includes('read_state'))assert.equal((await client.state()).data.schema,'gateway-management-state/v1');
    client.dispose();
  }
  const client=createClient('gateway',syntheticTransport('usage'));
  const groups=(await client.usage(1,2,'UTC')).data.groups;
  assert.equal(groups[0].token_sums.output_tokens,null);assert.equal(groups[1].token_sums.output_tokens,0);
  assert.equal(groups[1].token_sums.input_tokens,'9007199254740993123');client.dispose();
});
test('scope, pagination, audit detail and reset preserve their meanings',async()=>{
  const team=createClient('gateway',syntheticTransport('team'));
  assert.deepEqual((await team.capabilities()).data.allowed_operations,['read_usage']);
  await assert.rejects(team.state(),{status:403});
  assert.equal((await team.usage(1,2,'UTC')).data.requests[0].record.admission.subject,'synthetic-alice');
  const client=createClient('gateway',syntheticTransport('long'));
  const first=(await client.operations()).data.items,second=(await client.operations(first.at(-1).cursor)).data.items;
  assert.equal(first.length,20);assert.equal(second[0].cursor,21);
  assert.equal((await client.operation(first[2].operation.id)).data.observed_state,'uncertain');
  await client.logout();await assert.rejects(client.capabilities(),{status:401});
  const reset=createClient('gateway',syntheticTransport('long'));assert.equal((await reset.operations()).data.items[0].cursor,1);
  for(const item of [team,client,reset])item.dispose();
});
test('loading is cancellable and explicit failures never fall back to data',async()=>{
  const pending=createClient('gateway',syntheticTransport('loading')),request=pending.state();pending.dispose();await assert.rejects(request);
  for(const [scenario,code] of [['offline','connection_unavailable'],['expired','unauthorized']]) {
    const client=createClient('gateway',syntheticTransport(scenario));await client.capabilities();
    await assert.rejects(client.capabilities(),{code});client.dispose();
  }
});
test('production build rejects imported development modules and fixture strings',()=>{
  const plugin=productionBoundary('/unused/','/unused/');
  assert.throws(()=>plugin.moduleParsed({id:'/source/devdemo/unused.mjs'}),/Development/);
  for(const chunk of [{type:'chunk',modules:{'/source/devdemo/scenarios.mjs':{}},code:''},{type:'chunk',modules:{},code:'gateway-devdemo/v1'},{type:'asset',source:'/@vite/client'}]) {
    assert.throws(()=>plugin.generateBundle({}, {chunk}),/Development/);
  }
});
