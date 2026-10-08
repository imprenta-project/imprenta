import { describe, expect, it } from 'vitest';
import {
  Anchor,
  Document,
  Footer,
  Link,
  PageOf,
  Section,
  Text,
  toDocument,
} from '../src/pdf/index.js';

const childrenOf = async (element: Parameters<typeof toDocument>[0]) =>
  (await toDocument(<Document>{element}</Document>)).children;

describe('sections', () => {
  it('say only what differs from the document, and take a band away with false', async () => {
    const [cover] = await childrenOf(
      <Section margin={56} footer={false} numbering="none">
        <Text>Portada</Text>
      </Section>,
    );

    expect(cover).toEqual({
      t: 'section',
      page: { margin: { top: 56, right: 56, bottom: 56, left: 56 } },
      footer: null,
      numbering: 'none',
      children: [{ t: 'text', runs: [{ text: 'Portada' }] }],
    });
  });

  it('inherit a band they say nothing about, and take one declared inside them', async () => {
    const [chapter] = await childrenOf(
      <Section numbering={{ restart: 1 }}>
        <Footer>
          <Text>Anexo</Text>
        </Footer>
        <Text>Uno</Text>
      </Section>,
    );

    expect(chapter).toEqual({
      t: 'section',
      footer: { children: [{ t: 'text', runs: [{ text: 'Anexo' }] }] },
      numbering: { restart: 1 },
      children: [{ t: 'text', runs: [{ text: 'Uno' }] }],
    });
  });

  it('take a page size by name, as the document does', async () => {
    const [landscape] = await childrenOf(
      <Section size="A4" landscape>
        <Text>Ancho</Text>
      </Section>,
    );

    expect(landscape.page).toEqual({ width: 841.8898, height: 595.2756 });
  });
});

describe('places in the document', () => {
  it('names a place, with an outline entry when given a bookmark', async () => {
    const children = await childrenOf(
      <>
        <Anchor id="resumen" bookmark="Resumen ejecutivo" />
        <Anchor id="detalle" bookmark="Detalle" level={2} />
        <Anchor id="nota" />
      </>,
    );

    expect(children).toEqual([
      { t: 'anchor', id: 'resumen', bookmark: 'Resumen ejecutivo' },
      { t: 'anchor', id: 'detalle', bookmark: 'Detalle', level: 2 },
      { t: 'anchor', id: 'nota' },
    ]);
  });

  it('prints the page a place landed on, and links to it', async () => {
    const [link] = await childrenOf(
      <Link href="#resumen">
        <Text>
          Resumen, página <PageOf id="resumen" />
        </Text>
      </Link>,
    );

    expect(link).toEqual({
      t: 'link',
      href: '#resumen',
      child: { t: 'text', runs: [{ text: 'Resumen, página {{pageof:resumen}}' }] },
    });
  });
});
