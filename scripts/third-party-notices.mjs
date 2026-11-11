import { spawnSync } from 'node:child_process';
import { readFileSync, readdirSync, existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const metadata = spawnSync('cargo', ['metadata', '--format-version', '1', '--locked', '--offline', '--filter-platform', 'x86_64-pc-windows-msvc'], {
  cwd: root, encoding: 'utf8', windowsHide: true, maxBuffer: 32 * 1024 * 1024,
});
if (metadata.error || metadata.status !== 0) throw metadata.error ?? new Error(metadata.stderr);
const cargo = JSON.parse(metadata.stdout);
const resolved = new Set(cargo.resolve.nodes.map(n => n.id));
const lines = ['DayNote third-party notices', '', 'Generated from the locked Windows dependency graph and installed npm packages.',
  'This inventory also includes build tools. Each component retains its own license.', ''];

function append(name, version, license, directory) {
  lines.push('='.repeat(72), `${name} ${version}`, `License: ${license || 'See upstream license files'}`, '');
  const files = readdirSync(directory, { withFileTypes: true }).filter(f => f.isFile() && /^(license|licence|copying|copyright|notice)([._-]|$)/i.test(f.name));
  for (const file of files) lines.push(`--- ${file.name} ---`, readFileSync(join(directory, file.name), 'utf8'), '');
}
for (const pkg of cargo.packages.filter(p => p.source && resolved.has(p.id)).sort((a,b) => a.name.localeCompare(b.name))) {
  append(pkg.name, pkg.version, pkg.license, dirname(pkg.manifest_path));
}
const lock = JSON.parse(readFileSync(join(root, 'package-lock.json'), 'utf8'));
for (const path of Object.keys(lock.packages).filter(Boolean).sort()) {
  const manifest = join(root, path, 'package.json');
  if (!existsSync(manifest)) continue; // Optional packages for other platforms.
  const pkg = JSON.parse(readFileSync(manifest, 'utf8'));
  append(pkg.name, pkg.version, typeof pkg.license === 'string' ? pkg.license : pkg.license?.type, dirname(manifest));
}
const directory = join(root, 'src-tauri/binaries');
mkdirSync(directory, { recursive: true });
writeFileSync(join(directory, 'THIRD-PARTY-NOTICES.txt'), lines.join('\n'));
console.log('Generated third-party notices from local dependency sources.');
