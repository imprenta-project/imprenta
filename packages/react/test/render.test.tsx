import { createContext, useContext, useMemo, useState } from 'react';
import { describe, expect, it } from 'vitest';
import {
  B,
  Box,
  Document,
  Image,
  Link,
  List,
  PageBreak,
  Row,
  render,
  Spacer,
  Table,
  Text,
  toDocument,
} from '../src/pdf/index.js';
import type { Run } from '../src/pdf/ir.js';

const runsOf = async (element: Parameters<typeof toDocument>[0]) => {
  const document = await toDocument(<Document>{element}</Document>);
  const first = document.children[0];
  if (first.t !== 'text') {
    throw new Error(`expected a paragraph, got ${first.t}`);
  }
  return first.runs as Run[];
};

describe('a document', () => {
  it('comes out as the IR the engine reads', async () => {
    const document = await toDocument(
      <Document>
        <Text>Hola</Text>
      </Document>,
    );

    expect(document).toEqual({
      page: {
        width: 595.2756,
        height: 841.8898,
        margin: { top: 34, right: 34, bottom: 34, left: 34 },
      },
      children: [{ t: 'text', runs: [{ text: 'Hola' }] }],
    });
  });

  it('renders to a string the engine can be handed directly', async () => {
    const ir = await render(
      <Document>
        <Text>Hola</Text>
      </Document>,
    );

    expect(JSON.parse(ir).children[0].runs[0].text).toBe('Hola');
  });

  it('takes a page size by name', async () => {
    const a4 = await toDocument(<Document />);
    const letter = await toDocument(<Document size="Letter" />);

    expect(a4.page.width).toBeCloseTo(595.28, 1);
    expect(letter.page.width).toBeCloseTo(612, 1);
    expect(letter.page.height).toBeCloseTo(792, 1);
  });

  it('takes a page laid on its side', async () => {
    const portrait = await toDocument(<Document />);
    const landscape = await toDocument(<Document landscape />);

    expect(landscape.page.width).toBeCloseTo(portrait.page.height, 3);
    expect(landscape.page.height).toBeCloseTo(portrait.page.width, 3);
  });

  it('takes an explicit size over a named one', async () => {
    const document = await toDocument(<Document width={200} height={300} />);

    expect(document.page.width).toBe(200);
    expect(document.page.height).toBe(300);
  });

  it('takes one margin for all four sides, or four', async () => {
    const same = await toDocument(<Document margin={10} />);
    const each = await toDocument(<Document margin={{ top: 1, bottom: 2 }} />);

    expect(same.page.margin).toEqual({ top: 10, right: 10, bottom: 10, left: 10 });
    expect(each.page.margin).toEqual({ top: 1, bottom: 2 });
  });
});

describe('the elements', () => {
  it('stacks children in a box and lays them out in a row', async () => {
    const document = await toDocument(
      <Document>
        <Box>
          <Text>a</Text>
        </Box>
        <Row>
          <Text>b</Text>
        </Row>
      </Document>,
    );

    expect(document.children.map((c) => c.t)).toEqual(['box', 'row']);
    expect(document.children[0]).toEqual({
      t: 'box',
      children: [{ t: 'text', runs: [{ text: 'a' }] }],
    });
  });

  it('nests as deep as the author nests it', async () => {
    const document = await toDocument(
      <Document>
        <Box>
          <Row>
            <Box>
              <Text>deep</Text>
            </Box>
          </Row>
        </Box>
      </Document>,
    );

    expect(JSON.stringify(document)).toContain('"deep"');
  });

  it('carries a growing spacer through, and says nothing when it is not one', async () => {
    // The one thing an author cannot work out for themselves: how much room
    // is left on the page. `grow` is what hands that question to the packer,
    // so it has to survive the trip; and a plain spacer must not start
    // carrying a field it never asked for.
    const document = await toDocument(
      <Document>
        <Spacer grow />
        <Spacer height={12} />
      </Document>,
    );

    expect(document.children[0]).toEqual({ t: 'spacer', height: 0, grow: true });
    expect(document.children[1]).toEqual({ t: 'spacer', height: 12 });
  });

  it('carries the leaves through with their props', async () => {
    const document = await toDocument(
      <Document>
        <Image src="logo" width={108} />
        <Spacer height={12} />
        <PageBreak to="odd" />
      </Document>,
    );

    expect(document.children).toEqual([
      { t: 'image', src: 'logo', width: 108 },
      { t: 'spacer', height: 12 },
      { t: 'pageBreak', to: 'odd' },
    ]);
  });

  it('wraps a link round exactly one child', async () => {
    const document = await toDocument(
      <Document>
        <Link href="https://imprenta.dev">
          <Text>Condiciones</Text>
        </Link>
      </Document>,
    );

    expect(document.children[0]).toEqual({
      t: 'link',
      href: 'https://imprenta.dev',
      child: { t: 'text', runs: [{ text: 'Condiciones' }] },
    });
  });

  it('refuses a link with more than one child, rather than dropping one', async () => {
    const two = toDocument(
      <Document>
        <Link href="https://imprenta.dev">
          <Text>a</Text>
          <Text>b</Text>
        </Link>
      </Document>,
    );

    await expect(two).rejects.toThrow(/link/i);
  });

  it('takes a style on a box', async () => {
    const document = await toDocument(
      <Document>
        <Box padding={4} background="#f5f7fa" width={200} />
      </Document>,
    );

    expect(document.children[0]).toEqual({
      t: 'box',
      style: {
        padding: { top: 4, right: 4, bottom: 4, left: 4 },
        background: '#f5f7fa',
        width: 200,
      },
    });
  });
});

describe('alignment', () => {
  it('sets a paragraph against the edge it was told to', async () => {
    // Alignment lived only on a table column, so a figure could only be put
    // against the right margin by making it a table. `text-right` is the way
    // an author will reach for it, and the prop is the way TypeScript checks
    // it; both have to arrive as the same word the engine reads.
    const document = await toDocument(
      <Document>
        <Text align="end">por la prop</Text>
        <Text className="text-right">por la clase</Text>
        <Text>sin decir nada</Text>
      </Document>,
    );

    const styleOf = (at: number) =>
      (document.children[at] as unknown as { style?: { align?: string } }).style;

    expect(styleOf(0)?.align).toBe('end');
    expect(styleOf(1)?.align).toBe('end');
    // Left is what a paragraph does anyway, so it is not written down: the IR
    // carries what was asked for, not what was defaulted.
    expect(styleOf(2)?.align).toBeUndefined();
  });

  it('justifies a paragraph when asked, by the prop or by the class', async () => {
    // Justification is not a fourth direction to shove the line in: the line
    // stays where it is and its spaces grow. It reaches the engine as a word
    // like the others, though, so this is the same journey and the same test.
    const document = await toDocument(
      <Document>
        <Text align="justify">por la prop</Text>
        <Text className="text-justify">por la clase</Text>
      </Document>,
    );

    const styleOf = (at: number) =>
      (document.children[at] as unknown as { style?: { align?: string } }).style;

    expect(styleOf(0)?.align).toBe('justify');
    expect(styleOf(1)?.align).toBe('justify');
  });
});

describe('tables and lists', () => {
  it('resolves a row style exactly as it resolves a box style', async () => {
    // `RowProps.style` is typed as a box's props and an author is entitled to
    // read that literally: the three words that draw a hairline under a box
    // have to draw one under a row. They used to reach the engine untouched —
    // a colour where it holds a border per side, a number where it holds four
    // — so the document could not be read at all, and a `className` on a row
    // was dropped without a word. `background` and `radius` happened to work,
    // which is what made it look like the rest did too.
    const style = {
      border: '#D1D5DB',
      borderWidth: 0.5,
      borderSides: ['bottom'] as ('top' | 'right' | 'bottom' | 'left')[],
      padding: 4,
      className: 'bg-slate-100',
    };

    const document = await toDocument(
      <Document>
        <Box {...style}>
          <Text>x</Text>
        </Box>
        <Table
          columns={[{ width: 'auto' }]}
          header={{ style, cells: [{ text: 'Cabecera' }] }}
          rows={[{ style, cells: [{ text: 'Fila' }] }]}
        />
      </Document>,
    );

    const box = document.children[0] as unknown as { style: unknown };
    const table = document.children[1] as unknown as {
      header: { style: unknown };
      rows: { style: unknown }[];
    };

    expect(table.rows[0].style).toEqual(box.style);
    expect(table.header.style).toEqual(box.style);
  });

  it('passes a table through as the engine declares it', async () => {
    const document = await toDocument(
      <Document>
        <Table
          columns={[{ width: 60 }, { width: 'auto' }, { width: 80, align: 'end' }]}
          header={{ cells: [{ text: 'Ref.' }, { text: 'Concepto' }, { text: 'Importe' }] }}
          rows={[{ cells: [{ text: '001' }, { text: 'Licencia' }, { text: '1.200,00 €' }] }]}
          padding={4}
        />
      </Document>,
    );

    expect(document.children[0]).toEqual({
      t: 'table',
      columns: [
        { width: { unit: 'pt', value: 60 } },
        { width: { unit: 'auto' } },
        { width: { unit: 'pt', value: 80 }, align: 'end' },
      ],
      header: { cells: [{ text: 'Ref.' }, { text: 'Concepto' }, { text: 'Importe' }] },
      rows: [{ cells: [{ text: '001' }, { text: 'Licencia' }, { text: '1.200,00 €' }] }],
      padding: { top: 4, right: 4, bottom: 4, left: 4 },
    });
  });

  it('writes a column width the way the engine reads lengths', async () => {
    // A bare number is points, a string ending in % is a share of the width,
    // and "auto" is what is left. The author should not have to know the
    // tagged shape the engine wants.
    const document = await toDocument(
      <Document>
        <Table columns={[{ width: 60 }, { width: '50%' }, { width: 'auto' }, {}]} rows={[]} />
      </Document>,
    );

    expect((document.children[0] as unknown as { columns: unknown[] }).columns).toEqual([
      { width: { unit: 'pt', value: 60 } },
      { width: { unit: 'percent', value: 0.5 } },
      { width: { unit: 'auto' } },
      {},
    ]);
  });

  it('repeats a header unless told not to', async () => {
    const on = await toDocument(
      <Document>
        <Table columns={[]} rows={[]} />
      </Document>,
    );
    const off = await toDocument(
      <Document>
        <Table columns={[]} rows={[]} repeatHeader={false} />
      </Document>,
    );

    expect(on.children[0]).not.toHaveProperty('repeatHeader');
    expect(off.children[0]).toMatchObject({ repeatHeader: false });
  });

  it('passes a list through', async () => {
    const document = await toDocument(
      <Document>
        <List marker="decimal" items={['Pago a treinta días', 'Renovación tácita']} />
      </Document>,
    );

    expect(document.children[0]).toEqual({
      t: 'list',
      marker: 'decimal',
      items: ['Pago a treinta días', 'Renovación tácita'],
    });
  });
});

describe('paragraphs', () => {
  it('makes one run out of plain text', async () => {
    expect(await runsOf(<Text>Total a pagar</Text>)).toEqual([{ text: 'Total a pagar' }]);
  });

  it('keeps a bold stretch apart from the text around it', async () => {
    expect(
      await runsOf(
        <Text>
          Total <B>7.400,00 €</B>
        </Text>,
      ),
    ).toEqual([{ text: 'Total ' }, { text: '7.400,00 €', weight: 'bold' }]);
  });

  it('preserves the spaces between stretches', async () => {
    const runs = await runsOf(
      <Text>
        Total <B>7.400</B> €
      </Text>,
    );

    expect(runs.map((r) => r.text).join('')).toBe('Total 7.400 €');
  });

  it('joins neighbouring stretches that are styled the same', async () => {
    // Where JSX happens to split a string should not reach the engine: each
    // run is a shaping call, and a break between two is a break parley could
    // otherwise have chosen better.
    const runs = await runsOf(
      <Text>
        <B>{'7.400,00'} €</B>
      </Text>,
    );

    expect(runs).toEqual([{ text: '7.400,00 €', weight: 'bold' }]);
  });

  it('says where loose text should have gone', async () => {
    // `<Document>Hola</Document>` is a mistake every author makes once. The
    // engine cannot set text that is not in a paragraph, and "unexpected
    // node" would send them looking in the wrong place.
    const loose = toDocument(<Document>Hola</Document>);

    await expect(loose).rejects.toThrow(/Text/);
    await expect(loose).rejects.toThrow(/Hola/);
  });

  it('takes a size and colour from the paragraph', async () => {
    const document = await toDocument(
      <Document>
        <Text size={18} color="#1b3a5c">
          FACTURA
        </Text>
      </Document>,
    );

    expect(document.children[0]).toEqual({
      t: 'text',
      runs: [{ text: 'FACTURA' }],
      style: { size: 18, color: '#1b3a5c' },
    });
  });
});

describe('React itself', () => {
  it("runs the author's components", async () => {
    const Money = ({ amount }: { amount: number }) => <B>{amount.toFixed(2)} €</B>;
    const Line = () => (
      <Text>
        Total <Money amount={7400} />
      </Text>
    );

    expect(await runsOf(<Line />)).toEqual([
      { text: 'Total ' },
      { text: '7400.00 €', weight: 'bold' },
    ]);
  });

  it('gives components their hooks', async () => {
    // The reason this is a reconciler and not a tree walk. A component that
    // uses state, memo or context has to work the same here as anywhere.
    const Counter = () => {
      const [n] = useState(3);
      const doubled = useMemo(() => n * 2, [n]);
      return <Text>{doubled}</Text>;
    };

    expect(await runsOf(<Counter />)).toEqual([{ text: '6' }]);
  });

  it('gives components their context, through the paragraph too', async () => {
    const Currency = createContext('€');
    const Amount = () => <B>7.400,00 {useContext(Currency)}</B>;

    const runs = await runsOf(
      <Currency.Provider value="£">
        <Text>
          <Amount />
        </Text>
      </Currency.Provider>,
    );

    expect(runs).toEqual([{ text: '7.400,00 £', weight: 'bold' }]);
  });

  it('honours conditionals and lists', async () => {
    const overdue = false;
    const items = ['a', 'b'];
    const document = await toDocument(
      <Document>
        {overdue && <Text>Vencida</Text>}
        {items.map((item) => (
          <Text key={item}>{item}</Text>
        ))}
      </Document>,
    );

    expect(document.children).toHaveLength(2);
  });

  it('reports an error from a component rather than swallowing it', async () => {
    const Broken = () => {
      throw new Error('the total is missing');
    };

    await expect(
      toDocument(
        <Document>
          <Broken />
        </Document>,
      ),
    ).rejects.toThrow('the total is missing');
  });
});
