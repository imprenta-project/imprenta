import { B, Document, Footer, PageCount, PageNumber, Table, Text } from '@imprentajs/react/pdf';

export interface Line {
  ref: string;
  concept: string;
  price: number;
}

const euros = (n: number) => `${n.toLocaleString('es-ES', { minimumFractionDigits: 2 })} €`;

/**
 * The same component the preview shows and a controller renders.
 *
 * It says nothing about fonts: a document declares what it looks like, and
 * which files that is set in belongs to whoever is printing it. The same
 * invoice goes out in the brand's typeface from the server and in whatever
 * the preview is configured with on a laptop.
 */
export default function Factura({ number, lines }: { number: string; lines: Line[] }) {
  const total = lines.reduce((sum, line) => sum + line.price, 0);

  return (
    <Document className="p-10">
      <Footer height={20}>
        <Text className="text-xs text-slate-500">
          {number} · Página <PageNumber /> de <PageCount />
        </Text>
      </Footer>

      <Text className="text-2xl font-bold mb-1">FACTURA</Text>
      <Text className="text-base mb-6">
        <B>{number}</B>
      </Text>

      <Table
        columns={[{ width: 46 }, { width: 'auto' }, { width: 90, align: 'end' }]}
        header={{ cells: [{ text: 'Ref.' }, { text: 'Concepto' }, { text: 'Importe' }] }}
        rows={[
          ...lines.map((line) => ({
            cells: [{ text: line.ref }, { text: line.concept }, { text: euros(line.price) }],
          })),
          {
            cells: [
              { text: '' },
              { text: 'TOTAL', weight: 'bold' as const },
              { text: euros(total), weight: 'bold' as const },
            ],
          },
        ]}
        padding={5}
      />
    </Document>
  );
}
