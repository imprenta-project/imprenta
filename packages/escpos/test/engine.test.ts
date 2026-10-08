import { readFile } from 'node:fs/promises';
import { describe, expect, it } from 'vitest';
import { Engine } from '../dist/engine.js';
import { close, render } from '../dist/index.js';

describe('portable WASM engine', () => {
  it('has no host, WASI or Node imports', async () => {
    const bytes = await readFile(new URL('../imprenta-escpos.wasm', import.meta.url));
    const module = await WebAssembly.compile(bytes);
    expect(WebAssembly.Module.imports(module)).toEqual([]);
    const engine = await Engine.load({ wasm: module });
    expect(engine.render('{"children":[{"type":"text","text":"Portable"}]}').tickets).toBe(1);
  });

  it('copies output and recovers after a rejected job on the same instance', async () => {
    const engine = await Engine.load();
    const ir = '{"children":[{"type":"text","text":"First"}]}';
    const first = engine.render(ir);
    const snapshot = first.escpos.slice();
    expect(() => engine.render('{')).toThrow();
    for (let index = 0; index < 100; index++) {
      engine.render('{"children":[{"type":"text","text":"Next"}]}');
    }
    expect(first.escpos).toEqual(snapshot);
    expect(engine.render(ir).escpos).toEqual(snapshot);
  });

  it('decodes image assets in WASM and clears them between jobs', async () => {
    const engine = await Engine.load();
    const data = new Uint8Array(
      await readFile(
        new URL('../../../crates/imprenta-core/tests/images/logo.png', import.meta.url),
      ),
    );
    const ir = '{"children":[{"type":"image","src":"logo","width":96,"filter":"dither"}]}';
    const result = engine.render(ir, [{ name: 'logo', data }]);
    expect(Buffer.from(result.escpos).includes(Buffer.from([0x1d, 0x76, 0x30]))).toBe(true);
    expect(result.diagnostics).toEqual([]);
    expect(() => engine.render(ir)).toThrow(/logo/);
    expect(engine.render(ir, [{ name: 'logo', data }]).escpos).toEqual(result.escpos);
  });

  it('isolates queued jobs and starts a new pool after closing', async () => {
    try {
      const results = await Promise.all(
        Array.from({ length: 20 }, (_, index) =>
          render({
            children: [{ type: 'text', text: `Job ${index}`, bold: index % 2 === 0 }],
          }),
        ),
      );
      results.forEach((result, index) => {
        expect(Buffer.from(result.escpos).includes(Buffer.from(`Job ${index}\n`))).toBe(true);
      });
      await close();
      expect((await render({ children: [{ type: 'text', text: 'Restarted' }] })).tickets).toBe(1);
    } finally {
      await close();
    }
  });
});
