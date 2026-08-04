import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { render as toPdf } from '@imprentajs/pdf';
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

const fixtures = fileURLToPath(new URL('../../../crates/imprenta-pdf/tests', import.meta.url));
const font = (n: string) => readFileSync(join(fixtures, 'fonts', n));

const items = [
  { ref: '001', concept: 'Licencia anual Imprenta Server, plan profesional', price: 1200 },
  { ref: '002', concept: 'Implantación y migración de las plantillas existentes', price: 3400 },
  { ref: '003', concept: 'Soporte prioritario 24×7, bolsa de 40 horas', price: 2800 },
];
const euros = (n: number) => `${n.toLocaleString('es-ES', { minimumFractionDigits: 2 })} €`;
const total = items.reduce((sum, i) => sum + i.price, 0);

/** Everything here is styled with Tailwind classes. No numbers, no hexes. */
const Invoice = ({ number }: { number: string }) => (
  <Theme colors={{ brand: '#1b3a5c', 'brand-soft': '#e8eef4' }}>
    <Document className="p-10">
      <Row>
        <Image src="logo" width={108} />
        <Box className="w-32" />
        <Box className="w-52">
          <Text className="text-2xl text-brand font-bold">FACTURA</Text>
          <Text className="text-base font-bold">{number}</Text>
          <Text className="text-xs text-slate-400 mb-6">Emitida el 2 de agosto de 2026</Text>
        </Box>
      </Row>

      <Text className="text-sm text-slate-700 mb-4">
        Declarada en <B>React</B> y estilada con <B>Tailwind</B>. Las utilidades resuelven a la
        estructura de estilo del motor:{' '}
        <Span className="text-brand">ni CSS, ni cascada, ni hoja de estilos</Span>.
      </Text>

      <Box className="bg-slate-50 border border-slate-200 rounded-lg p-4 mb-5">
        <Text className="text-xs text-slate-500">
          <B>rounded-lg border p-4 bg-slate-50</B> — esta caja no lleva un solo número escrito a
          mano.
        </Text>
      </Box>

      <Table
        columns={[{ width: 46 }, { width: 'auto' }, { width: 90, align: 'end' }]}
        header={{
          cells: [
            { text: 'Ref.', color: '#ffffff', weight: 'bold' },
            { text: 'Concepto', color: '#ffffff', weight: 'bold' },
            { text: 'Importe', color: '#ffffff', weight: 'bold' },
          ],
          style: { background: '#1b3a5c' },
        }}
        rows={[
          ...items.map((item) => ({
            cells: [{ text: item.ref }, { text: item.concept }, { text: euros(item.price) }],
          })),
          {
            cells: [
              { text: '' },
              { text: 'TOTAL', weight: 'bold' as const, color: '#1b3a5c' },
              { text: euros(total), weight: 'bold' as const, color: '#1b3a5c' },
            ],
            style: { background: '#e8eef4' },
          },
        ]}
        padding={5}
        spaceAfter={20}
      />

      <Text className="text-base font-bold mb-2">Condiciones</Text>
      <List
        marker="decimal"
        className="text-sm text-slate-700"
        items={[
          'Pago por transferencia en un plazo de treinta días desde la emisión.',
          'El soporte prioritario se factura por bolsa y no es acumulable entre periodos.',
          'La licencia se renueva tácitamente salvo aviso con un mes de antelación.',
        ]}
      />
    </Document>
  </Theme>
);

const out = process.argv[2] ?? '.';
const result = await toPdf(await render(<Invoice number="FV-2026-00418" />), {
  fonts: [
    { weight: 'regular', data: font('Roboto-Regular.ttf') },
    { weight: 'bold', data: font('Roboto-Bold.ttf') },
  ],
  images: [{ name: 'logo', data: readFileSync(join(fixtures, 'images', 'logo.png')) }],
});
writeFileSync(join(out, 'from-react.pdf'), result.pdf);
console.log(
  `${out}/from-react.pdf: ${result.pages} page(s), ${(result.bytes / 1024).toFixed(1)} KB`,
);
if (result.diagnostics.length) console.log(result.diagnostics.join('\n'));
