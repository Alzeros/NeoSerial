import { build } from 'vite';
import { mkdir, readFile, readdir, rename, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import path from 'node:path';

// Keep exact release maps for stack traces without embedding source code in the app.
const buildId = process.env.GITHUB_SHA || new Date().toISOString();
process.env.NEOSERIAL_FRONTEND_BUILD_ID = buildId;
await build();
const symbols = path.resolve('frontend-symbols', buildId.replace(/[^\w.-]/g, '-'));
const assets = [];
async function collect(dir) {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const file = path.join(dir, entry.name);
    if (entry.isDirectory()) { await collect(file); continue; }
    const relative = path.relative('dist', file).split(path.sep).join('/');
    if (entry.name.endsWith('.map')) {
      const destination = path.join(symbols, relative);
      await mkdir(path.dirname(destination), { recursive: true });
      await rename(file, destination);
    } else if (entry.name.endsWith('.js')) {
      assets.push({ file: relative, sha256: createHash('sha256').update(await readFile(file)).digest('hex') });
    }
  }
}
await collect('dist');
await mkdir(symbols, { recursive: true });
const { version } = JSON.parse(await readFile('package.json', 'utf8'));
await writeFile(path.join(symbols, 'manifest.json'), JSON.stringify({ build: buildId, version, assets }, null, 2));
console.log(`Frontend diagnostic symbols: ${symbols}`);
