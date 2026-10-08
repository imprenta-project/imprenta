import { afterAll, describe, expect, it } from 'vitest';
import { close, type Receipt, render } from '../dist/index.js';

afterAll(close);

describe('native receipt commands', () => {
  it('initializes, encodes Spanish and euro in cp858, feeds and cuts', async () => {
    const result = await render({
      children: [
        { type: 'text', text: 'España 9,50 €' },
        { type: 'feed', lines: 3 },
        { type: 'cut', mode: 'partial' },
      ],
    });
    expect(result.escpos).toBeInstanceOf(Uint8Array);
    expect(Array.from(result.escpos.slice(0, 2))).toEqual([0x1b, 0x40]);
    expect(Buffer.from(result.escpos)).toContain(0xa4);
    expect(Buffer.from(result.escpos)).toContain(0xd5);
    expect(Buffer.from(result.escpos).includes(Buffer.from([0x1d, 0x56, 1]))).toBe(true);
    expect(result.bytes).toBe(result.escpos.length);
    expect(result.diagnostics).toEqual([]);
  });

  it('wraps product names without moving the price into their column', async () => {
    const result = await render({
      profile: { columns: 20 },
      children: [
        {
          type: 'row',
          gap: 1,
          columns: [
            { text: 'Extra virgin olive oil' },
            { text: '12.50', width: 6, justification: 'right' },
          ],
        },
      ],
    });
    expect(
      Buffer.from(result.escpos).includes(
        Buffer.from('Extra virgin   12.50\nolive oil           \n'),
      ),
    ).toBe(true);
  });

  it('splits long words, preserves explicit newlines and allocates flexible columns', async () => {
    const result = await render({
      profile: { columns: 8 },
      children: [
        {
          type: 'row',
          gap: 1,
          columns: [{ text: 'abcdefghij\nx' }, { text: '2', width: 2, justification: 'right' }],
        },
      ],
    });
    expect(Buffer.from(result.escpos).includes(Buffer.from('abcde  2\nfghij   \nx       \n'))).toBe(
      true,
    );
  });

  it('rejects a row with no room instead of truncating a price', async () => {
    await expect(
      render({
        profile: { columns: 8 },
        children: [
          { type: 'row', columns: [{ text: 'Product' }, { text: '123456789', width: 9 }] },
        ],
      }),
    ).rejects.toThrow(/row|columns|width/i);
  });

  it('reports characters a native printer cannot represent', async () => {
    const result = await render({ children: [{ type: 'text', text: 'Hello 🐈' }] });
    expect(result.diagnostics.join(' ')).toMatch(/unsupported-character/);
  });

  it('refuses control sequences inside customer data', async () => {
    await expect(render({ children: [{ type: 'text', text: 'name\u001b@' }] })).rejects.toThrow(
      /control/i,
    );
  });

  it('does not leak bold or scale into the following text', async () => {
    const result = await render({
      children: [
        { type: 'text', text: 'TOTAL', bold: true, fontWidth: 2, fontHeight: 2 },
        { type: 'text', text: 'Thank you' },
      ],
    });
    const bytes = Buffer.from(result.escpos);
    const following = bytes.indexOf('Thank you');
    expect(bytes.subarray(0, following).includes(Buffer.from([0x1b, 0x45, 0]))).toBe(true);
    expect(bytes.subarray(0, following).includes(Buffer.from([0x1d, 0x21, 0]))).toBe(true);
  });

  it('encodes native QR and barcode commands', async () => {
    const result = await render({
      children: [
        { type: 'qrcode', value: 'https://example.com/receipt/1' },
        { type: 'barcode', value: '123456789012', barcodeType: 'code128' },
      ],
    });
    const bytes = Buffer.from(result.escpos);
    expect(bytes.includes(Buffer.from([0x1d, 0x28, 0x6b]))).toBe(true);
    expect(bytes.includes(Buffer.from([0x1d, 0x6b]))).toBe(true);
  });

  it.each([0, -1, 3.5, NaN, Infinity])('rejects invalid columns %s', async (columns) => {
    await expect(render({ profile: { columns }, children: [] })).rejects.toThrow(/columns/);
  });

  it('accepts JSON from a queue as well as the typed representation', async () => {
    const receipt: Receipt = { children: [{ type: 'text', text: 'Receipt 1' }] };
    const expected = await render(receipt);
    expect((await render(JSON.stringify(receipt))).escpos).toEqual(expected.escpos);
    expect((await render(new TextEncoder().encode(JSON.stringify(receipt)))).escpos).toEqual(
      expected.escpos,
    );
  });

  it('rejects unknown nodes from untyped JSON', async () => {
    await expect(render('{"children":[{"type":"raw","value":"bad"}]}')).rejects.toThrow(/raw/);
  });

  it('requires configured images and names the missing asset', async () => {
    await expect(render({ children: [{ type: 'image', src: 'logo', width: 64 }] })).rejects.toThrow(
      /logo/,
    );
  });
});
