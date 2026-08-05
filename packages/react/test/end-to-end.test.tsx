import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { render as toPdf } from '@imprentajs/pdf';
import { describe, expect, it } from 'vitest';
import {
  B,
  Box,
  Document,
  Image,
  List,
  Row,
  render,
  Span,
  Table,
  Text,
  Theme,
} from '../src/pdf/index.js';

/**
 * The whole chain, once: components to IR to a PDF with glyphs in it.
 *
 * Everything either side of this is tested on its own. What only this can
 * catch is the two halves disagreeing — a prop the React side names one way
 * and the engine reads another — which no amount of unit testing on either
 * side would notice.
 */

const fixtures = fileURLToPath(new URL('../../../crates/imprenta-pdf/tests', import.meta.url));
const font = (name: string) => readFileSync(join(fixtures, 'fonts', name));
const image = (name: string) => readFileSync(join(fixtures, 'images', name));

const assets = {
  fonts: [
    { weight: 'regular', data: font('Roboto-Regular.ttf') },
    { weight: 'bold', data: font('Roboto-Bold.ttf') },
  ],
  images: [{ name: 'logo', data: image('logo.png') }],
};

const NAVY = '#1b3a5c';

const items = [
  { ref: '001', concept: 'Licencia anual Imprenta Server, plan profesional', price: 1200 },
  { ref: '002', concept: 'Implantación y migración de las plantillas existentes', price: 3400 },
  { ref: '003', concept: 'Soporte prioritario 24×7, bolsa de 40 horas', price: 2800 },
];

const euros = (n: number) => `${n.toLocaleString('es-ES', { minimumFractionDigits: 2 })} €`;

const Invoice = ({ number }: { number: string }) => (
  <Document margin={40}>
    <Row>
      <Image src="logo" width={108} />
      <Box width={120} />
      <Box width={200}>
        <Text size={22} color={NAVY}>
          <B>FACTURA</B>
        </Text>
        <Text size={11}>
          <B>{number}</B>
        </Text>
        <Text size={8} color="#8a97a5">
          Emitida el 2 de agosto de 2026
        </Text>
      </Box>
    </Row>

    <Spacer />

    <Text size={9} spaceAfter={12}>
      Declarada en <B>React</B>, renderizada por el motor. El mismo IR lo puede producir{' '}
      <Span color={NAVY}>Vue, Python o un fichero escrito a mano</Span>.
    </Text>

    <Table
      columns={[{ width: 46 }, { width: 'auto' }, { width: 90, align: 'end' }]}
      header={{
        cells: [{ text: 'Ref.' }, { text: 'Concepto' }, { text: 'Importe' }],
        style: { background: NAVY },
      }}
      rows={[
        ...items.map((item) => ({
          cells: [{ text: item.ref }, { text: item.concept }, { text: euros(item.price) }],
        })),
        {
          cells: [
            { text: '' },
            { text: 'TOTAL', weight: 'bold' as const, color: NAVY },
            {
              text: euros(items.reduce((sum, i) => sum + i.price, 0)),
              weight: 'bold' as const,
              color: NAVY,
            },
          ],
          style: { background: '#e8eef4' },
        },
      ]}
      padding={5}
      spaceAfter={18}
    />

    <Text size={11} spaceAfter={6}>
      <B>Condiciones</B>
    </Text>
    <List
      marker="decimal"
      size={9}
      items={[
        'Pago por transferencia en un plazo de treinta días desde la emisión.',
        'El soporte prioritario se factura por bolsa y no es acumulable.',
        'La licencia se renueva tácitamente salvo aviso con un mes de antelación.',
      ]}
    />
  </Document>
);

// Declared after `Invoice` uses it only for spacing, so it stays out of the
// example's way; a spacer with no height would be dropped by the engine.
function Spacer() {
  return <Box spaceAfter={16} />;
}

describe('React to PDF', () => {
  it('prints a document nobody wrote a line of Rust or JSON for', async () => {
    const ir = await render(<Invoice number="FV-2026-00418" />);

    const out = await toPdf(ir, assets);

    expect(Buffer.from(out.pdf.subarray(0, 5)).toString()).toBe('%PDF-');
    expect(out.pages).toBe(1);
    expect(out.diagnostics).toEqual([]);
  });

  it('lays out the same document the same way twice', async () => {
    // The engine promises byte-identical output for identical input, and a
    // producer that shuffles keys or object identity would break it without
    // ever producing a wrong page.
    const once = await toPdf(await render(<Invoice number="FV-1" />), assets);
    const twice = await toPdf(await render(<Invoice number="FV-1" />), assets);

    expect(Buffer.from(once.pdf).equals(Buffer.from(twice.pdf))).toBe(true);
  });

  it('paginates a long table and carries its header over', async () => {
    const many = Array.from({ length: 400 }, (_, i) => ({
      cells: [{ text: `${i}` }, { text: `Asiento contable número ${i}` }, { text: '1.200,00 €' }],
    }));

    const out = await toPdf(
      await render(
        <Document>
          <Table
            columns={[{ width: 46 }, { width: 'auto' }, { width: 90, align: 'end' }]}
            header={{ cells: [{ text: 'Ref.' }, { text: 'Concepto' }, { text: 'Importe' }] }}
            rows={many}
          />
        </Document>,
      ),
      assets,
    );

    expect(out.pages).toBeGreaterThan(5);
    expect(out.diagnostics).toEqual([]);
  });

  it('prints what Tailwind classes asked for', async () => {
    // The test that was missing when the React side invented its own shape
    // for a border and nothing noticed: unit tests on either side agreed
    // with themselves, and only the engine could say they were wrong.
    const out = await toPdf(
      await render(
        <Theme colors={{ brand: '#1b3a5c' }}>
          <Document className="p-10">
            <Box className="bg-slate-50 border border-slate-300 p-4 mb-4">
              <Text className="text-sm text-brand font-bold">Panel</Text>
            </Box>
            <Text className="text-xs text-slate-500">Nota al pie</Text>
          </Document>
        </Theme>,
      ),
      assets,
    );

    expect(out.pages).toBe(1);
    expect(out.diagnostics).toEqual([]);
  });

  it('refuses a class the engine cannot honour, before printing anything', async () => {
    const flexed = render(
      <Document>
        <Box className="flex items-center" />
      </Document>,
    );

    await expect(flexed).rejects.toThrow(/flex/);
  });

  it('prints a rounded panel', async () => {
    const out = await toPdf(
      await render(
        <Document>
          <Box className="bg-slate-100 border border-slate-300 rounded-lg p-4">
            <Text className="text-sm">Panel</Text>
          </Box>
        </Document>,
      ),
      assets,
    );

    expect(out.pages).toBe(1);
    expect(out.diagnostics).toEqual([]);
  });

  it('says when only part of a rounded box will follow the corner', async () => {
    // A radius with a rule on one side: the background rounds and the rule
    // does not. Reasonable, but not obvious, so the engine says so.
    const out = await toPdf(
      await render(
        <Document>
          <Box className="bg-slate-100 border-b border-slate-300 rounded-lg p-4">
            <Text className="text-sm">Panel</Text>
          </Box>
        </Document>,
      ),
      assets,
    );

    expect(out.diagnostics.join(' ')).toContain('square-corner');
  });

  it('tells the author when the fonts cannot set what they wrote', async () => {
    const out = await toPdf(
      await render(
        <Document>
          <Text>日本語</Text>
        </Document>,
      ),
      assets,
    );

    expect(out.diagnostics.join(' ')).toContain('missing-glyph');
  });
});
