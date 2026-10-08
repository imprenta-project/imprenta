import { execFileSync } from 'node:child_process';
import { copyFileSync, rmSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '..', '..', '..');
const target = 'wasm32-unknown-unknown';
rmSync(join(here, '..', 'dist'), { recursive: true, force: true });
execFileSync('cargo', ['build', '--release', '-p', 'imprenta-escpos-wasm', '--target', target], {
  cwd: root,
  stdio: 'inherit',
});
copyFileSync(
  join(root, 'target', target, 'release', 'imprenta_escpos_wasm.wasm'),
  join(here, '..', 'imprenta-escpos.wasm'),
);
