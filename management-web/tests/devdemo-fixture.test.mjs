import test from 'node:test';
import assert from 'node:assert/strict';
import {createServer,globalAgent} from 'node:http';
import {spawn} from 'node:child_process';
import {once} from 'node:events';
import {fileURLToPath} from 'node:url';
import {mkdirSync,readdirSync} from 'node:fs';
import {createClient} from '../src/api.mjs';
import {createFixtureProxy} from '../scripts/fixture-proxy.mjs';
import {startFixtureDemo} from '../scripts/fixture-demo.mjs';
import {loadDemoConfig,fixtureReadCredential} from '../devdemo/config.mjs';
const real=Boolean(process.env.WEB_FIXTURE_BIN);
async function listen(server){server.listen(0,'127.0.0.1');await once(server,'listening');return 'http://127.0.0.1:'+server.address().port;}
async function close(server){server.closeAllConnections();await new Promise(resolve=>server.close(resolve));}
test('fixture proxy keeps exact requests but strips unrelated cookies and avoids the global agent',async()=>{
  const seen=[];
  const backend=createServer((request,response)=>{
    seen.push({method:request.method,url:request.url,headers:request.headers});
    if(request.url==='/management/v1/session')response.setHeader('Set-Cookie','fixture_read=synthetic-session; HttpOnly; SameSite=Strict; Path=/management/v1');
    response.setHeader('Content-Type','application/json');response.end('{"schema":"gateway-management-http/v1","data":{}}');
  });
  const origin=await listen(backend),proxy=createFixtureProxy({origin}),frontend=createServer(proxy),base=await listen(frontend);
  const previous=globalAgent.createConnection;globalAgent.createConnection=()=>{throw Error('Global agent must not be inherited');};
  try {
    const login=await fetch(base+'/management/v1/session',{method:'POST',headers:{Origin:base,Authorization:'Bearer synthetic-read-credential'}});
    assert.equal(login.status,200);
    await fetch(base+'/management/v1/state?target=gateway',{headers:{Origin:base,Cookie:'unrelated=synthetic-private; fixture_read=synthetic-session',Authorization:'Bearer must-not-forward'}});
    assert.equal(seen[1].headers.cookie,'fixture_read=synthetic-session');assert.equal(seen[1].headers.authorization,undefined);
    assert.equal(seen[1].headers.origin,origin);assert.equal(seen[1].headers.host,new URL(origin).host);
    assert.equal(seen[1].url,'/management/v1/state?target=gateway');
    assert.equal((await fetch(base+'/management/v1/preflight',{method:'POST',headers:{Origin:base}})).status,404);
    assert.equal((await fetch(base+'/management/v1/session',{method:'POST'})).status,403);
    assert.equal((await fetch(base+'/management/v1/session',{method:'POST',headers:{Origin:base},body:'x'})).status,403);
    assert.equal(seen.length,2);
  } finally {globalAgent.createConnection=previous;await close(frontend);await close(backend);}
});
test('development config rejects unsupported modes and aborts instead of choosing synthetic data',async()=>{
  const previous=globalThis.fetch;
  try {
    globalThis.fetch=async()=>new Response('{"schema":"gateway-devdemo/v1","mode":"external","preset":"all"}');
    await assert.rejects(loadDemoConfig(),/Unsupported/);
    globalThis.fetch=(_url,{signal})=>new Promise((_resolve,reject)=>{if(signal.aborted)reject(Error('aborted'));else signal.addEventListener('abort',()=>reject(Error('aborted')),{once:true});});
    const controller=new AbortController(),pending=loadDemoConfig(controller.signal);controller.abort();await assert.rejects(pending,/aborted/);
  } finally {globalThis.fetch=previous;}
});
test('actual API presets enforce read sessions and shutdown after fixture termination',{skip:!real,timeout:240000},async()=>{
  for(const [index,preset] of ['all','usage'].entries()) {
    const running=await startFixtureDemo({port:43148+index,preset});
    let cookie='';
    const base=running.url.replace(/\/$/,'');
    const client=createClient('gateway',async(path,options)=>{
      const response=await fetch(base+path,{...options,headers:{...options.headers,Origin:base,...(cookie?{Cookie:cookie}:{})}});
      if(response.headers.has('set-cookie'))cookie=response.headers.get('set-cookie').split(';')[0];
      return response;
    });
    try {
      assert.deepEqual(await(await fetch(base+'/__devdemo/config')).json(),{schema:'gateway-devdemo/v1',mode:'fixture',preset});
      await assert.rejects(client.capabilities(),{status:401});
      await client.login(fixtureReadCredential);
      const allowed=(await client.capabilities()).data.allowed_operations;
      assert.deepEqual(allowed,preset==='usage'?['read_usage']:['read_state','read_usage','read_operations']);
      assert.equal((await client.usage(1,2,'UTC')).data.groups[1].token_sums.input_tokens,'9007199254740993123');
      if(preset==='usage'){await assert.rejects(client.state(),{status:403});await assert.rejects(client.operations(),{status:403});}
      else {assert.equal((await client.state()).data.modules.length,3);assert.equal((await client.operations()).data.items[0].observed_state,'uncertain');}
      assert.equal((await fetch(base+'/management/v1/state?target=gateway',{headers:{Cookie:cookie,Origin:'https://untrusted.example'}})).status,403);
      assert.equal((await fetch(base+'/management/v1/credential-delivery',{method:'POST',headers:{Cookie:cookie,Origin:base}})).status,404);
      await client.logout();await assert.rejects(client.capabilities(),{status:401});
      await running.fixture.stop();
      assert.deepEqual(await running.finished,{unexpected:true});
      await assert.rejects(fetch(base+'/__devdemo/config'));
      await assert.rejects(fetch(running.fixture.origin+'/management/v1/capabilities?target=gateway'));
    } finally {client.dispose();await running.close();}
  }
});
test('fixture launcher closes its owned temporary store and server on parent EOF',{skip:!real,timeout:180000},async()=>{
  const directory=fileURLToPath(new URL('../../.local/web-fixtures/',import.meta.url));mkdirSync(directory,{recursive:true});
  const before=readdirSync(directory).sort();
  const child=spawn(process.execPath,[fileURLToPath(new URL('../scripts/devdemo-fixture.mjs',import.meta.url)),'--port','43150','--preset','usage','--parent-stdin'],{stdio:['pipe','pipe','pipe']});
  child.stderr.resume();const exited=once(child,'exit');
  try {
    await new Promise((resolve,reject)=>{
      let text='';const timer=setTimeout(()=>reject(Error('Fixture launcher readiness timeout')),120000);
      child.stdout.on('data',chunk=>{text+=chunk;if(text.includes('DevDemo API fixture: http://127.0.0.1:43150/')){clearTimeout(timer);resolve();}});
      child.once('error',error=>{clearTimeout(timer);reject(error);});
      child.once('exit',()=>{clearTimeout(timer);reject(Error('Fixture launcher ended before readiness'));});
    });
    assert.equal((await fetch('http://127.0.0.1:43150/__devdemo/config')).status,200);
  } finally {
    child.stdin.end();const timer=setTimeout(()=>child.kill('SIGKILL'),10000);const [code]=await exited;clearTimeout(timer);assert.equal(code,0);
    assert.deepEqual(readdirSync(directory).sort(),before);
  }
  await assert.rejects(fetch('http://127.0.0.1:43150/__devdemo/config'));
});
test('terminal interrupt preserves graceful owned fixture cleanup',{skip:!real||process.platform==='win32',timeout:180000},async()=>{
  const directory=fileURLToPath(new URL('../../.local/web-fixtures/',import.meta.url));mkdirSync(directory,{recursive:true});
  const before=readdirSync(directory).sort();
  const child=spawn(process.execPath,[fileURLToPath(new URL('../scripts/devdemo-fixture.mjs',import.meta.url)),'--port','43151','--preset','usage','--parent-stdin'],{stdio:['pipe','pipe','pipe'],detached:true});
  child.stderr.resume();const exited=once(child,'exit');
  try {
    await new Promise((resolve,reject)=>{
      let text='';const timer=setTimeout(()=>reject(Error('Fixture launcher readiness timeout')),120000);
      child.stdout.on('data',chunk=>{text+=chunk;if(text.includes('DevDemo API fixture: http://127.0.0.1:43151/')){clearTimeout(timer);resolve();}});
      child.once('error',error=>{clearTimeout(timer);reject(error);});
      child.once('exit',()=>{clearTimeout(timer);reject(Error('Fixture launcher ended before readiness'));});
    });
    assert.equal((await fetch('http://127.0.0.1:43151/__devdemo/config')).status,200);
  } finally {
    if(child.exitCode===null)process.kill(-child.pid,'SIGINT');const timer=setTimeout(()=>child.kill('SIGKILL'),10000);const [code]=await exited;clearTimeout(timer);assert.equal(code,0);
    assert.deepEqual(readdirSync(directory).sort(),before);
  }
  await assert.rejects(fetch('http://127.0.0.1:43151/__devdemo/config'));
});
test('an invalid fixture preset fails without starting synthetic mode',{timeout:10000},async()=>{
  const child=spawn(process.execPath,[fileURLToPath(new URL('../scripts/devdemo-fixture.mjs',import.meta.url)),'--preset','unknown'],{stdio:['ignore','pipe','pipe']});
  let output='';child.stdout.on('data',chunk=>{output+=chunk;});child.stderr.resume();
  const [code]=await once(child,'exit');assert.notEqual(code,0);assert.doesNotMatch(output,/DevDemo synthetic:/);
});
test('a missing Rust toolchain fails before starting any demo server',{timeout:10000},async()=>{
  const child=spawn(process.execPath,[fileURLToPath(new URL('../scripts/devdemo-fixture.mjs',import.meta.url))],{env:{...process.env,PATH:''},stdio:['ignore','pipe','pipe']});
  let output='',errors='';child.stdout.on('data',chunk=>{output+=chunk;});child.stderr.on('data',chunk=>{errors+=chunk;});
  const [code]=await once(child,'exit');assert.notEqual(code,0);assert.doesNotMatch(output,/DevDemo (?:synthetic|API fixture):/);assert.match(errors,/API fixture mode could not start/);
});
