import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createClient, inventoryRows } from '../src/api.mjs';
import { messages } from '../src/i18n.mjs';
const schema = 'gateway-management-http/v1';
const reply = (data, status = 200) => new Response(JSON.stringify({ schema, observed_at_ms: 1, data }), { status });
test('client emits only same-origin bounded reads and scoped session requests', async () => {
  const calls = [], token = 'synthetic-browser-read-key-01234567890123456789';
  const client = createClient('gateway', async (path, options) => { calls.push([path,options]);return reply(path.includes('/state?') ? {schema:'gateway-management-state/v1',modules:[]} : {}); });
  await client.login(token);await client.capabilities();await client.state();await client.usage(1,2,'Asia/Seoul');await client.operations();await client.operation('operation-1');await client.logout();
  assert.deepEqual(calls.map(([,o])=>o.method), ['POST','GET','GET','GET','GET','GET','DELETE']);
  for (const [path,options] of calls) {
    assert.match(path,/^\/management\/v1\//);assert.equal(options.credentials,'same-origin');assert.equal(options.redirect,'error');assert.equal(options.cache,'no-store');assert.equal(options.body,undefined);
    assert.equal(options.headers.Authorization, options.method === 'POST' ? `Bearer ${token}` : undefined);
  }
  assert.throws(()=>client.operation('..'));assert.throws(()=>client.operation('a/b'));
  assert.throws(()=>createClient('https://remote'));
  const source = readFileSync(new URL('../src/App.vue',import.meta.url),'utf8');
  assert.doesNotMatch(source,/v-html|innerHTML|sessionStorage|localStorage\.setItem\((?!'gateway-view-language')/);
  assert.doesNotMatch(readFileSync(new URL('../src/api.mjs',import.meta.url),'utf8'),/'(?:PUT|PATCH)'|preflight|reconcile|submit|WebSocket/);
});
test('unknown contracts, duplicate modules and authentication errors remain explicit', async () => {
  for (const body of ['null','{}','{"schema":"gateway-management-http/v2"}']) {
    await assert.rejects(createClient('gateway',async()=>new Response(body)).capabilities(),{code:'unsupported_contract'});
  }
  const module={id:'native',contract:'gateway-extension-status/v1',observation:{state:'unsupported'}};
  await assert.rejects(createClient('gateway',async()=>reply({schema:'gateway-management-state/v1',modules:[module,module]})).state(),{code:'invalid_response'});
  await assert.rejects(createClient('gateway',async()=>new Response(JSON.stringify({schema,error:{code:'unauthorized'}}),{status:401})).capabilities(),{code:'unauthorized',status:401});
});
test('large usage counters preserve exact digits; null and zero stay distinct', async () => {
  const value=await createClient('gateway',async()=>new Response(`{"schema":"${schema}","data":{"large":9007199254740993123,"missing":null,"zero":0}}`)).usage(1,2,'UTC');
  assert.equal(value.data.large,'9007199254740993123');assert.equal(value.data.missing,null);assert.equal(value.data.zero,0);
});
test('unbounded responses are canceled and malformed UTF-8 is rejected', async () => {
  await assert.rejects(createClient('gateway',async()=>new Response('x'.repeat(2*1024*1024+1))).capabilities(),{code:'response_too_large'});
  await assert.rejects(createClient('gateway',async()=>new Response(new Uint8Array([0xff]))).capabilities(),{code:'connection_unavailable'});
});
test('installed, selected and effective versions remain independent', () => {
  const item = (version,digest) => ({id:'codec',version,package_sha256:digest,verified:true});
  const a=item('1.0.0','a'), b=item('2.0.0','b');
  const data={store:{inventory:{installed:[a,b],activation:{extensions:[{...b,grants:['network']} ]}}},effective:{packages:[a]}};
  const module={observation:{state:'observed',data}};
  assert.deepEqual(inventoryRows(module).map(r=>[r.selected,r.effective]),[[false,true],[true,false]]);
  data.effective=null;assert.deepEqual(inventoryRows(module).map(r=>r.effective),[null,null]);
  module.observation.state='unobserved';assert.deepEqual(inventoryRows(module),[]);
});
test('Korean and English have equal nonempty messages', () => {
  assert.deepEqual(Object.keys(messages.en).sort(),Object.keys(messages.ko).sort());
  for(const values of Object.values(messages))for(const value of Object.values(values))assert.ok(value.length);
});
