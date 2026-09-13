import test from 'node:test';
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {once} from 'node:events';
import {fileURLToPath} from 'node:url';
import {createConnection} from 'node:net';
import {request as httpRequest} from 'node:http';
import {mkdirSync,mkdtempSync,writeFileSync,rmSync} from 'node:fs';
import {join} from 'node:path';
import {requestBoundary,validPort} from '../scripts/devdemo-server.mjs';
test('development request authority is exact for HTTP and HMR',()=>{
  const make=(host,origin)=>({headers:{host,...(origin?{origin}:{})},rawHeaders:['Host',host,...(origin?['Origin',origin]:[])]});
  const authority='127.0.0.1:43142';
  assert.ok(requestBoundary(make(authority),authority));
  assert.ok(requestBoundary(make(authority,'http://'+authority),authority,true));
  assert.equal(requestBoundary(make(authority),authority,true),false);
  assert.equal(requestBoundary(make('localhost:43142'),authority),false);
  assert.equal(requestBoundary(make(authority,'https://untrusted.example'),authority),false);
  const repeated=make(authority);repeated.rawHeaders.push('Host',authority);assert.equal(requestBoundary(repeated,authority),false);
  for(const value of ['0','NaN','80','65536'])assert.throws(()=>validPort(value));
});
test('development server denies private files, foreign requests and management endpoints, and exits with its parent',{timeout:30000},async()=>{
  const script=fileURLToPath(new URL('../scripts/devdemo.mjs',import.meta.url));
  const privateRoot=fileURLToPath(new URL('../../.local/devdemo-boundary-tests/',import.meta.url));mkdirSync(privateRoot,{recursive:true});
  const directory=mkdtempSync(join(privateRoot,'case-')),privateFile=join(directory,'private.txt');writeFileSync(privateFile,'Synthetic nonpublic sentinel');
  const child=spawn(process.execPath,[script,'--port','43147','--parent-stdin'],{stdio:['pipe','pipe','pipe']});
  child.stderr.resume();const exited=once(child,'exit');
  try {
    const ready=await new Promise((resolve,reject)=>{
      let output='';const timer=setTimeout(()=>reject(Error('Development server readiness timeout')),15000);
      child.stdout.on('data',chunk=>{output+=chunk;const match=output.match(/DevDemo synthetic: (http:\/\/127\.0\.0\.1:43147\/)/);if(match){clearTimeout(timer);resolve(match[1]);}});
      child.once('error',error=>{clearTimeout(timer);reject(error);});
      child.once('exit',()=>{clearTimeout(timer);reject(Error('Development server ended before readiness'));});
    });
    assert.equal((await fetch(ready)).status,200);
    assert.equal((await fetch(ready+'canvas.js')).status,200);
    assert.equal((await fetch(ready+'@fs'+privateFile)).status,403);
    for(const path of ['.env','@fs/etc/passwd','@fs'+fileURLToPath(new URL('../../.local/not-public',import.meta.url)),'management/v1/state','__devdemo/config']) {
      assert.ok([403,404].includes((await fetch(ready+path)).status));
    }
    assert.equal((await fetch(ready,{headers:{Origin:'https://untrusted.example'}})).status,403);
    const wrongHost=await new Promise((resolve,reject)=>{const request=httpRequest(ready,{headers:{Host:'untrusted.invalid'}},response=>{response.resume();resolve(response.statusCode);});request.on('error',reject);request.end();});
    assert.equal(wrongHost,403);
    assert.equal((await fetch(ready,{method:'POST'})).status,405);
    const socket=createConnection({host:'127.0.0.1',port:43147});
    await once(socket,'connect');const closed=once(socket,'close');let received='';
    socket.on('data',chunk=>{received+=chunk;});
    socket.write('GET / HTTP/1.1\r\nHost: 127.0.0.1:43147\r\nOrigin: https://untrusted.example\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n');
    await closed;assert.doesNotMatch(received,/101 Switching Protocols/);
  } finally {
    child.stdin.end();const timer=setTimeout(()=>child.kill('SIGKILL'),5000);const [code]=await exited;clearTimeout(timer);rmSync(directory,{recursive:true,force:true});assert.equal(code,0);
  }
});
