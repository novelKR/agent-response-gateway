import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { verifiedOutput, sha256 } from '../scripts/output.mjs';
test('static output verification rejects modified, unlisted, missing and unsafe source artifacts', () => {
  const base=fileURLToPath(new URL('../../.local/web-tests/',import.meta.url));mkdirSync(base,{recursive:true});const dir=mkdtempSync(base+'output-');
  const commit='a'.repeat(40), manifest={schema:'gateway-management-web/v1',api_contract:'gateway-management-http/v1',state_contract:'gateway-management-state/v1',source_commit:commit,source_dirty:false,source_url:'https://github.com/novelKR/agent-response-gateway',read_only:true,files:{}};
  const record=()=>writeFileSync(dir+'/web-manifest.json',JSON.stringify(manifest));
  try {
    for(const name of ['index.html','LICENSE.txt','web-notices.txt','web-dependencies.json']){writeFileSync(dir+'/'+name,name);manifest.files[name]=sha256(name);}record();
    assert.equal(verifiedOutput(dir,commit).files.size,5);
    manifest.source_dirty=true;record();assert.throws(()=>verifiedOutput(dir,commit));manifest.source_dirty=false;record();
    writeFileSync(dir+'/index.html','modified');assert.throws(()=>verifiedOutput(dir));writeFileSync(dir+'/index.html','index.html');
    writeFileSync(dir+'/unexpected.js','unexpected');assert.throws(()=>verifiedOutput(dir));rmSync(dir+'/unexpected.js');
    manifest.files['../escape']='a'.repeat(64);record();assert.throws(()=>verifiedOutput(dir));delete manifest.files['../escape'];record();
    rmSync(dir+'/LICENSE.txt');assert.throws(()=>verifiedOutput(dir));
  } finally { rmSync(dir,{recursive:true,force:true}); }
});
