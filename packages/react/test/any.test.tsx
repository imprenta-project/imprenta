import { describe, expect, it } from 'vitest';
import { renderAny } from '../src/any.js';
import { Theme } from '../src/element.js';
import { Document, Text } from '../src/pdf/elements.js';
import { Cell, Row, Sheet, Workbook } from '../src/xlsx/elements.js';

/**
 * What tooling gets when it does not know what it was handed.
 *
 * The CLI's whole problem: a `.tsx` default-exports a function, and which
 * format it declares is only knowable once that function has been called.
 */
describe('renderAny', () => {
  it('recognises a document and gives back the page IR', async () => {
    const out = await renderAny(
      <Document margin={40}>
        <Text>Hola</Text>
      </Document>,
    );

    expect(out.format).toBe('pdf');
    expect(out.ir).toHaveProperty('page');
  });

  it('recognises a workbook and gives back the sheet IR', async () => {
    const out = await renderAny(
      <Workbook>
        <Sheet name="H">
          <Row>
            <Cell value={1200} />
          </Row>
        </Sheet>
      </Workbook>,
    );

    expect(out.format).toBe('xlsx');
    expect(out.ir).toHaveProperty('sheets');
  });

  it('sees through a theme wrapped round either of them', async () => {
    const wrapped = await renderAny(
      <Theme ptPerRem={14}>
        <Workbook>
          <Sheet name="H" />
        </Workbook>
      </Theme>,
    );

    expect(wrapped.format).toBe('xlsx');
  });

  it('names what it found when the root is neither', async () => {
    await expect(renderAny(<Text>suelto</Text>)).rejects.toThrow(
      /<Document> or a <Workbook>.*<text>/s,
    );
  });

  it('says so when a component returned nothing at all', async () => {
    const Empty = () => null;
    await expect(renderAny(<Empty />)).rejects.toThrow(
      /expects one <Document> or <Workbook>, and was given 0/,
    );
  });
});
