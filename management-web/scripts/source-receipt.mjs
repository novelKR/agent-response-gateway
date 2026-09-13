import { readFileSync, readdirSync, lstatSync, realpathSync } from 'node:fs';
import { resolve, join, relative } from 'node:path';
import { createHash } from 'node:crypto';
// A verified source export is a build input, not an identity-provider assertion or attestation.
export function exportedSource(root, receiptPath) {
  root=realpathSync(root);
  const raw=readFileSync(receiptPath);
  if(raw.length>2*1024*1024)throw new Error('Source receipt is too large');
  const receipt=JSON.parse(raw);
  if(receipt.schema!=='gateway-source-export/v1'||!/^[0-9a-f]{40}$/.test(receipt.source_commit)
      ||!receipt.files||Object.keys(receipt.files).length>8192)throw new Error('Invalid source receipt');
  const files=new Set(Object.keys(receipt.files));let total=0;
  for(const [name,digest] of Object.entries(receipt.files)) {
    if(!/^[a-zA-Z0-9_./-]+$/.test(name)||name.split('/').some(p=>!p||p==='.'||p==='..'||['.git','.private','.local','target','node_modules'].includes(p))
        ||!/^[0-9a-f]{64}$/.test(digest))throw new Error('Invalid source path or hash');
    const file=resolve(root,name);
    if(!lstatSync(file).isFile()||realpathSync(file)!==file)throw new Error('Source contains a link or special file');
    const bytes=readFileSync(file);total+=bytes.length;
    if(total>256*1024*1024||createHash('sha256').update(bytes).digest('hex')!==digest)throw new Error('Exported source differs');
  }
  function walk(directory) {
    for(const item of readdirSync(directory,{withFileTypes:true})) {
      if(['node_modules','.local','target'].includes(item.name)&&item.isDirectory())continue;
      const file=join(directory,item.name),name=relative(root,file).split('\\').join('/');
      if(item.isSymbolicLink())throw new Error('Linked source');
      if(item.isDirectory())walk(file);
      else if(!item.isFile()||!files.has(name))throw new Error('Unlisted source file');
    }
  }
  walk(root);
  for(const name of ['LICENSE','Cargo.lock','management-web/package-lock.json','management-web/scripts/build.mjs','management-web/src/App.vue'])
    if(!files.has(name))throw new Error('Incomplete source receipt');
  return {source_commit:receipt.source_commit,source_dirty:false};
}
