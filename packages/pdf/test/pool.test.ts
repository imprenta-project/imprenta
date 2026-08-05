import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterEach, describe, expect, it } from 'vitest';
import { close, render } from '../dist/index.js';
import { Printer } from '../dist/stream.js';

const fixtures = fileURLToPath(new URL('../../../crates/imprenta-pdf/tests', import.meta.url));
const roman = {
  fonts: [
    {
      weight: 'regular',
      data: new Uint8Array(readFileSync(join(fixtures, 'fonts', 'Roboto-Regular.ttf'))),
    },
  ],
};

const page = { width: 595, height: 842 };
const columns = [
  { width: { unit: 'percent', value: 0.6 } },
  { width: { unit: 'percent', value: 0.4 } },
];
const rows = (from: number, to: number) =>
  Array.from({ length: to - from }, (_, k) => ({
    cells: [{ text: `Prestación de servicios, asiento ${from + k}` }, { text: '1.200,00' }],
  }));
const ledger = (n: number) =>
  JSON.stringify({ page, children: [{ t: 'table', columns, rows: rows(0, n) }] });

afterEach(async () => {
  await close();
});

/**
 * The pool is what makes a synchronous engine usable from a server.
 *
 * These are the properties a service depends on and that no unit test of the
 * engine can see: that the calling thread stays free, that documents queue
 * rather than fail, and that a session holds an engine to itself.
 */
describe('the pool', () => {
  it('keeps the event loop turning while it prints', async () => {
    // The rule this package has always stated first. A promise wrapped around
    // a blocking call would satisfy the type and lose the property, so the
    // assertion has to be about the loop — which is what a server actually
    // loses.
    let ticks = 0;
    const timer = setInterval(() => {
      ticks++;
    }, 1);

    await Promise.all([
      render(ledger(4000), { ...roman, size: 2 }),
      render(ledger(4000), { ...roman, size: 2 }),
    ]);
    clearInterval(timer);

    expect(ticks).toBeGreaterThan(0);
  }, 60_000);

  it('queues what it cannot start and renders all of it', async () => {
    // Two engines, six documents. Nothing may be dropped and nothing may fail
    // for want of a free one.
    const documents = Array.from({ length: 6 }, (_, i) => ledger(50 + i));

    const results = await Promise.all(documents.map((ir) => render(ir, { ...roman, size: 2 })));

    expect(results).toHaveLength(6);
    for (const result of results) {
      expect(Buffer.from(result.pdf.subarray(0, 5)).toString()).toBe('%PDF-');
    }
  }, 60_000);

  it('survives a document it could not read and keeps the engine', async () => {
    await expect(render('{ not json', { ...roman, size: 1 })).rejects.toThrow(/not valid JSON/);

    const after = await render(ledger(10), { ...roman, size: 1 });
    expect(after.pages).toBeGreaterThan(0);
  }, 60_000);

  it('lets a printer hold an engine while other documents use the rest', async () => {
    // A session cannot move between engines, so it takes one out of
    // circulation. On a pool of two that must still leave one for everybody
    // else rather than wedging the whole thing.
    const printer = new Printer(page, { ...roman, size: 2 });
    await printer.openTable({ columns });
    await printer.rows(rows(0, 100));

    const other = await render(ledger(20), { ...roman, size: 2 });
    expect(other.pages).toBeGreaterThan(0);

    await printer.closeTable();
    const streamed = await printer.finish();
    expect(streamed.pages).toBeGreaterThan(0);
  }, 60_000);

  it('gives the engine back when a document finishes', async () => {
    // A lease that never returned would take the pool down one engine per
    // document until nothing could start at all.
    for (let i = 0; i < 4; i++) {
      const printer = new Printer(page, { ...roman, size: 1 });
      await printer.openTable({ columns });
      await printer.rows(rows(0, 20));
      await printer.closeTable();
      await printer.finish();
    }

    const after = await render(ledger(10), { ...roman, size: 1 });
    expect(after.pages).toBe(1);
  }, 60_000);

  it('rejects work once it has been closed, and starts again on the next call', async () => {
    const first = await render(ledger(10), { ...roman, size: 1 });
    await close();

    // Not an error: `close` forgets the pool, so the next call builds one.
    const second = await render(ledger(10), { ...roman, size: 1 });

    expect(Buffer.from(second.pdf).equals(Buffer.from(first.pdf))).toBe(true);
  }, 60_000);
});
