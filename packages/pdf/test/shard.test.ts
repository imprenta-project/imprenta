import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterAll, describe, expect, it } from 'vitest';
import { close, render } from '../dist/index.js';
import { shardable } from '../dist/shard.js';

const fixtures = fileURLToPath(new URL('../../../crates/imprenta-pdf/tests', import.meta.url));
const roman = {
  fonts: [
    {
      weight: 'regular' as const,
      data: new Uint8Array(readFileSync(join(fixtures, 'fonts', 'Roboto-Regular.ttf'))),
    },
  ],
};

const page = { width: 595, height: 842 };
const columns = [
  { width: { unit: 'percent', value: 0.6 } },
  { width: { unit: 'percent', value: 0.4 } },
];

const ledger = (rows: number, extra: Record<string, unknown> = {}) =>
  JSON.stringify({
    page,
    ...extra,
    children: [
      {
        t: 'table',
        columns,
        rows: Array.from({ length: rows }, (_, i) => ({
          cells: [{ text: `Prestación de servicios, asiento ${i}` }, { text: '1.200,00' }],
        })),
      },
    ],
  });

afterAll(async () => {
  await close();
});

/**
 * Splitting one document across the pool.
 *
 * The whole point is that a reader cannot tell. These assert the two ways it
 * could: a page count that drifts, and page numbers that restart.
 */
describe('a sharded render', () => {
  it('produces the pages the whole document produces', async () => {
    // Split anywhere but a page boundary and the pieces repaginate. This is
    // the assertion that catches it, and it is the reason there is a planning
    // pass at all.
    const whole = await render(ledger(3000), { ...roman, size: 1, shard: false });

    const sharded = await render(ledger(3000), { ...roman, size: 4, shard: true });

    expect(sharded.pages).toBe(whole.pages);
    expect(sharded.pages).toBeGreaterThan(40);
  }, 120_000);

  it('numbers its pages as one document, not as four', async () => {
    // A fragment that numbered itself from one would put "Página 1 de 43" a
    // quarter of the way through, and nothing about the file would look wrong
    // until somebody read it.
    const footer = {
      footer: {
        height: 30,
        children: [{ t: 'text', size: 8, runs: [{ text: 'Pagina {{page}} de {{pages}}' }] }],
      },
    };

    const sharded = await render(ledger(3000, footer), { ...roman, size: 4, shard: true });
    const whole = await render(ledger(3000, footer), { ...roman, size: 1, shard: false });

    expect(sharded.pages).toBe(whole.pages);
    const text = Buffer.from(sharded.pdf).toString('latin1');
    // The pages are compressed, so the numbers are not readable here. What is
    // readable is that the document has one page tree with the right count —
    // and the count came from parsing the merged file, not from adding up.
    expect(text).toContain('/Count ' + sharded.pages);
  }, 120_000);

  it('reads back as one document', async () => {
    const sharded = await render(ledger(2000), { ...roman, size: 4, shard: true });

    expect(Buffer.from(sharded.pdf.subarray(0, 5)).toString()).toBe('%PDF-');
    expect(sharded.pdf.length).toBeGreaterThan(0);
  }, 120_000);

  it('is faster than the same document on one engine', async () => {
    // The only reason any of this exists. Loose enough not to flake on a busy
    // machine, tight enough that a sharded path which quietly stopped
    // sharding would fail.
    const ir = ledger(6000);

    const alone = Date.now();
    await render(ir, { ...roman, size: 1, shard: false });
    const single = Date.now() - alone;

    const together = Date.now();
    await render(ir, { ...roman, size: 4, shard: true });
    const split = Date.now() - together;

    expect(split).toBeLessThan(single);
  }, 180_000);
});

describe('what may be sharded', () => {
  it('takes a document that is one table', () => {
    expect(shardable(JSON.parse(ledger(100)))).not.toBeNull();
  });

  it('refuses a document with running totals', () => {
    // The planner packs heights, and a running total is not a height. Until it
    // carries contributions too, a document that prints "suma y sigue" has to
    // go down the one-engine path — where it is correct.
    const withTotals = JSON.parse(ledger(100, { accumulators: ['saldo'] }));

    expect(shardable(withTotals)).toBeNull();
  });

  it('refuses a table whose header is declared, because that is an extra atom', () => {
    const withHeader = JSON.parse(ledger(100));
    withHeader.children[0].header = { cells: [{ text: 'Ref.' }, { text: 'Importe' }] };

    expect(shardable(withHeader)).toBeNull();
  });

  it('refuses anything that is not exactly one table', () => {
    const mixed = JSON.parse(ledger(100));
    mixed.children.unshift({ t: 'text', runs: [{ text: 'FACTURA' }] });

    expect(shardable(mixed)).toBeNull();
  });
});
