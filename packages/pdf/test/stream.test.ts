import { readFileSync, rmSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { render } from '../index.js';
import { Printer } from '../src/stream.js';

const fixtures = fileURLToPath(new URL('../../../crates/imprenta-pdf/tests', import.meta.url));
const font = (name: string) => readFileSync(join(fixtures, 'fonts', name));

const roman = { fonts: [{ weight: 'regular', data: font('Roboto-Regular.ttf') }] };
const page = { width: 595, height: 842 };
const head = {
  columns: [{ width: { unit: 'pt', value: 80 } }, {}],
  header: { cells: [{ text: 'Ref.' }, { text: 'Concepto' }] },
};

const rows = (from: number, to: number) =>
  Array.from({ length: to - from }, (_, i) => ({
    cells: [{ text: `${from + i}` }, { text: `Asiento contable numero ${from + i}` }],
  }));

/** The same ledger as one declared document, for comparison. */
const declared = (count: number) =>
  JSON.stringify({ page, children: [{ t: 'table', ...head, rows: rows(0, count) }] });

const scratch = (name: string) => {
  const path = join(tmpdir(), `imprenta-stream-${name}-${process.pid}.pdf`);
  rmSync(path, { force: true });
  return path;
};

const stream = async (count: number, batch: number, path?: string) => {
  const printer = new Printer(page, roman);
  await printer.openTable(head);
  for (let sent = 0; sent < count; sent += batch) {
    await printer.rows(rows(sent, Math.min(sent + batch, count)));
  }
  await printer.closeTable();
  return printer.finish(path);
};

describe('a document fed in pieces', () => {
  it('is the document', async () => {
    // Everything rests on this. If the two paths differ at all there are two
    // engines here, and two sets of bugs to find.
    const whole = await render(declared(400), roman);

    const fed = await stream(400, 50);

    expect(fed.pages).toBe(whole.pages);
    expect(fed.pdf?.equals(whole.pdf)).toBe(true);
  });

  it('does not care how the pieces are cut', async () => {
    // A producer batches by whatever its database does, and none of that
    // should reach the page.
    const [one, seven, all] = await Promise.all([stream(200, 1), stream(200, 7), stream(200, 200)]);

    expect(one.pdf?.equals(seven.pdf as Buffer)).toBe(true);
    expect(seven.pdf?.equals(all.pdf as Buffer)).toBe(true);
  });

  it('writes to a file without the bytes passing through the heap', async () => {
    const path = scratch('to-file');

    const out = await stream(200, 50, path);

    expect(out.pdf ?? null).toBeNull();
    expect(out.path).toBe(path);
    expect(statSync(path).size).toBe(out.bytes);
  });

  it('takes nodes as well as tables', async () => {
    const printer = new Printer(page, roman);
    await printer.node({ t: 'text', runs: [{ text: 'Libro mayor' }] });
    await printer.openTable(head);
    await printer.rows(rows(0, 20));
    await printer.closeTable();
    await printer.node({ t: 'text', runs: [{ text: 'Fin' }] });

    const out = await printer.finish();

    expect(out.pages).toBe(1);
    expect(out.diagnostics).toEqual([]);
  });

  it('reports what the engine noticed, at the end', async () => {
    const printer = new Printer(page, roman);
    await printer.openTable(head);
    await printer.rows([{ cells: [{ text: '1' }, { text: '日本語' }] }]);
    await printer.closeTable();

    const out = await printer.finish();

    expect(out.diagnostics.join(' ')).toContain('missing-glyph');
  });
});

describe('a document with no table in it', () => {
  const para = (i: number) => ({
    t: 'text',
    runs: [{ text: `${i}. Intervención registrada en el acta del expediente` }],
  });

  it('is the document too', async () => {
    // A transcript, a log, a book. The table is not what makes streaming
    // worth doing, and measuring only tables was how this went unnoticed.
    const count = 600;
    const whole = await render(
      JSON.stringify({ page, children: Array.from({ length: count }, (_, i) => para(i)) }),
      roman,
    );

    const printer = new Printer(page, roman);
    for (let sent = 0; sent < count; sent += 50) {
      await printer.nodes(Array.from({ length: 50 }, (_, i) => para(sent + i)));
    }
    const fed = await printer.finish();

    expect(fed.pages).toBe(whole.pages);
    expect(fed.pdf?.equals(whole.pdf)).toBe(true);
  });

  it('holds no more than a table does', async () => {
    const printer = new Printer(page, roman);
    const held: number[] = [];
    for (let sent = 0; sent < 8_000; sent += 500) {
      await printer.nodes(Array.from({ length: 500 }, (_, i) => para(sent + i)));
      held.push(printer.pending);
    }
    await printer.finish();

    expect(Math.max(...held)).toBeLessThan(400);
    expect(held[held.length - 1]).toBeLessThan(held[0] + 100);
  });

  it('still takes a single node when there is only one', async () => {
    const printer = new Printer(page, roman);
    await printer.node(para(1));

    const out = await printer.finish();

    expect(out.pages).toBe(1);
  });
});

describe('what it costs', () => {
  it('holds about a page however long the document gets', async () => {
    // The number the whole design exists to keep flat. Not a threshold: the
    // claim is that it does not move, and a threshold would pass for a
    // hundred rows and hide a leak at a hundred thousand.
    const printer = new Printer(page, roman);
    await printer.openTable(head);

    const held: number[] = [];
    for (let sent = 0; sent < 20_000; sent += 1_000) {
      await printer.rows(rows(sent, sent + 1_000));
      held.push(printer.pending);
    }
    await printer.finish();

    const first = held[0];
    const last = held[held.length - 1];
    expect(last).toBeLessThan(first + 100);
    expect(Math.max(...held)).toBeLessThan(400);
  });

  it('keeps the event loop turning', async () => {
    // The property the buffered path won and this one must not give back.
    // Doing the measuring inline would trade a service's memory for its
    // ability to answer anything at all.
    let ticks = 0;
    const timer = setInterval(() => {
      ticks += 1;
    }, 1);

    const started = performance.now();
    const out = await stream(60_000, 2_000);
    const elapsed = performance.now() - started;
    clearInterval(timer);

    expect(out.pages).toBeGreaterThan(100);
    expect(elapsed).toBeGreaterThan(50);
    expect(ticks).toBeGreaterThan(20);
  });

  it('costs the caller less than declaring the document whole', async () => {
    // The reason to prefer it. Measured as the peak the JS heap reaches,
    // since that is the half of the memory the engine could never fix.
    const count = 60_000;

    global.gc?.();
    const beforeWhole = process.memoryUsage().heapUsed;
    const whole = declared(count);
    const wholePeak = process.memoryUsage().heapUsed - beforeWhole;
    expect(whole.length).toBeGreaterThan(0);

    global.gc?.();
    const beforeFed = process.memoryUsage().heapUsed;
    let fedPeak = 0;
    const printer = new Printer(page, roman);
    await printer.openTable(head);
    for (let sent = 0; sent < count; sent += 1_000) {
      await printer.rows(rows(sent, sent + 1_000));
      fedPeak = Math.max(fedPeak, process.memoryUsage().heapUsed - beforeFed);
    }
    await printer.closeTable();
    await printer.finish();

    expect(fedPeak).toBeLessThan(wholePeak);
  });
});

describe('using it wrongly', () => {
  it('refuses a second call before the first has settled', async () => {
    // Not because the order would break — the channel is first in, first
    // out — but because a loop that forgets to await would queue the whole
    // ledger, which is the one thing this exists to avoid.
    const printer = new Printer(page, roman);
    await printer.openTable(head);

    const first = printer.rows(rows(0, 100));
    await expect(printer.rows(rows(100, 200))).rejects.toThrow(/await/);

    await first;
    await printer.finish();
  });

  it('recovers from a batch it could not read', async () => {
    // One bad batch must not wedge the document shut with the real error
    // buried under a complaint about being busy.
    const printer = new Printer(page, roman);
    await printer.openTable(head);

    await expect(printer.rows([{ cells: 'not a list' } as never])).rejects.toThrow();
    await printer.rows(rows(0, 10));

    const out = await printer.finish();
    expect(out.pages).toBe(1);
  });

  it('refuses rows with no table open', async () => {
    const printer = new Printer(page, roman);

    await expect(printer.rows(rows(0, 10))).rejects.toThrow(/table/);
  });

  it('refuses to be used after it has finished', async () => {
    const printer = new Printer(page, roman);
    await printer.node({ t: 'text', runs: [{ text: 'a' }] });
    await printer.finish();

    await expect(printer.node({ t: 'text', runs: [{ text: 'b' }] })).rejects.toThrow();
  });

  it('prints what arrived when a table was left open', async () => {
    // A stream that ended early — a dropped connection, a cancelled job —
    // should give back the pages that did arrive.
    const printer = new Printer(page, roman);
    await printer.openTable(head);
    await printer.rows(rows(0, 20));

    const out = await printer.finish();

    expect(out.pages).toBe(1);
  });
});
