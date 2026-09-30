const { spawnSync } = require('node:child_process');
const { existsSync } = require('node:fs');
const { resolve } = require('node:path');

const repositoryRoot = resolve(__dirname, '..');
const localCli = resolve(repositoryRoot, 'node_modules/@tauri-apps/cli/tauri.js');

function fail(reason) {
  console.error(`Release preflight failed: ${reason}`);
  console.error('Run pnpm install --frozen-lockfile in the project directory before retrying.');
  process.exit(1);
}

if (!existsSync(localCli)) {
  fail('the project-local Tauri CLI is missing.');
}

const options = { cwd: repositoryRoot, encoding: 'utf8', timeout: 10_000 };
const packageManager = spawnSync('pnpm', ['--version'], options);
if (packageManager.error || packageManager.status !== 0) {
  fail('pnpm could not be started. Install or repair pnpm first.');
}

// Invoke the installed launcher directly: a global CLI must not conceal missing
// local dependencies, and starting it also verifies its native platform binding.
const cli = spawnSync(process.execPath, [localCli, '--version'], options);
if (cli.error || cli.status !== 0) {
  fail('the project-local Tauri CLI could not be started.');
}

console.log('Release dependencies are ready.');
