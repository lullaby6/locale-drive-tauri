// Prepares everything the desktop app ships besides its own exe:
//   src-tauri/server/                  the locale-drive server (src + production node_modules)
//   src-tauri/binaries/node-<target>   the Node.js runtime that runs it (Tauri sidecar)
//
// The web app is already built and committed in locale-drive/src/public, so it is only
// rebuilt from the locale-drive-frontend submodule with --frontend (or if it is missing).

import { execSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

const root = path.resolve(import.meta.dirname, '..');
const app = path.join(root, 'locale-drive');
const tauri = path.join(root, 'src-tauri');
const staged = path.join(tauri, 'server');
const binaries = path.join(tauri, 'binaries');

const run = (command, cwd) => execSync(command, { cwd, stdio: 'inherit' });

const [major, minor] = process.versions.node.split('.').map(Number);
if (major < 22 || (major === 22 && minor < 12)) {
    console.error(`Node.js >= 22.12.0 is required (this one is ${process.version}): it is the runtime bundled into the app.`);
    process.exit(1);
}

// 1. The locale-drive submodule
if (!fs.existsSync(path.join(app, 'package.json'))) {
    run('git submodule update --init --recursive', root);
}

// 2. The web app (src/public)
const rebuildFrontend = process.argv.includes('--frontend') || !fs.existsSync(path.join(app, 'src', 'public', 'index.html'));

if (rebuildFrontend) {
    const frontend = path.join(app, 'locale-drive-frontend');
    if (!fs.existsSync(path.join(frontend, 'package.json'))) run('git submodule update --init --recursive', app);
    if (!fs.existsSync(path.join(frontend, 'node_modules'))) run('npm install', frontend);
    run('npm run build', app);
}

// 3. The server, with only its production dependencies
fs.mkdirSync(staged, { recursive: true });
fs.rmSync(path.join(staged, 'src'), { recursive: true, force: true });
fs.cpSync(path.join(app, 'src'), path.join(staged, 'src'), { recursive: true });

const lockOf = dir => {
    const file = path.join(dir, 'package-lock.json');
    return fs.existsSync(file) ? fs.readFileSync(file, 'utf8') : null;
};

const depsChanged = lockOf(app) !== lockOf(staged) || !fs.existsSync(path.join(staged, 'node_modules'));

fs.copyFileSync(path.join(app, 'package.json'), path.join(staged, 'package.json'));
fs.copyFileSync(path.join(app, 'package-lock.json'), path.join(staged, 'package-lock.json'));

if (depsChanged) {
    run('npm ci --omit=dev --no-audit --no-fund', staged);
} else {
    console.log('Server dependencies are up to date');
}

// 4. The Node.js runtime, named the way Tauri expects sidecars: <name>-<target triple>[.exe]
const rustInfo = execSync('rustc -vV').toString();
const target = rustInfo.match(/^host: (.+)$/m)[1].trim();
const extension = process.platform === 'win32' ? '.exe' : '';
const nodeBinary = path.join(binaries, `node-${target}${extension}`);

fs.mkdirSync(binaries, { recursive: true });

if (!fs.existsSync(nodeBinary) || fs.statSync(nodeBinary).size !== fs.statSync(process.execPath).size) {
    fs.copyFileSync(process.execPath, nodeBinary);
    console.log(`Node.js ${process.version} copied to ${path.relative(root, nodeBinary)}`);
}

console.log('Server ready in src-tauri/server');
