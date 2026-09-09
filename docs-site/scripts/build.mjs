import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { build } from 'vitepress';
import { copyFileSync, readFileSync, writeFileSync } from 'node:fs';

const site = fileURLToPath(new URL('../', import.meta.url));
const python = process.env.DOCS_PYTHON || 'python3';
if (process.versions.node !== '24.21.0') throw new Error('Use Node 24.21.0');
const prepare = spawnSync(python, ['-B', site + 'scripts/prepare.py'], { stdio: 'inherit' });
if (prepare.status !== 0) process.exit(prepare.status || 1);
await build(site);
copyFileSync(site + '../LICENSE', site + '../.local/docs-site/dist/LICENSE.txt');
const generated = readFileSync(site + '../.local/docs-site/shipped-packages.json');
const reviewed = site + 'licensing/dependencies.json';
if (process.argv.includes('--record-notices')) writeFileSync(reviewed, generated);
else if (!generated.equals(readFileSync(reviewed))) throw new Error('Web dependency notice review is missing or stale');
const check = spawnSync(python, ['-B', site + 'scripts/check-output.py', '--record'], { stdio: 'inherit' });
if (check.status !== 0) process.exit(check.status || 1);
