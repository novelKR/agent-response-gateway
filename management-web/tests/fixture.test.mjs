import test from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { fileURLToPath } from 'node:url';
import { createClient } from '../src/api.mjs';
test('built Web uses real HTTP read sessions and durable synthetic journal queries', {skip:!process.env.WEB_FIXTURE_BIN,timeout:30000}, async () => {
  const assets=fileURLToPath(new URL('../../.local/management-web/dist/',import.meta.url));
  const child=spawn(process.env.WEB_FIXTURE_BIN,['--assets',assets],{stdio:['pipe','pipe','pipe']});
  let stderr='';child.stderr.on('data',chunk=>{stderr+=chunk;});
  const exited=once(child,'exit');
  try {
    const url=await new Promise((resolve,reject)=>{let line='';child.stdout.on('data',chunk=>{line+=chunk;if(line.includes('\n'))resolve(line.split('\n')[0]);});child.once('error',reject);child.once('exit',()=>reject(new Error('Fixture exited: '+stderr)));});
    const origin=new URL(url).origin;
    const html=await fetch(url);assert.equal(html.status,200);assert.match(html.headers.get('content-security-policy'),/frame-ancestors 'none'/);assert.doesNotMatch(await html.text(),/synthetic-browser-read-key|mock/);
    assert.equal((await fetch(url,{headers:{Origin:'https://untrusted.example'}})).status,403);
    assert.equal((await fetch(origin+'/dashboard/.git/config')).status,404);
    let cookie='';const methods=[];
    const client=createClient('gateway',async(path,options)=>{methods.push(options.method+' '+path.split('?')[0]);const response=await fetch(origin+path,{...options,headers:{...options.headers,Origin:origin,...(cookie?{Cookie:cookie}:{})}});if(response.headers.has('set-cookie'))cookie=response.headers.get('set-cookie').split(';')[0];return response;});
    await assert.rejects(client.capabilities(),{status:401});
    await client.login('synthetic-browser-read-key-01234567890123456789');
    const caps=await client.capabilities();assert.deepEqual(caps.data.allowed_operations,['read_state','read_usage','read_operations']);
    assert.equal((await client.state()).data.modules.length,3);
    const usage=(await client.usage(1,2,'UTC')).data;assert.equal(usage.groups[1].token_sums.input_tokens,'9007199254740993123');
    const operations=(await client.operations()).data.items;assert.equal(operations.length,1);assert.equal(operations[0].observed_state,'uncertain');
    assert.equal((await client.operation(operations[0].operation.id)).data.operation.events.length,3);
    const blocked=await fetch(origin+'/management/v1/preflight',{method:'POST',headers:{Cookie:cookie,Origin:origin,'Content-Type':'application/json'},body:JSON.stringify({schema:'gateway-management-http/v1',target:'gateway',idempotency_key:'browser-denied',command:{kind:'runtime_start'}})});assert.equal(blocked.status,401);
    const cross=await fetch(origin+'/management/v1/state?target=gateway',{headers:{Cookie:cookie,Origin:'https://untrusted.example'}});assert.equal(cross.status,403);
    await client.logout();await assert.rejects(client.state(),{status:401});
    assert.ok(methods.filter(m=>!m.startsWith('GET')).every(m=>m.endsWith('/session')));
  } finally {child.stdin.end();const timer=setTimeout(()=>child.kill('SIGKILL'),5000);await exited;clearTimeout(timer);}
});
