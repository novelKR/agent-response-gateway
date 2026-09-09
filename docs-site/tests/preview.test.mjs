import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, rm, symlink } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { request } from 'node:http';
import { previewServer } from '../scripts/preview.mjs';

test('static preview preserves the project base and refuses unpublished or altered files', async () => {
  const state = fileURLToPath(new URL('../../.local/test-state/', import.meta.url));
  await mkdir(state, { recursive: true });
  const root = await mkdtemp(state + 'preview-');
  const files = { 'index.html': '<h1>Home</h1>', 'guide.html': '<h1>Guide</h1>', '404.html': '<h1>Missing</h1>' };
  const hashes = {};
  for (const [name, content] of Object.entries(files)) {
    await writeFile(root + '/' + name, content);
    hashes[name] = createHash('sha256').update(content).digest('hex');
  }
  await writeFile(root + '/build-manifest.json', JSON.stringify({ schema: 'gateway-docs-build/v1', files: hashes }));
  await writeFile(root + '/unlisted.txt', 'unpublished fixture');
  await symlink(root + '/index.html', root + '/link.html');
  const server = previewServer(root);
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const port = server.address().port;
  function get(path, options = {}) {
    return new Promise((resolve, reject) => {
      const req = request({ host: '127.0.0.1', port, path, ...options }, response => {
        let body = '';
        response.on('data', chunk => { body += chunk; });
        response.on('end', () => resolve({ status: response.statusCode, headers: response.headers, body }));
      });
      req.on('error', reject); req.end();
    });
  }
  try {
    assert.equal((await get('/agent-response-gateway/')).status, 200);
    assert.equal((await get('/agent-response-gateway/guide')).body, files['guide.html']);
    assert.equal((await get('/agent-response-gateway/guide', { method: 'HEAD' })).body, '');
    const absent = await get('/agent-response-gateway/missing');
    assert.equal(absent.status, 404); assert.equal(absent.body, files['404.html']);
    for (const path of ['/guide', '/agent-response-gateway/%2e%2e/index.html', '/agent-response-gateway/.git/config',
      '/agent-response-gateway/unlisted.txt', '/agent-response-gateway/link.html', '/agent-response-gateway/guide%5cfile']) {
      assert.equal((await get(path)).status, 404, path);
    }
    assert.equal((await get('/agent-response-gateway/', { headers: { host: 'untrusted.example' } })).status, 403);
    assert.equal((await get('/agent-response-gateway/', { method: 'POST' })).status, 403);
    assert.equal((await get('/agent-response-gateway/', { headers: { origin: 'https://untrusted.example' } })).headers['access-control-allow-origin'], undefined);
    await writeFile(root + '/guide.html', 'changed after verification');
    assert.equal((await get('/agent-response-gateway/guide')).status, 404);
  } finally {
    await new Promise(resolve => server.close(resolve));
    await rm(root, { recursive: true });
  }
});
