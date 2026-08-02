// Cross-platform launcher for the `rad` CLI.
//
// The npm scripts used to hardcode `..\..\..\target\debug\rad.exe`, which only
// worked on Windows and could not run on the Linux CI runners. This resolves the
// binary for the host platform and falls back to `cargo run` when it is absent,
// so a fresh clone works without a manual build step first.

import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const serverDir = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = resolve(serverDir, '..', '..', '..');

const binary = join(
  repoRoot,
  'target',
  'debug',
  process.platform === 'win32' ? 'rad.exe' : 'rad',
);

const args = process.argv.slice(2);
if (args.length === 0) {
  console.error('usage: node scripts/run-rad.mjs <entry.rad> [args...]');
  process.exit(2);
}

const [command, commandArgs] = existsSync(binary)
  ? [binary, args]
  : [
      'cargo',
      ['run', '-q', '--manifest-path', join(repoRoot, 'Cargo.toml'), '-p', 'rad-vm', '--bin', 'rad', '--', ...args],
    ];

// cwd stays the server directory so relative `.rad` entry paths keep resolving.
const child = spawn(command, commandArgs, {
  cwd: serverDir,
  stdio: 'inherit',
  shell: false,
});

child.on('error', (err) => {
  console.error(`failed to launch ${command}: ${err.message}`);
  process.exit(1);
});

child.on('exit', (code, signal) => {
  if (signal) {
    process.exit(1);
  }
  process.exit(code ?? 1);
});
