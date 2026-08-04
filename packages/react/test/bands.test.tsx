import { describe, expect, it } from 'vitest';
import {
  B,
  Document,
  Footer,
  Header,
  PageCount,
  PageNumber,
  RunningTotal,
  Text,
  toDocument,
} from '../src/pdf/index.js';

describe('bands', () => {
  it('lifts a header out of the children', async () => {
    // A band is not part of the flow: it belongs to the page, and leaving it
    // among the children would print it once, at the top of page one.
    const document = await toDocument(
      <Document>
        <Header height={40}>
          <Text>Libro mayor</Text>
        </Header>
        <Text>Contenido</Text>
      </Document>,
    );

    expect(document.children).toHaveLength(1);
    expect(document.header).toEqual({
      height: 40,
      children: [{ t: 'text', runs: [{ text: 'Libro mayor' }] }],
    });
  });

  it('lifts a footer the same way', async () => {
    const document = await toDocument(
      <Document>
        <Text>Contenido</Text>
        <Footer height={24}>
          <Text>Pie</Text>
        </Footer>
      </Document>,
    );

    expect(document.children).toHaveLength(1);
    expect(document.footer?.height).toBe(24);
  });

  it('leaves a document with no bands alone', async () => {
    const document = await toDocument(
      <Document>
        <Text>Contenido</Text>
      </Document>,
    );

    expect(document.header).toBeUndefined();
    expect(document.footer).toBeUndefined();
  });

  it('refuses a second header rather than quietly dropping one', async () => {
    const two = toDocument(
      <Document>
        <Header height={10} />
        <Header height={20} />
      </Document>,
    );

    await expect(two).rejects.toThrow(/one header/i);
  });

  describe('what a page knows', () => {
    it('writes the page number as the token the engine fills in', async () => {
      const document = await toDocument(
        <Document>
          <Footer height={20}>
            <Text>
              Página <PageNumber /> de <PageCount />
            </Text>
          </Footer>
        </Document>,
      );

      expect(document.footer?.children[0]).toEqual({
        t: 'text',
        runs: [{ text: 'Página {{page}} de {{pages}}' }],
      });
    });

    it('writes a running total, at the open or at the close', async () => {
      const document = await toDocument(
        <Document accumulators={['saldo']}>
          <Footer height={20}>
            <Text>
              <RunningTotal name="saldo" at="opening" /> → <RunningTotal name="saldo" />
            </Text>
          </Footer>
        </Document>,
      );

      expect(document.footer?.children[0]).toMatchObject({
        runs: [{ text: '{{opening:saldo}} → {{closing:saldo}}' }],
      });
    });

    it('keeps a token in the style it was written in', async () => {
      // `<B><PageNumber/></B>` has to come out bold, like any other stretch.
      const document = await toDocument(
        <Document>
          <Footer height={20}>
            <Text>
              Página{' '}
              <B>
                <PageNumber />
              </B>
            </Text>
          </Footer>
        </Document>,
      );

      expect(document.footer?.children[0]).toMatchObject({
        runs: [{ text: 'Página ' }, { text: '{{page}}', weight: 'bold' }],
      });
    });

    it('declares the accumulators a document keeps', async () => {
      const document = await toDocument(<Document accumulators={['saldo', 'iva']} />);

      expect(document.accumulators).toEqual(['saldo', 'iva']);
    });
  });

  it('takes a band styled with classes like anything else', async () => {
    const document = await toDocument(
      <Document>
        <Footer height={20}>
          <Text className="text-xs text-slate-500">Pie</Text>
        </Footer>
      </Document>,
    );

    expect(document.footer?.children[0]).toMatchObject({
      style: { size: 9, color: '#62748e' },
    });
  });
});
