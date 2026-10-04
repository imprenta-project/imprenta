import {
  BarCode,
  Column,
  Cut,
  EscPos,
  Feed,
  Init,
  QrCode,
  Row,
  Rule,
  Text,
} from '@imprentajs/react/escpos';

interface Product {
  name: string;
  quantity: number;
  price: number;
}

interface TicketProps {
  number: string;
  date: string;
  products: Product[];
  paid: number;
}

const money = (cents: number) => `${(cents / 100).toFixed(2).replace('.', ',')} €`;

export default function Ticket({ number, date, products, paid }: TicketProps) {
  const total = products.reduce((sum, product) => sum + product.quantity * product.price, 0);
  // Sample prices include 7% IGIC; all monetary calculations use integer cents.
  const base = Math.round(total / 1.07);

  return (
    <EscPos profile={{ columns: 48, codepage: 'cp858', dpi: 203 }}>
      <Init width={64} marginLeft={4} />
      <Feed />
      <Text justification="center" bold fontWidth={2} fontHeight={2} lineSpacing={56}>
        MERCADO CANARIO
      </Text>
      <Text justification="center" font="B" lineSpacing={24}>
        Productos de aquí, cada día
      </Text>
      <Text justification="center" font="B" lineSpacing={24}>
        C/ La Marina, 12 · Santa Cruz
      </Text>
      <Text justification="center" font="B" lineSpacing={24}>
        NIF: B12345678
      </Text>
      <Feed />
      <Text bold lineSpacing={28}>
        Ticket: {number}
      </Text>
      <Text font="B" lineSpacing={24}>
        Fecha: {date}
      </Text>
      <Text font="B" lineSpacing={24}>
        Caja: 02 · Atendido por: Ana
      </Text>
      <Feed />
      <Row bold font="B" lineSpacing={24}>
        <Column>ARTÍCULO</Column>
        <Column width={5} justification="right">
          UD.
        </Column>
        <Column width={11} justification="right">
          IMPORTE
        </Column>
      </Row>
      {products.map((product) => (
        <Row key={product.name} lineSpacing={28}>
          <Column>{product.name}</Column>
          <Column width={5} justification="right">
            {product.quantity}
          </Column>
          <Column width={11} justification="right">
            {money(product.quantity * product.price)}
          </Column>
        </Row>
      ))}
      <Rule lineSpacing={24} />
      <Row font="B" lineSpacing={24}>
        <Column>Base imponible</Column>
        <Column width={11} justification="right">
          {money(base)}
        </Column>
      </Row>
      <Row font="B" lineSpacing={24}>
        <Column>IGIC incluido (7%)</Column>
        <Column width={11} justification="right">
          {money(total - base)}
        </Column>
      </Row>
      <Feed />
      <Row bold fontWidth={2} fontHeight={2} lineSpacing={56}>
        <Column>TOTAL</Column>
        <Column width={11} justification="right">
          {money(total)}
        </Column>
      </Row>
      <Row font="B" lineSpacing={24}>
        <Column>Efectivo</Column>
        <Column width={11} justification="right">
          {money(paid)}
        </Column>
      </Row>
      <Row font="B" lineSpacing={24}>
        <Column>Cambio</Column>
        <Column width={11} justification="right">
          {money(paid - total)}
        </Column>
      </Row>
      <Feed lines={2} />
      <Text justification="center" bold lineSpacing={28}>
        ¡Gracias por tu compra!
      </Text>
      <Text justification="center" font="B" lineSpacing={24}>
        Conserva este ticket para cambios.
      </Text>
      <Feed />
      <Text justification="center" font="B" lineSpacing={24}>
        Consulta tu ticket digital
      </Text>
      <QrCode justification="center" size={5} errorCorrectionLevel="M">
        {`https://example.com/tickets/${number}`}
      </QrCode>
      <Feed />
      <BarCode type="code128" justification="center" height={60} hriPosition="below-bar-code">
        {number}
      </BarCode>
      <Feed lines={3} />
      <Cut mode="partial" />
    </EscPos>
  );
}

Ticket.PreviewProps = {
  number: 'TC-2026-0042',
  date: '04/10/2026 12:35',
  products: [
    { name: 'Café de especialidad 250 g', quantity: 1, price: 895 },
    { name: 'Pan de masa madre', quantity: 2, price: 320 },
    { name: 'Mermelada artesanal de plátano de Canarias', quantity: 1, price: 475 },
    { name: 'Bolsa de papel', quantity: 1, price: 10 },
  ],
  paid: 2500,
} satisfies TicketProps;
