// Compiles the engine to WebAssembly and drops the module next to the package.
//
// There is no per-platform matrix here and that is the point: one artefact,
// built anywhere, that runs in Node, the browser, Deno, Bun and on the edge.
// Compare `.github/workflows/publish.yml`, which exists entirely because a
// `.node` has to be linked on a machine that can produce that target.
//
// `wasm-opt -O4` is deliberately *not* run. Measured on this engine it is
// worth 16% of the file size and 1.9% of the time — real, but it is another
// binary this build would have to find, and a release step is the right place
// for it rather than every `pnpm build`.
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '..', '..', '..');
const TARGET = 'wasm32-unknown-unknown';

// SIMD is on by default in every engine that matters and costs nothing where
// it is not used. Without it the module is 1.9% slower on a ledger.
const flags = ['-C', 'target-feature=+simd128'];

execFileSync('cargo', ['build', '--release', '-p', 'imprenta-xlsx-wasm', '--target', TARGET], {
  cwd: root,
  stdio: 'inherit',
  env: { ...process.env, RUSTFLAGS: [process.env.RUSTFLAGS, ...flags].filter(Boolean).join(' ') },
});

const built = join(root, 'target', TARGET, 'release', 'imprenta_xlsx_wasm.wasm');
const out = join(here, '..', 'imprenta-xlsx.wasm');
mkdirSync(dirname(out), { recursive: true });
copyFileSync(built, out);
console.log(`wrote ${out}`);
