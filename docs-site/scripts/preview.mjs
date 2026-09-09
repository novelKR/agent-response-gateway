import { createServer } from 'node:http';
import { readFile, lstat } from 'node:fs/promises';
import { resolve, extname, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';

const defaultRoot = fileURLToPath(new URL('../../.local/docs-site/dist/', import.meta.url));
const base = '/agent-response-gateway/';
const types = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.css': 'text/css; charset=utf-8', '.json': 'application/json; charset=utf-8', '.txt': 'text/plain; charset=utf-8', '.svg': 'image/svg+xml', '.ico': 'image/x-icon' };

export function previewServer(root = defaultRoot) {
  return createServer(async (request, response) => {
    response.setHeader('X-Content-Type-Options', 'nosniff');
    response.setHeader('Cache-Control', 'no-store');
    const port = request.socket.localPort;
    if (request.headers.host !== '127.0.0.1:' + port || !['GET', 'HEAD'].includes(request.method)) {
      response.writeHead(403); response.end(); return;
    }
    try {
      const pathname = decodeURIComponent(request.url.split('?')[0]);
      const parts = pathname.slice(base.length).split('/').filter(Boolean);
      if (!pathname.startsWith(base) || parts.some(p => p.startsWith('.') || p.includes('\\') || p.includes('\0'))) {
        response.writeHead(404); response.end(); return;
      }
      let file = resolve(root, ...parts);
      if (file !== resolve(root) && !file.startsWith(resolve(root) + sep)) throw new Error('Path escape');
      let candidate = resolve(root);
      for (const part of parts) {
        candidate = resolve(candidate, part);
        try { if ((await lstat(candidate)).isSymbolicLink()) throw new Error('Symlink'); }
        catch (error) { if (error.code !== 'ENOENT') throw error; }
      }
      if (pathname.endsWith('/')) file = resolve(file, 'index.html');
      else if (!extname(file)) file += '.html';
      let status = 200;
      let bytes;
      const manifest = JSON.parse(await readFile(resolve(root, 'build-manifest.json'), 'utf8'));
      if (manifest.schema !== 'gateway-docs-build/v1') throw new Error('Invalid build manifest');
      const readPublished = async (path) => {
        const relative = path.slice(resolve(root).length + 1).split(sep).join('/');
        if (!Object.hasOwn(manifest.files, relative)) {
          const missing = new Error('Unpublished file'); missing.code = 'ENOENT'; throw missing;
        }
        const stat = await lstat(path);
        if (!stat.isFile() || stat.isSymbolicLink()) throw new Error('Not a regular file');
        const content = await readFile(path);
        if (createHash('sha256').update(content).digest('hex') !== manifest.files[relative]) throw new Error('Artifact bytes changed');
        return content;
      };
      try {
        bytes = await readPublished(file);
      } catch (error) {
        if (error.code !== 'ENOENT') throw error;
        file = resolve(root, '404.html'); bytes = await readPublished(file); status = 404;
      }
      response.writeHead(status, { 'Content-Type': types[extname(file)] || 'application/octet-stream', 'Content-Length': bytes.length });
      response.end(request.method === 'HEAD' ? undefined : bytes);
    } catch { response.writeHead(404); response.end(); }
  });
}
if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const server = previewServer();
  server.listen(Number(process.env.DOCS_PORT || 43140), '127.0.0.1', () => {
    console.log('Static preview: http://127.0.0.1:' + server.address().port + base);
  });
}
