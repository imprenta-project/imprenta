import { createContext, useContext } from 'react';
import { describe, expect, it } from 'vitest';
import { renderAny } from '../src/any.js';
import {
  BarCode,
  Bytes,
  Column,
  Cut,
  EscPos,
  Feed,
  Image,
  Init,
  PDF417,
  QrCode,
  Row,
  Rule,
  render,
  Text,
  toReceipt,
} from '../src/escpos/index.js';

describe('React receipt authoring', () => {
  it('resolves reusable product components, fragments, conditionals and numbers', async () => {
    const Currency = createContext('€');
    function Product({ name, amount }: { name: string; amount: number }) {
      const currency = useContext(Currency);
      return (
        <Row>
          <Column>{name}</Column>
          <Column width={10} justification="right">
            {amount} {currency}
          </Column>
        </Row>
      );
    }
    const ir = await toReceipt(
      <Currency.Provider value="EUR">
        <EscPos profile={{ columns: 32 }}>
          <Text justification="center" bold>
            Shop {1}
          </Text>
          <>
            <Rule />
            <Product name="Olive oil" amount={12.5} />
            {false && <Text>hidden</Text>}
          </>
          <Feed lines={3} />
          <Cut mode="partial" />
        </EscPos>
      </Currency.Provider>,
    );
    expect(ir).toEqual({
      profile: { columns: 32 },
      children: [
        { type: 'text', text: 'Shop 1', justification: 'center', bold: true },
        { type: 'rule' },
        {
          type: 'row',
          columns: [{ text: 'Olive oil' }, { text: '12.5 EUR', width: 10, justification: 'right' }],
        },
        { type: 'feed', lines: 3 },
        { type: 'cut', mode: 'partial' },
      ],
    });
  });

  it('dispatches receipts from the same tooling entrypoint as PDF and XLSX', async () => {
    expect(
      await renderAny(
        <EscPos>
          <Text>Hello</Text>
        </EscPos>,
      ),
    ).toEqual({
      format: 'escpos',
      ir: { children: [{ type: 'text', text: 'Hello' }] },
    });
  });

  it('serializes the IR independently of the printer connection', async () => {
    const ticket = (
      <EscPos>
        <Text>España €</Text>
      </EscPos>
    );
    expect(JSON.parse(await render(ticket))).toEqual(await toReceipt(ticket));
  });

  it('supports the native code and image elements', async () => {
    const ir = await toReceipt(
      <EscPos>
        <Init width={51} dpi={203} />
        <Image src="logo" width={96} />
        <QrCode size={4}>https://example.com</QrCode>
        <BarCode type="code128">123456</BarCode>
        <PDF417>data</PDF417>
        <Bytes>1B 32</Bytes>
      </EscPos>,
    );
    expect(ir.children.map((node) => node.type)).toEqual([
      'init',
      'image',
      'qrcode',
      'barcode',
      'pdf417',
      'bytes',
    ]);
    expect(ir.children[3]).toEqual({ type: 'barcode', barcodeType: 'code128', value: '123456' });
  });

  it('rejects misplaced columns and foreign elements instead of ignoring them', async () => {
    await expect(
      toReceipt(
        <EscPos>
          <Column>wrong</Column>
        </EscPos>,
      ),
    ).rejects.toThrow(/Column|column/);
    await expect(
      toReceipt(
        <EscPos>
          <div>wrong</div>
        </EscPos>,
      ),
    ).rejects.toThrow(/div/);
    await expect(
      toReceipt(
        <EscPos>
          <Row>
            <Text>wrong</Text>
          </Row>
        </EscPos>,
      ),
    ).rejects.toThrow(/Column/);
  });

  it('rejects text outside a text element and nested markup', async () => {
    await expect(toReceipt(<EscPos>wrong</EscPos>)).rejects.toThrow(/Text/);
    await expect(
      toReceipt(
        <EscPos>
          <Text>
            <strong>wrong</strong>
          </Text>
        </EscPos>,
      ),
    ).rejects.toThrow(/strong/);
  });

  it('propagates errors from the author component', async () => {
    function Broken(): never {
      throw new Error('missing sale');
    }
    await expect(toReceipt(<Broken />)).rejects.toThrow('missing sale');
  });
});
