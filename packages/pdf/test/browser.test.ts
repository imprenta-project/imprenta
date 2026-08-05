import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const here = fileURLToPath(new URL('.', import.meta.url));
const dist = (name: string) => readFileSync(join(here, '..', 'dist', name), 'utf8');

/**
 * The claim this whole package rests on is that the same file runs in a
 * browser. Nothing here can open one, so these assert the two things that
 * would break it and that no other test would notice.
 */
describe('the browser path', () => {
  it('reaches for no Node built-in until it is asked to read from disk', () => {
    // A static `import 'node:fs'` in the browser-facing entry points would
    // break a bundler targeting the web — and it would break at somebody
    // else's build, not here. The one place that needs the filesystem does a
    // dynamic import inside a `process.versions.node` guard, which a bundler
    // can leave alone.
    for (const name of ['engine.js', 'module.js']) {
      const source = dist(name);
      const statics = [...source.matchAll(/^import\s[^;]*from\s+'([^']+)'/gm)].map((m) => m[1]);

      expect(
        statics.filter((s) => s.startsWith('node:')),
        `${name} static imports`,
      ).toEqual([]);
    }
  });

  it('renders with no Buffer, no filesystem and bytes handed in', () => {
    // As close to a browser as Node gets: `Buffer` gone, the module supplied
    // the way `fetch` would supply it, and nothing allowed to fall back to
    // reading it from disk. Run in a child process because deleting `Buffer`
    // from this one would take the test runner with it.
    const script = `
      delete globalThis.Buffer;
      const { readFileSync } = await import('node:fs');
      const { Engine } = await import('${join(here, '..', 'dist', 'engine.js')}');

      // Bytes, as an ArrayBuffer, exactly as \`await response.arrayBuffer()\`
      // would hand them over.
      const wasm = new Uint8Array(readFileSync('${join(here, '..', 'imprenta-pdf.wasm')}')).buffer;
      const font = new Uint8Array(
        readFileSync('${join(here, '..', '..', '..', 'crates', 'imprenta-pdf', 'tests', 'fonts', 'Roboto-Regular.ttf')}'),
      );

      const engine = await Engine.load({ wasm, fonts: [{ data: font }] });
      const ir = JSON.stringify({
        page: { width: 595, height: 842 },
        footer: { height: 30, children: [{ t: 'text', size: 8, runs: [{ text: 'Pagina {{page}} de {{pages}}' }] }] },
        children: [{
          t: 'table',
          columns: [{ width: { unit: 'percent', value: 0.6 } }, { width: { unit: 'percent', value: 0.4 } }],
          rows: Array.from({ length: 400 }, (_, i) => ({
            cells: [{ text: 'Asiento ' + i }, { text: '1.200,00' }],
          })),
        }],
      });

      const out = engine.render(ir);
      const head = new TextDecoder().decode(out.pdf.subarray(0, 5));
      // A second one, because an instance that serves a single document is
      // the failure the whole binding was rewritten to avoid.
      const again = engine.render(ir);

      if (globalThis.Buffer !== undefined) throw new Error('Buffer came back');
      console.log(JSON.stringify({ head, pages: out.pages, same: again.pages === out.pages }));
    `;

    const output = execFileSync(process.execPath, ['--input-type=module', '-e', script], {
      encoding: 'utf8',
    });

    expect(JSON.parse(output.trim())).toEqual({ head: '%PDF-', pages: 6, same: true });
  }, 60_000);
});
