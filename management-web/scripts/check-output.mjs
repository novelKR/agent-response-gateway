import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { verifiedOutput } from './output.mjs';
const root = fileURLToPath(new URL('../../', import.meta.url));
const { files } = verifiedOutput(root + '.local/management-web/dist/', process.argv[2]);
if (!files.get('web-dependencies.json').equals(readFileSync(root + 'management-web/licensing/dependencies.json'))
    || !files.get('LICENSE.txt').equals(readFileSync(root + 'LICENSE'))) throw new Error('Reviewed notices or project license differ');
const html = files.get('index.html').toString();
if (/<script[^>]+src=["']https?:|<style|style=|https?:\/\/[^< ]+\.(?:js|css)/i.test(html)) throw new Error('External or inline executable content');
console.log(`Verified management Web: ${files.size} files, reviewed notices and static artifact digests`);
