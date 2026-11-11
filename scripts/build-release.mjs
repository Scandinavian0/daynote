import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync, readFileSync, writeFileSync, readdirSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { dirname, resolve, join } from 'node:path';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
if (process.platform !== 'win32') throw new Error('Build the Windows release on Windows.');
function run(file, args) {
  const result = spawnSync(file, args, { cwd: root, stdio: 'inherit', windowsHide: true });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${file} failed (${result.status})`);
}
const host = spawnSync('rustc', ['-vV'], { encoding: 'utf8', windowsHide: true });
if (host.error || host.status !== 0) throw host.error ?? new Error('rustc is required');
const triple = host.stdout.match(/^host: (.+)$/m)?.[1]?.trim();
if (triple !== 'x86_64-pc-windows-msvc') throw new Error('The release currently supports x64 MSVC only.');
run('cargo', ['build', '--release', '--locked', '-p', 'daynote-cli']);
mkdirSync(join(root, 'src-tauri/binaries'), { recursive: true });
copyFileSync(join(root, 'target/release/daynote-cli.exe'), join(root, `src-tauri/binaries/daynote-cli-${triple}.exe`));
run(process.execPath, [join(root, 'scripts/third-party-notices.mjs')]);
run(process.execPath, [join(root, 'node_modules/@tauri-apps/cli/tauri.js'), 'build', '--config', 'src-tauri/tauri.release.conf.json', '--bundles', 'nsis', '--', '--locked']);
const version = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')).version;
const out = join(root, 'dist');
mkdirSync(out, { recursive: true });
const source = join(root, 'target/release/bundle/nsis');
const installer = readdirSync(source).find(name => name === `DayNote_${version}_x64-setup.exe`);
if (!installer) throw new Error('Expected NSIS installer not found.');
const filename = `DayNote-${version}-windows-x64-setup.exe`;
copyFileSync(join(source, installer), join(out, filename));
const digest = createHash('sha256').update(readFileSync(join(out, filename))).digest('hex');
writeFileSync(join(out, 'SHA256SUMS.txt'), `${digest}  ${filename}\n`);
console.log(`Release ready: ${join(out, filename)}`);
