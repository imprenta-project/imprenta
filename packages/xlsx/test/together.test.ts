import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

/**
 * Both addons in one process, which is the ordinary case.
 *
 * A service that returns an invoice and also exports a spreadsheet loads
 * `@imprentajs/pdf` and `@imprentajs/xlsx` together, and the two are separate
 * native libraries. That went wrong once and went wrong loudly: the xlsx
 * addon had copied the PDF one's `#[global_allocator]`, two static allocators
 * ended up in one process, and the *first* PDF render after both were loaded
 * aborted the process with "the font contains no usable family". Not an
 * exception — an abort, because a panic inside a napi task cannot unwind.
 *
 * It was order-dependent, which is what gave it away: loading xlsx first and
 * pdf second worked, and the other way round did not. Nothing about the fonts
 * was wrong at any point.
 *
 * Both orders are tested because only one of them failed.
 */

const fixtures = fileURLToPath(new URL('../../../crates/imprenta-pdf/tests', import.meta.url));
const roboto = readFileSync(`${fixtures}/fonts/Roboto-Regular.ttf`);

const DOCUMENT = JSON.stringify({
  page: { width: 595, height: 842 },
  children: [{ t: 'text', runs: [{ text: 'Una factura corriente' }] }],
});

const WORKBOOK = JSON.stringify({
  sheets: [{ name: 'Libro', rows: [{ cells: [{ value: { t: 'number', v: 1200 } }] }] }],
});

async function printThenExport() {
  const { render } = await import('@imprentajs/pdf');
  const { write } = await import('@imprentajs/xlsx');
  return { render, write };
}

async function exportThenPrint() {
  const { write } = await import('@imprentajs/xlsx');
  const { render } = await import('@imprentajs/pdf');
  return { render, write };
}

describe('the two addons in one process', () => {
  it('prints a document after the spreadsheet writer has been loaded', async () => {
    // The order that used to abort. If this ever fails again it will not fail
    // as an assertion — the process will die — so a red run here looks like a
    // crashed worker rather than a failed expectation.
    const { render, write } = await printThenExport();

    const printed = await render(DOCUMENT, { fonts: [{ weight: 'regular', data: roboto }] });
    const exported = await write(WORKBOOK);

    expect(printed.pages).toBe(1);
    expect(exported.sheets).toBe(1);
  });

  it('prints a document when the writer was loaded first', async () => {
    const { render, write } = await exportThenPrint();

    const exported = await write(WORKBOOK);
    const printed = await render(DOCUMENT, { fonts: [{ weight: 'regular', data: roboto }] });

    expect(exported.sheets).toBe(1);
    expect(printed.pages).toBe(1);
  });

  it('keeps printing after several turns of both', async () => {
    // The corruption showed on the first render after both were loaded, but
    // nothing said it had to. Alternating is cheap insurance.
    const { render, write } = await printThenExport();

    for (let turn = 0; turn < 3; turn += 1) {
      const printed = await render(DOCUMENT, { fonts: [{ weight: 'regular', data: roboto }] });
      const exported = await write(WORKBOOK);
      expect(printed.pages).toBe(1);
      expect(exported.sheets).toBe(1);
    }
  });
});
