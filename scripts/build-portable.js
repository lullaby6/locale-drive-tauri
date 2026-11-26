// Packs the portable version of the app after `tauri build`:
//   src-tauri/target/release/bundle/portable/Locale Drive_<version>_x64-portable.zip
//
// Tauri leaves everything the app needs next to its exe in target/release: the exe,
// the Node.js sidecar (node.exe) and the server resources (server/). The installer
// copies those same files, so zipping them gives an app that runs without installing.

import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

const root = path.resolve(import.meta.dirname, '..');
const release = path.join(root, 'src-tauri', 'target', 'release');
const config = JSON.parse(fs.readFileSync(path.join(root, 'src-tauri', 'tauri.conf.json'), 'utf8'));

const name = config.productName;
const exe = `${config.mainBinaryName}.exe`;
const files = [exe, 'node.exe', 'server'];

const missing = files.filter(file => !fs.existsSync(path.join(release, file)));
if (missing.length > 0) {
    console.error(`Missing in ${path.relative(root, release)}: ${missing.join(', ')}. Run "npm run build" first.`);
    process.exit(1);
}

const output = path.join(release, 'bundle', 'portable');
const folder = path.join(output, name);
const zip = path.join(output, `${name}_${config.version}_x64-portable.zip`);

// Stage the files in a "Locale Drive" folder, so unzipping doesn't scatter them.
fs.rmSync(output, { recursive: true, force: true });
fs.mkdirSync(folder, { recursive: true });

for (const file of files) {
    fs.cpSync(path.join(release, file), path.join(folder, file), { recursive: true });
}

// Windows 10+ ships bsdtar, which writes .zip files with -a. Use it explicitly: Git Bash's
// GNU tar can come first in PATH and can't write zips (or read "C:" paths).
const tar = process.platform === 'win32' ? path.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'tar.exe') : 'tar';
execFileSync(tar, ['-a', '-c', '-f', path.basename(zip), name], { cwd: output, stdio: 'inherit' });
fs.rmSync(folder, { recursive: true, force: true });

const size = (fs.statSync(zip).size / 1024 / 1024).toFixed(1);
console.log(`Portable version ready: ${path.relative(root, zip)} (${size} MB)`);
