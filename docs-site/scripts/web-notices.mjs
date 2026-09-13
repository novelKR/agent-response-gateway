import { readFileSync, existsSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';
import { createHash } from 'node:crypto';

// Rollup's client module inventory excludes server-only tools.
export function webNoticesPlugin(root, project = 'docs-site', introduction = 'Web dependencies shipped by this documentation build.\nOriginal license and notice bytes follow each package heading.\n') {
  const projectRoot = root + project + '/';
  const stateRoot = root + '.local/' + project + '/';
  return {
    name: 'gateway-web-notices',
    apply(config, env) { return env.command === 'build' && !config.build?.ssr; },
    generateBundle(_options, bundle) {
      const packages = new Map();
      for (const chunk of Object.values(bundle)) {
        if (chunk.type !== 'chunk') continue;
        for (const id of Object.keys(chunk.modules)) {
          const clean = id.split('?')[0];
          if (!clean.includes('/node_modules/')) continue;
          let directory = dirname(clean);
          while (directory.includes('/node_modules/')) {
            if (existsSync(join(directory, 'package.json'))) {
              const metadata = JSON.parse(readFileSync(join(directory, 'package.json'), 'utf8'));
              if (!metadata.name || !metadata.version) throw new Error('Invalid shipped package metadata');
              packages.set(metadata.name + '@' + metadata.version, { directory, metadata });
              break;
            }
            directory = dirname(directory);
          }
        }
      }
      const records = [];
      const supplements = JSON.parse(readFileSync(projectRoot + 'licensing/supplements.json', 'utf8'));
      const lock = JSON.parse(readFileSync(projectRoot + 'package-lock.json', 'utf8'));
      writeFileSync(stateRoot + 'client-package-candidates.json', JSON.stringify([...packages].map(([id, p]) => ({ id, path: relative(projectRoot, p.directory) })), null, 2) + '\n');
      const parts = [introduction];
      for (const [identity, { directory, metadata }] of [...packages].sort(([a], [b]) => a.localeCompare(b))) {
        const locked = lock.packages[relative(projectRoot, directory)];
        if (!locked || locked.version !== metadata.version || locked.license !== metadata.license
            || !locked.integrity || !locked.resolved.startsWith('https://registry.npmjs.org/')) {
          throw new Error('Shipped package differs from the npm lock: ' + identity);
        }
        let notices = readdirSync(directory).filter(name => /^(licen[cs]e|copying|notice)([.-]|$)/i.test(name));
        let noticeDirectory = directory;
        if (!notices.length && supplements[identity]) {
          noticeDirectory = projectRoot + 'licensing/originals/';
          notices = [supplements[identity].file];
          const hash = createHash('sha256').update(readFileSync(join(noticeDirectory, notices[0]))).digest('hex');
          if (hash !== supplements[identity].sha256) throw new Error('Supplement notice digest differs');
        }
        if (!notices.length) throw new Error('Missing shipped package notice: ' + identity);
        const record = { name: metadata.name, version: metadata.version, license: metadata.license,
          resolved: locked.resolved, integrity: locked.integrity, notices: [] };
        parts.push('\n===== ' + identity + ' =====\n');
        for (const name of notices.sort()) {
          const bytes = readFileSync(join(noticeDirectory, name));
          record.notices.push({ file: name, sha256: createHash('sha256').update(bytes).digest('hex'),
            source: supplements[identity]?.source || locked.resolved });
          parts.push('\n--- ' + name + ' ---\n', bytes, '\n');
        }
        records.push(record);
      }
      const embedded = JSON.parse(readFileSync(projectRoot + 'licensing/embedded-assets.json', 'utf8'));
      for (const asset of embedded) {
        if (createHash('sha256').update(readFileSync(projectRoot + asset.asset)).digest('hex') !== asset.asset_sha256) {
          throw new Error('Embedded theme asset changed');
        }
        parts.push('\n===== ' + asset.name + ' (' + asset.embedded_version + ') =====\n');
        for (const notice of asset.notices) {
          const bytes = readFileSync(projectRoot + 'licensing/originals/' + notice.file);
          if (createHash('sha256').update(bytes).digest('hex') !== notice.sha256) throw new Error('Embedded original notice changed');
          parts.push('\n--- ' + notice.file + ' ---\n', bytes, '\n');
        }
        records.push(asset);
      }
      const noticeBytes = Buffer.concat(parts.map(part => Buffer.isBuffer(part) ? part : Buffer.from(part)));
      this.emitFile({ type: 'asset', fileName: 'web-notices.txt', source: noticeBytes });
      this.emitFile({ type: 'asset', fileName: 'web-dependencies.json', source: JSON.stringify(records, null, 2) + '\n' });
      writeFileSync(stateRoot + 'shipped-packages.json', JSON.stringify(records, null, 2) + '\n');
    },
  };
}
