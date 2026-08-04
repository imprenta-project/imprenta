import { describe, expect, it } from 'vitest';
import { Document, Footer, Table, Text, toChunks, toDocument } from '../src/pdf/index.js';

const rows = (from: number, to: number) =>
  Array.from({ length: to - from }, (_, i) => ({
    cells: [{ text: `${from + i}` }, { text: `Asiento ${from + i}` }],
  }));

const columns = [{ width: 60 }, { width: 'auto' as const }];

const drain = async (element: Parameters<typeof toChunks>[0]) => {
  const out = [];
  for await (const chunk of toChunks(element)) {
    out.push(chunk);
  }
  return out;
};

describe('toChunks', () => {
  it('gives the page setup first, so a printer can be opened', async () => {
    const chunks = await drain(
      <Document>
        <Text>a</Text>
      </Document>,
    );

    expect(chunks[0].t).toBe('open');
    expect(chunks[0]).toMatchObject({ page: { width: 595.2756 } });
  });

  it('carries the bands and the accumulators in that first chunk', async () => {
    // They belong to the document rather than to any page, and a printer
    // needs them before the first row arrives.
    const chunks = await drain(
      <Document accumulators={['saldo']}>
        <Footer height={20}>
          <Text>pie</Text>
        </Footer>
        <Text>a</Text>
      </Document>,
    );

    expect(chunks[0]).toMatchObject({
      accumulators: ['saldo'],
      footer: { height: 20 },
    });
  });

  it('sends ordinary nodes in batches', async () => {
    const chunks = await drain(
      <Document>
        <Text>uno</Text>
        <Text>dos</Text>
      </Document>,
    );

    expect(chunks[1]).toMatchObject({ t: 'nodes' });
    expect((chunks[1] as { nodes: unknown[] }).nodes).toHaveLength(2);
  });

  it('breaks a long table into its head and its rows', async () => {
    // The one node that can be too big to hold, and the only reason any of
    // this exists.
    const chunks = await drain(
      <Document>
        <Table
          columns={columns}
          header={{ cells: [{ text: 'Ref' }, { text: 'C' }] }}
          rows={rows(0, 2500)}
        />
      </Document>,
    );

    const kinds = chunks.map((c) => c.t);
    expect(kinds).toContain('openTable');
    expect(kinds.filter((k) => k === 'rows').length).toBeGreaterThan(1);
    expect(kinds[kinds.length - 1]).toBe('closeTable');
  });

  it('keeps every row, in order, however it batched them', async () => {
    const chunks = await drain(
      <Document>
        <Table columns={columns} rows={rows(0, 2500)} />
      </Document>,
    );

    const sent = chunks
      .filter((c) => c.t === 'rows')
      .flatMap((c) => (c as { rows: { cells: { text: string }[] }[] }).rows)
      .map((r) => r.cells[0].text);

    expect(sent).toEqual(rows(0, 2500).map((r) => r.cells[0].text));
  });

  it('leaves a short table whole, since batching it buys nothing', async () => {
    const chunks = await drain(
      <Document>
        <Table columns={columns} rows={rows(0, 5)} />
      </Document>,
    );

    expect(chunks.map((c) => c.t)).toEqual(['open', 'nodes']);
  });

  it('describes the same document the whole-document path does', async () => {
    // The promise this rests on, at the level React can check it: what the
    // chunks add up to is what `toDocument` would have produced.
    const element = (
      <Document accumulators={['saldo']}>
        <Text>Antes</Text>
        <Table
          columns={columns}
          header={{ cells: [{ text: 'Ref' }, { text: 'C' }] }}
          rows={rows(0, 2500)}
        />
        <Text>Después</Text>
      </Document>
    );

    const whole = await toDocument(element);
    const chunks = await drain(element);

    const rebuilt: unknown[] = [];
    let open: Record<string, unknown> | null = null;
    for (const chunk of chunks) {
      if (chunk.t === 'nodes') rebuilt.push(...(chunk as { nodes: unknown[] }).nodes);
      if (chunk.t === 'openTable')
        open = { t: 'table', ...(chunk as { head: object }).head, rows: [] };
      if (chunk.t === 'rows' && open) {
        (open.rows as unknown[]).push(...(chunk as { rows: unknown[] }).rows);
      }
      if (chunk.t === 'closeTable' && open) {
        rebuilt.push(open);
        open = null;
      }
    }

    expect(rebuilt).toEqual(whole.children);
  });

  it("takes a batch size of the caller's choosing", async () => {
    const chunks = await drain2(
      <Document>
        <Table columns={columns} rows={rows(0, 300)} />
      </Document>,
      100,
    );

    expect(chunks.filter((c) => c.t === 'rows')).toHaveLength(3);
  });
});

async function drain2(element: Parameters<typeof toChunks>[0], batch: number) {
  const out = [];
  for await (const chunk of toChunks(element, { batch })) {
    out.push(chunk);
  }
  return out;
}
