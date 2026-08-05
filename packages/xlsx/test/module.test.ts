import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { close, write } from '../dist/index.js';
import { Writer } from '../dist/writer.js';

const wasm = readFileSync(fileURLToPath(new URL('../imprenta-xlsx.wasm', import.meta.url)));

const book = JSON.stringify({
  sheets: [
    {
      name: 'Ventas',
      rows: [
        {
          cells: [{ value: { t: 'text', v: 'Servicios' } }, { value: { t: 'number', v: 1200 } }],
        },
      ],
    },
  ],
});

/** What only exists once the crate has been compiled. */
describe('the module', () => {
  it('imports nothing', async () => {
    const module = await WebAssembly.compile(wasm);

    expect(WebAssembly.Module.imports(module)).toEqual([]);
  });

  it('exports everything the binding calls, and every export is prefixed', async () => {
    const module = await WebAssembly.compile(wasm);
    const exported = WebAssembly.Module.exports(module).map((e) => e.name);

    for (const name of [
      'imprenta_alloc',
      'imprenta_dealloc',
      'imprenta_write',
      'imprenta_book_open',
      'imprenta_book_rows',
      'imprenta_book_merge',
      'imprenta_book_next_sheet',
      'imprenta_book_row',
      'imprenta_book_finish',
      'imprenta_out_ptr',
      'imprenta_out_len',
      'imprenta_out_sheets',
      'imprenta_out_release',
      'imprenta_error_ptr',
      'imprenta_error_len',
    ]) {
      expect(exported, `missing ${name}`).toContain(name);
    }

    const loose = exported.filter((n) => !n.startsWith('imprenta_') && !n.startsWith('__'));
    expect(loose, 'unprefixed exports').toEqual(['memory']);
  });
});

describe('the browser path', () => {
  it('reaches for no Node built-in until it is asked to read from disk', () => {
    // A static `import 'node:fs'` in the browser-facing entry points would
    // break a bundler targeting the web, and it would break at somebody
    // else's build rather than here. The one place that needs the filesystem
    // does a dynamic import inside a `process.versions.node` guard.
    for (const name of ['writer.js', 'module.js']) {
      const source = readFileSync(
        fileURLToPath(new URL(`../dist/${name}`, import.meta.url)),
        'utf8',
      );
      const statics = [...source.matchAll(/^import\s[^;]*from\s+'([^']+)'/gm)].map((m) => m[1]);

      expect(
        statics.filter((imported) => imported.startsWith('node:')),
        `${name} static imports`,
      ).toEqual([]);
    }
  });
});

/** The synchronous surface — a browser, a worker, a CLI. */
describe('Writer', () => {
  it('writes the same bytes the promise-returning call writes', async () => {
    const there = await write(book);
    const writer = await Writer.load();

    const here = writer.write(book);

    expect(here.sheets).toBe(there.sheets);
    expect(Buffer.from(here.xlsx).equals(Buffer.from(there.xlsx))).toBe(true);
    await close();
  });

  it('writes a second workbook on the same instance', async () => {
    const writer = await Writer.load();

    const first = writer.write(book);
    const second = writer.write(book);

    expect(Buffer.from(second.xlsx).equals(Buffer.from(first.xlsx))).toBe(true);
  });

  it('reports a malformed workbook rather than dying', async () => {
    const writer = await Writer.load();

    expect(() => writer.write('{ not json')).toThrow(/not valid JSON/);
    expect(writer.write(book).sheets).toBe(1);
  });

  it('feeds a workbook a batch at a time, and says how far down it is', async () => {
    const writer = await Writer.load();
    const open = writer.book([{ name: 'Ventas' }]);

    open.rows([{ cells: [{ value: { t: 'number', v: 1 } }] }]);
    open.rows([{ cells: [{ value: { t: 'number', v: 2 } }] }]);

    expect(open.at).toBe(2);
    expect(Buffer.from(open.finish().xlsx.subarray(0, 2)).toString()).toBe('PK');
  });

  it('settles at a footprint instead of climbing with every workbook', async () => {
    const writer = await Writer.load();
    const many = JSON.stringify({
      sheets: [
        {
          name: 'Ventas',
          rows: Array.from({ length: 5000 }, (_, i) => ({
            cells: [{ value: { t: 'number', v: i } }],
          })),
        },
      ],
    });
    for (let i = 0; i < 3; i++) writer.write(many);
    const settled = writer.memoryBytes;

    for (let i = 0; i < 20; i++) writer.write(many);

    expect(writer.memoryBytes).toBeLessThanOrEqual(settled * 1.1);
  });
});
