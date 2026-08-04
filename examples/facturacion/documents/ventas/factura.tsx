import {
  B,
  Box,
  Document,
  Footer,
  Image,
  List,
  PageCount,
  PageNumber,
  Row,
  Span,
  Table,
  Text,
  Theme,
} from '@imprentajs/react/pdf';

interface Line {
  ref: string;
  concept: string;
  price: number;
}

interface Props {
  number: string;
  issued: string;
  lines: Line[];
}

const euros = (n: number) => `${n.toLocaleString('es-ES', { minimumFractionDigits: 2 })} €`;

export default function Factura({ number, issued, lines }: Props) {
  const total = lines.reduce((sum, line) => sum + line.price, 0);

  return (
    <Theme colors={{ brand: '#1b3a5c', 'brand-soft': '#e8eef4' }}>
      <Document className="p-10">
        <Footer height={22}>
          <Text className="text-xs text-slate-500">
            {number} · Página <PageNumber /> de <PageCount />
          </Text>
        </Footer>

        <Row>
          <Image src="logo" width={108} />
          <Box className="w-32" />
          <Box className="w-52">
            <Text className="text-2xl text-brand font-bold">FACTURA</Text>
            <Text className="text-base font-bold">{number}</Text>
            <Text className="text-xs text-slate-500 mb-6">Emitida el {issued}</Text>
          </Box>
        </Row>

        <Text className="text-sm text-slate-700 mb-4">
          Este documento se declara en <B>React</B> y se estila con <B>Tailwind</B>.{' '}
          <Span className="text-brand">Guarda el fichero y la vista se actualiza sola.</Span>
        </Text>

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
            ...lines.map((line) => ({
              cells: [{ text: line.ref }, { text: line.concept }, { text: euros(line.price) }],
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
            'La licencia se renueva tácitamente salvo aviso con un mes de antelación.',
          ]}
        />
      </Document>
    </Theme>
  );
}

/**
 * What the preview renders it with.
 *
 * Sample data lives beside the document and ships nowhere: the preview does
 * `<Factura {...Factura.PreviewProps} />` and production passes real data.
 */
Factura.PreviewProps = {
  number: 'FV-2026-00418',
  issued: '2 de agosto de 2026',
  lines: [
    { ref: '001', concept: 'Licencia anual Imprenta Server, plan profesional', price: 1200 },
    { ref: '002', concept: 'Implantación y migración de las plantillas existentes', price: 3400 },
    { ref: '003', concept: 'Soporte prioritario 24×7, bolsa de 40 horas', price: 2800 },
  ],
} satisfies Props;
