import { describe, expect, it } from 'vitest';
import { B, Box, Document, Text, Theme, toDocument } from '../src/pdf/index.js';
import type { IrNode } from '../src/pdf/ir.js';

const only = async (element: Parameters<typeof toDocument>[0]): Promise<IrNode> =>
  (await toDocument(element)).children[0];

describe('className', () => {
  it('styles a box', async () => {
    const box = await only(
      <Document>
        <Box className="bg-slate-100 p-3 border-b border-slate-300" />
      </Document>,
    );

    expect(box).toEqual({
      t: 'box',
      style: {
        background: '#f1f5f9',
        padding: { top: 9, right: 9, bottom: 9, left: 9 },
        border: { bottom: { width: 0.75, color: '#cad5e2' } },
      },
    });
  });

  it('styles a paragraph', async () => {
    const text = await only(
      <Document>
        <Text className="text-sm text-slate-700">Hola</Text>
      </Document>,
    );

    expect(text).toEqual({
      t: 'text',
      runs: [{ text: 'Hola' }],
      style: { size: 10.5, color: '#314158' },
    });
  });

  it('makes a whole paragraph bold, the way inheritance would', async () => {
    // `font-bold` on the paragraph is not a paragraph property in the IR —
    // weight lives on runs — so it has to become the style the runs start
    // from, and a run that says otherwise still wins.
    const text = await only(
      <Document>
        <Text className="font-bold italic">Total</Text>
      </Document>,
    );

    expect(text).toMatchObject({ runs: [{ text: 'Total', weight: 'bold', italic: true }] });
  });

  it('lets a nested stretch keep what it adds', async () => {
    const text = await only(
      <Document>
        <Text className="italic">
          por <B>7.400,00 €</B>
        </Text>
      </Document>,
    );

    expect(text).toMatchObject({
      runs: [
        { text: 'por ', italic: true },
        { text: '7.400,00 €', italic: true, weight: 'bold' },
      ],
    });
  });

  it('styles an inline stretch of its own', async () => {
    const text = await only(
      <Document>
        <Text>
          estado <B className="text-red-600">vencida</B>
        </Text>
      </Document>,
    );

    expect(text).toMatchObject({
      runs: [{ text: 'estado ' }, { text: 'vencida', weight: 'bold', color: '#e7000b' }],
    });
  });

  it('lets an explicit prop override the class that set it', async () => {
    // The prop is the more specific of the two and the typed one, so it wins.
    const text = await only(
      <Document>
        <Text className="text-sm" size={20}>
          Hola
        </Text>
      </Document>,
    );

    expect(text).toMatchObject({ style: { size: 20 } });
  });

  it('takes the page margin from a class', async () => {
    const document = await toDocument(<Document className="p-8" />);

    expect(document.page.margin).toEqual({ top: 24, right: 24, bottom: 24, left: 24 });
  });

  it('says which class it could not use, and where', async () => {
    // Not "unknown utility" three hundred pages later.
    const bad = toDocument(
      <Document>
        <Box className="p-4 backdrop-blur" />
      </Document>,
    );

    await expect(bad).rejects.toThrow(/backdrop-blur/);
  });
});

describe('Theme', () => {
  it('gives the document its own colours', async () => {
    const text = await only(
      <Document>
        <Theme colors={{ brand: '#1b3a5c' }}>
          <Text className="text-brand">FACTURA</Text>
        </Theme>
      </Document>,
    );

    expect(text).toMatchObject({ style: { color: '#1b3a5c' } });
  });

  it('applies to everything below it, however deep', async () => {
    const box = await only(
      <Document>
        <Theme colors={{ brand: '#1b3a5c' }}>
          <Box className="bg-brand">
            <Text className="text-brand">FACTURA</Text>
          </Box>
        </Theme>
      </Document>,
    );

    expect(JSON.stringify(box)).toContain('#1b3a5c');
    expect((box as unknown as { style: { background: string } }).style.background).toBe('#1b3a5c');
  });

  it('rescales the whole document at once', async () => {
    const text = await only(
      <Document>
        <Theme ptPerRem={10}>
          <Text className="text-base">Hola</Text>
        </Theme>
      </Document>,
    );

    expect(text).toMatchObject({ style: { size: 10 } });
  });

  it('nests, with the inner one adding to the outer', async () => {
    const text = await only(
      <Document>
        <Theme colors={{ brand: '#1b3a5c' }}>
          <Theme colors={{ accent: '#c0392b' }}>
            <Text className="text-brand">
              <B className="text-accent">a</B>
            </Text>
          </Theme>
        </Theme>
      </Document>,
    );

    expect(text).toMatchObject({
      style: { color: '#1b3a5c' },
      runs: [{ text: 'a', color: '#c0392b' }],
    });
  });

  it('can sit outside the document and theme all of it', async () => {
    // Where an author would naturally put it: once, at the top, round
    // everything — including the page setup.
    const document = await toDocument(
      <Theme colors={{ brand: '#1b3a5c' }} ptPerRem={10}>
        <Document className="p-4">
          <Text className="text-brand text-base">FACTURA</Text>
        </Document>
      </Theme>,
    );

    expect(document.page.margin).toEqual({ top: 10, right: 10, bottom: 10, left: 10 });
    expect(document.children[0]).toMatchObject({ style: { color: '#1b3a5c', size: 10 } });
  });

  it('draws nothing of its own', async () => {
    const document = await toDocument(
      <Document>
        <Theme colors={{ brand: '#1b3a5c' }}>
          <Text>a</Text>
          <Text>b</Text>
        </Theme>
      </Document>,
    );

    expect(document.children).toHaveLength(2);
    expect(document.children.every((c) => c.t === 'text')).toBe(true);
  });
});
