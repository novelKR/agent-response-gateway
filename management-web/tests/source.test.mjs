import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { exportedSource } from '../scripts/source-receipt.mjs';
test('an exported build requires exact committed file evidence and rejects changes or unlisted input',()=>{
  const parent=fileURLToPath(new URL('../../.local/web-source-tests/',import.meta.url));mkdirSync(parent,{recursive:true});
  const temporary=mkdtempSync(join(parent,'source-')),root=join(temporary,'export');mkdirSync(root);
  const files={};
  for(const name of ['LICENSE','Cargo.lock','management-web/package-lock.json','management-web/scripts/build.mjs','management-web/src/App.vue']) {
    const path=join(root,name);mkdirSync(join(path,'..'),{recursive:true});writeFileSync(path,'synthetic source');
    files[name]=createHash('sha256').update('synthetic source').digest('hex');
  }
  const receipt=join(temporary,'receipt.json');writeFileSync(receipt,JSON.stringify({schema:'gateway-source-export/v1',source_commit:'a'.repeat(40),files}));
  try {
    assert.deepEqual(exportedSource(root,receipt),{source_commit:'a'.repeat(40),source_dirty:false});
    writeFileSync(join(root,'LICENSE'),'changed');assert.throws(()=>exportedSource(root,receipt));
    writeFileSync(join(root,'LICENSE'),'synthetic source');writeFileSync(join(root,'unexpected.js'),'new code');assert.throws(()=>exportedSource(root,receipt));
  } finally {rmSync(temporary,{recursive:true,force:true});}
});
