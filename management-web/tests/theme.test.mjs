import test from 'node:test';
import assert from 'node:assert/strict';
import { createTheme, themeStorageKey } from '../src/theme.mjs';
import { createClient } from '../src/api.mjs';

test('theme preference defaults to light, follows system only when selected, and releases its listener', () => {
  const root={dataset:{}}, listeners=new Set(), values=new Map();
  const media={matches:true,addEventListener:(_,f)=>listeners.add(f),removeEventListener:(_,f)=>listeners.delete(f)};
  const storage={getItem:k=>values.get(k),setItem:(k,v)=>values.set(k,v)};
  const theme=createTheme({root,media,storage});
  assert.equal(root.dataset.theme,'light');
  theme.set('system');assert.equal(root.dataset.theme,'dark');
  media.matches=false;listeners.forEach(f=>f());assert.equal(root.dataset.theme,'light');
  theme.set('dark');listeners.forEach(f=>f());assert.equal(root.dataset.theme,'dark');
  assert.deepEqual([...values],[[themeStorageKey,'dark']]);
  theme.dispose();assert.equal(listeners.size,0);
  const reopened=createTheme({root,media,storage});assert.equal(reopened.choice,'dark');reopened.dispose();
  values.set(themeStorageKey,'<invalid>');const clean=createTheme({root,media,storage});assert.equal(clean.choice,'light');
  assert.throws(()=>clean.set('unknown'));clean.dispose();
});

test('unavailable storage and temporary presentation changes do not require persistence', () => {
  const root={dataset:{}}, media={matches:false,addEventListener(){},removeEventListener(){}};
  const storage={getItem(){throw Error('blocked');},setItem(){throw Error('blocked');}};
  const theme=createTheme({root,media,storage,initial:'dark'});theme.set('system');assert.equal(root.dataset.theme,'light');theme.dispose();
  const temporary=createTheme({root,media,initial:'dark'});assert.equal(temporary.choice,'dark');temporary.dispose();
});

test('disposing a client aborts all in-flight reads and rejects later reads', async () => {
  const signals=[];
  const client=createClient('gateway',(_path,{signal})=>new Promise((_resolve,reject)=>{
    signals.push(signal);signal.addEventListener('abort',()=>reject(Error('aborted')),{once:true});
  }));
  const pending=[client.capabilities(),client.operations()];
  client.dispose();
  for(const request of pending)await assert.rejects(request,{code:'connection_unavailable'});
  assert.equal(signals.length,2);assert.ok(signals.every(s=>s.aborted));
  await assert.rejects(client.state(),{code:'connection_unavailable'});assert.equal(signals.length,2);
});
