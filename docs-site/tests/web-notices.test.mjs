import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdirSync,mkdtempSync,writeFileSync,rmSync } from 'node:fs';
import { join,win32 } from 'node:path';
import { fileURLToPath } from 'node:url';
import { webNoticesPlugin,npmPath } from '../scripts/web-notices.mjs';
test('Windows package paths resolve npm lock entries and preserve shipped notice bytes',()=>{
  const lock={packages:{'node_modules/@vue/runtime-core':{version:'fixture'}}};
  const relative=win32.relative('C:\\source\\management-web','C:\\source\\management-web\\node_modules\\@vue\\runtime-core');
  assert.equal(lock.packages[relative],undefined);
  assert.equal(lock.packages[npmPath(relative)].version,'fixture');
  const base=fileURLToPath(new URL('../../.local/web-notice-tests/',import.meta.url));mkdirSync(base,{recursive:true});const root=mkdtempSync(join(base,'notice-'));
  const project=join(root,'management-web'),pkg=join(project,'node_modules','synthetic-notice');
  for(const directory of [pkg,join(project,'licensing'),join(root,'.local','management-web')])mkdirSync(directory,{recursive:true});
  const metadata={name:'synthetic-notice',version:'1.0.0',license:'MIT'};
  const original='Synthetic original notice.\n';
  writeFileSync(join(pkg,'package.json'),JSON.stringify(metadata));writeFileSync(join(pkg,'LICENSE'),original);writeFileSync(join(pkg,'index.js'),'export const synthetic=true;');
  writeFileSync(join(project,'package-lock.json'),JSON.stringify({packages:{'node_modules/synthetic-notice':{...metadata,resolved:'https://registry.npmjs.org/synthetic-notice/-/synthetic-notice-1.0.0.tgz',integrity:'sha512-synthetic'}}}));
  writeFileSync(join(project,'licensing','supplements.json'),'{}');writeFileSync(join(project,'licensing','embedded-assets.json'),'[]');
  const output=[];
  try {
    const plugin=webNoticesPlugin(root+'/','management-web');
    plugin.generateBundle.call({emitFile:asset=>output.push(asset)},{},{entry:{type:'chunk',modules:{[join(pkg,'index.js').replaceAll('/','\\')]:{}}}});
    const records=JSON.parse(output.find(f=>f.fileName==='web-dependencies.json').source);
    assert.equal(records.length,1);assert.equal(records[0].name,'synthetic-notice');
    assert.ok(output.find(f=>f.fileName==='web-notices.txt').source.includes(Buffer.from(original)));
  } finally {rmSync(root,{recursive:true,force:true});}
});
