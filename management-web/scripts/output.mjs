import { readFileSync, readdirSync, lstatSync, realpathSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { createHash } from 'node:crypto';
export const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
export function verifiedOutput(directory, commit) {
  const root = realpathSync(directory), files = new Map();
  const raw = readFileSync(join(root, 'web-manifest.json'));
  const manifest = JSON.parse(raw);
  if (manifest.schema !== 'gateway-management-web/v1' || manifest.api_contract !== 'gateway-management-http/v1'
      || manifest.state_contract !== 'gateway-management-state/v1' || manifest.read_only !== true
      || !/^[0-9a-f]{40}$/.test(manifest.source_commit) || typeof manifest.source_dirty !== 'boolean'
      || manifest.source_url !== 'https://github.com/novelKR/agent-response-gateway'
      || commit && (manifest.source_commit !== commit || manifest.source_dirty)) throw new Error('Invalid build provenance');
  if (!manifest.files || Object.keys(manifest.files).length > 64) throw new Error('Invalid build inventory');
  for (const [name, digest] of Object.entries(manifest.files)) {
    if (!/^[a-zA-Z0-9_./-]+$/.test(name) || name.split('/').some(part => !part || part === '.' || part === '..')
        || !/^[a-f0-9]{64}$/.test(digest)) throw new Error('Invalid output path or digest');
    const path = resolve(root, name);
    if (!lstatSync(path).isFile() || realpathSync(path) !== path) throw new Error('Invalid static file');
    const bytes = readFileSync(path);
    if (sha256(bytes) !== digest) throw new Error('Static file digest differs');
    files.set(name, bytes);
  }
  for (const name of ['index.html', 'LICENSE.txt', 'web-notices.txt', 'web-dependencies.json'])
    if (!files.has(name)) throw new Error('Missing required static file');
  function walk(directory, prefix = '') {
    for (const item of readdirSync(directory, { withFileTypes: true })) {
      const name = prefix + item.name;
      if (item.isSymbolicLink()) throw new Error('Linked build output');
      if (item.isDirectory()) walk(join(directory, item.name), name + '/');
      else if (name !== 'web-manifest.json' && !files.has(name)) throw new Error('Unlisted build output');
    }
  }
  walk(root);
  files.set('web-manifest.json', raw);
  return { manifest, files };
}
