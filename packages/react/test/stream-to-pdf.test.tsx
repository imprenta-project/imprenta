import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { render as toPdf } from '@imprentajs/pdf';
import { Printer } from '@imprentajs/pdf/stream';
import { describe, expect, it } from 'vitest';
import {
  Document,
  Footer,
  PageCount,
  PageNumber,
  render,
  Table,
  Text,
  toChunks,
} from '../src/pdf/index.js';

const fixtures = fileURLToPath(new URL('../../../crates/imprenta-pdf/tests', import.meta.url));
const roman = {
  fonts: [{ weight: 'regular', data: readFileSync(join(fixtures, 'fonts/Roboto-Regular.ttf')) }],
};

const rows = (n: number) =>
  Array.from({ length: n }, (_, i) => ({
    cells: [{ text: `${i}` }, { text: `Asiento contable numero ${i}` }],
  }));

const Ledger = ({ count }: { count: number }) => (
  <Document>
    <Footer height={20}>
      <Text size={8}>
        Página <PageNumber /> de <PageCount />
      </Text>
    </Footer>
    <Text>Libro mayor</Text>
    <Table
      columns={[{ width: 60 }, { width: 'auto' }]}
      header={{ cells: [{ text: 'Ref' }, { text: 'Concepto' }] }}
      rows={rows(count)}
    />
  </Document>
);

/** Feeds a printer from the chunks a document yields. */
const print = async (element: Parameters<typeof toChunks>[0]) => {
  let printer: Printer | null = null;
  for await (const chunk of toChunks(element, { batch: 500 })) {
    if (chunk.t === 'open') {
      printer = new Printer(chunk.page, {
        ...roman,
        accumulators: chunk.accumulators,
        header: chunk.header,
        footer: chunk.footer,
      });
    } else if (chunk.t === 'nodes') {
      await printer?.nodes(chunk.nodes);
    } else if (chunk.t === 'openTable') {
      await printer?.openTable(chunk.head);
    } else if (chunk.t === 'rows') {
      await printer?.rows(chunk.rows);
    } else {
      await printer?.closeTable();
    }
  }
  return printer?.finish();
};

describe('React straight into the printer', () => {
  it('prints the same document as rendering it whole', async () => {
    // The chain end to end, and the promise it rests on: what the pieces add
    // up to is what the whole would have been, byte for byte.
    const element = <Ledger count={1200} />;

    const whole = await toPdf(await render(element), roman);
    const streamed = await print(element);

    expect(streamed?.pages).toBe(whole.pages);
    expect(streamed?.pdf?.equals(whole.pdf)).toBe(true);
  }, 60_000);

  it('numbers the pages, which needs the total and so needs the whole document', async () => {
    const out = await print(<Ledger count={1200} />);

    expect(out?.pages).toBeGreaterThan(3);
    expect(out?.diagnostics).toEqual([]);
  }, 60_000);
});
