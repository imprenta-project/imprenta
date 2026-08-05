import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { Engine, EngineError } from '../dist/engine.js';
import { close, render } from '../dist/index.js';

const fixtures = fileURLToPath(new URL('../../../crates/imprenta-pdf/tests', import.meta.url));
const font = (name: string) => new Uint8Array(readFileSync(join(fixtures, 'fonts', name)));

const page = { width: 595, height: 842 };
const hello = JSON.stringify({ page, children: [{ t: 'text', runs: [{ text: 'Hola' }] }] });
const ledger = (rows: number) =>
  JSON.stringify({
    page,
    children: [
      {
        t: 'table',
        columns: [
          { width: { unit: 'percent', value: 0.6 } },
          { width: { unit: 'percent', value: 0.4 } },
        ],
        rows: Array.from({ length: rows }, (_, i) => ({
          cells: [{ text: `Prestación de servicios, asiento ${i}` }, { text: '1.200,00' }],
        })),
      },
    ],
  });

let engine: Engine;
beforeAll(async () => {
  engine = await Engine.load({ fonts: [{ data: font('Roboto-Regular.ttf') }] });
});

afterAll(async () => {
  await close();
});

/**
 * The synchronous surface — what a browser, a worker or a CLI reaches for.
 *
 * It is the same engine the promise-returning calls use, one boundary closer,
 * so the thing worth asserting is that the two cannot disagree.
 */
describe('Engine', () => {
  it('renders the same bytes the promise-returning call renders', async () => {
    const there = await render(ledger(500), {
      fonts: [{ weight: 'regular', data: font('Roboto-Regular.ttf') }],
      size: 1,
    });

    const here = engine.render(ledger(500));

    expect(here.pages).toBe(there.pages);
    expect(Buffer.from(here.pdf).equals(Buffer.from(there.pdf))).toBe(true);
  });

  it('renders a second document on the same instance', () => {
    // The napi/emnapi WebAssembly build deadlocked here and served exactly one
    // document per process. This binding exists partly because of it.
    const first = engine.render(hello);
    const second = engine.render(hello);

    expect(Buffer.from(second.pdf).equals(Buffer.from(first.pdf))).toBe(true);
  });

  it('keeps working after a document it could not read', () => {
    expect(() => engine.render('{ not json')).toThrow(EngineError);

    expect(engine.render(hello).pages).toBe(1);
  });

  it('refuses to start without a font, at load rather than at render', async () => {
    await expect(Engine.load({ fonts: [] })).rejects.toThrow(/no fonts/);
  });

  it('refuses a weight it does not know, at load rather than at render', async () => {
    await expect(
      // A caller can always reach past the types; the engine has to answer.
      Engine.load({ fonts: [{ weight: 'semibold' as 'bold', data: font('Roboto-Regular.ttf') }] }),
    ).rejects.toThrow(/semibold/);
  });

  it('hands back what the engine noticed', () => {
    const out = engine.render(
      JSON.stringify({ page, children: [{ t: 'text', runs: [{ text: '日本語' }] }] }),
    );

    expect(out.diagnostics.length).toBeGreaterThan(0);
  });

  it('settles at a footprint instead of climbing with every render', async () => {
    // The property `imprenta_out_release` exists for. WebAssembly memory is
    // never handed back to the host, so an instance that kept its last PDF —
    // or leaked a request buffer — would climb until the process restarted,
    // which on a server reads as a slow leak nobody can attribute.
    //
    // Not an equality: the allocator legitimately takes a little more room on
    // the first passes. What must not happen is growth that tracks the number
    // of documents.
    const settling = await Engine.load({ fonts: [{ data: font('Roboto-Regular.ttf') }] });
    for (let i = 0; i < 3; i++) settling.render(ledger(2000));
    const settled = settling.memoryBytes;

    for (let i = 0; i < 20; i++) settling.render(ledger(2000));

    expect(settling.memoryBytes).toBeLessThanOrEqual(settled * 1.1);
  });

  it('takes bytes as readily as a string', () => {
    const asText = engine.render(hello);
    const asBytes = engine.render(new TextEncoder().encode(hello));

    expect(Buffer.from(asBytes.pdf).equals(Buffer.from(asText.pdf))).toBe(true);
  });
});
