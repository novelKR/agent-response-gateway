import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { jsonTokens } from '../src/json-tokens.mjs';
import { parse, compileScript } from '@vue/compiler-sfc';
import { createSSRApp, h } from 'vue';
import { renderToString } from 'vue/server-renderer';

const sample = { '한글 "key"': ['quote" slash\\ newline\n', -12.5, 1e-7, 1e21, null, 0, false, true, '', [], {}], large:'9007199254740993123', html:'<script>alert("x")</script><img src=x onerror=alert(1)>' };
test('JSON tokens preserve exact serialized text, escapes, numeric lexemes and token roles', () => {
  const text=JSON.stringify(sample,null,2), tokens=jsonTokens(text);
  assert.equal(tokens.map(t=>t.text).join(''),text);
  for (const kind of ['key','string','number','boolean','null','punctuation','plain']) assert.ok(tokens.some(t=>t.kind===kind));
  assert.equal(tokens.find(t=>t.text==='"large"').kind,'key');
  assert.equal(tokens.find(t=>t.text==='"9007199254740993123"').kind,'string');
  assert.equal(tokens.find(t=>t.text==='1e-7').kind,'number');
  assert.equal(tokens.find(t=>t.text==='1e+21').kind,'number');
  const explicit='{ "escaped\\\"key" \n: "value: true", "n": -0.25E+12 }';
  const result=jsonTokens(explicit);
  assert.equal(result.map(t=>t.text).join(''),explicit);
  assert.equal(result.find(t=>t.text==='"escaped\\\"key"').kind,'key');
  assert.equal(result.find(t=>t.text==='"value: true"').kind,'string');
  for(const value of [false,0,'',[],{}]) assert.equal(jsonTokens(JSON.stringify(value)).map(t=>t.text).join(''),JSON.stringify(value));
});

test('shared Vue component escapes HTML and distinguishes missing observations from false and zero', async () => {
  const source=readFileSync(new URL('../src/JsonView.vue',import.meta.url),'utf8');
  const {descriptor}=parse(source);
  const script=compileScript(descriptor,{id:'json-view-test',inlineTemplate:true});
  const module=script.content
    .replace(/from (["'])vue\1/g,`from ${JSON.stringify(import.meta.resolve('vue'))}`)
    .replace("'./json-tokens.mjs'",JSON.stringify(new URL('../src/json-tokens.mjs',import.meta.url).href));
  const {default:JsonView}=await import(`data:text/javascript;base64,${Buffer.from(module).toString('base64')}`);
  const render=value=>renderToString(createSSRApp({render:()=>h(JsonView,{value,emptyText:'미관측'})}));
  const html=await render(sample);
  assert.doesNotMatch(html,/<script|<img/);
  assert.match(html,/&lt;script&gt;/);
  assert.match(html,/class="json-null">null/);
  assert.match(html,/class="json-number">0/);
  assert.match(html,/class="json-boolean">false/);
  for(const value of [null,undefined]) assert.match(await render(value),/미관측/);
  for(const value of [false,0,'',[],{}]) assert.doesNotMatch(await render(value),/미관측/);
});

test('JSON token colors have at least 4.5:1 contrast on both theme surfaces', () => {
  const css=readFileSync(new URL('../src/tokens.css',import.meta.url),'utf8');
  const luminance=hex=>{
    const channels=hex.match(/\w\w/g).map(v=>parseInt(v,16)/255).map(v=>v<=0.04045?v/12.92:((v+0.055)/1.055)**2.4);
    return channels[0]*0.2126+channels[1]*0.7152+channels[2]*0.0722;
  };
  for(const block of css.split('}').slice(0,2)) {
    const background=luminance(block.match(/--surface-muted:#([\da-f]{6})/)[1]);
    for(const [,kind,hex] of block.matchAll(/--json-(\w+):#([\da-f]{6})/g)) {
      const foreground=luminance(hex),contrast=(Math.max(background,foreground)+0.05)/(Math.min(background,foreground)+0.05);
      assert.ok(contrast>=4.5,`${kind}: ${contrast}`);
    }
  }
});
