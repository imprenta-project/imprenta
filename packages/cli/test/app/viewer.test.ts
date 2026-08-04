import { describe, expect, it } from 'vitest';
import { bytesFor, fillers, fit, pageName, type Rendered } from '../../app/src/viewer.js';

const rendered = (over: Partial<Rendered> = {}): Rendered => ({
  id: 'ventas/factura',
  format: 'pdf',
  url: 'blob:http://localhost/abc',
  ...over,
});

describe('which bytes the viewer is allowed to show', () => {
  it('shows the ones that were rendered for what is open', () => {
    expect(bytesFor(rendered(), 'ventas/factura', 'pdf')).toBe('blob:http://localhost/abc');
  });

  it('shows nothing while another document is still being rendered', () => {
    // The bug this exists for: picking a document set the report before the
    // bytes arrived, so for one frame the PDF viewer was pointed at the
    // previous document's blob. When that blob was a spreadsheet the browser
    // could not display it and **downloaded it instead** — a click on a file
    // in the sidebar quietly put a .xlsx in somebody's Downloads folder.
    expect(bytesFor(rendered({ id: 'hoja', format: 'xlsx' }), 'ventas/factura', 'pdf')).toBeNull();
  });

  it('shows nothing when the same document came back as the other format', () => {
    // A document declares its format by what it returns, so an edit can change
    // it without the name changing.
    expect(bytesFor(rendered(), 'ventas/factura', 'xlsx')).toBeNull();
  });

  it('shows nothing before anything has rendered', () => {
    expect(bytesFor(null, 'ventas/factura', 'pdf')).toBeNull();
  });
});

describe('fitting a page into the pane', () => {
  const a4 = { width: 595.2756, height: 841.8898 };

  it('keeps the page in its own proportions', () => {
    // The frame used to be the pane, so an A4 was drawn into whatever shape
    // the window happened to be and the viewer letterboxed it in grey. The
    // shadow went round the pane rather than round the sheet.
    expect(fit(a4).aspectRatio).toBe('595.2756 / 841.8898');
  });

  it('takes the smaller of the two fits, so the whole page is on screen', () => {
    // Height when the pane is tall and narrow, width when it is short and
    // wide. This is `object-fit: contain`, written in container units because
    // nothing here is a replaced element.
    expect(fit(a4).height).toBe('min(100cqh, calc(100cqw * 841.8898 / 595.2756))');
  });

  it('falls back to A4 when a document did not say', () => {
    // The engine's own default, so the frame and the page agree.
    expect(fit(undefined).aspectRatio).toBe('595.2756 / 841.8898');
  });
});

describe('filling the rest of the grid', () => {
  it('carries the grid on to the edge of the pane', () => {
    // A sheet eight columns wide used to stop at column H with the panel
    // colour beside it, which reads as the end of the window rather than the
    // end of the data. Excel rules the whole viewport, and so does this.
    expect(fillers(1000, 640)).toBe(5);
  });

  it('adds none when the sheet is already wider than the pane', () => {
    // There is a scrollbar in this case and nothing to fill.
    expect(fillers(600, 1400)).toBe(0);
  });

  it('leaves the last stripe to the spacer rather than overshooting', () => {
    // 300 spare is four whole columns and a bit. A fifth would push the sheet
    // past the pane and raise a horizontal scrollbar over nothing.
    expect(fillers(1000, 700)).toBe(4);
  });

  it('adds none before anything has been measured', () => {
    expect(fillers(0, 0)).toBe(0);
  });
});

describe('naming a page size', () => {
  it('names the ones a printer has a tray for', () => {
    expect(pageName({ width: 595.2756, height: 841.8898 })).toBe('A4');
    expect(pageName({ width: 612, height: 792 })).toBe('Letter');
    expect(pageName({ width: 792, height: 1224 })).toBe('Tabloid');
  });

  it('says when it is that size on its side', () => {
    expect(pageName({ width: 841.8898, height: 595.2756 })).toBe('A4 landscape');
  });

  it('gives millimetres for a size nobody named', () => {
    // Rounded, because 209.9997 mm is not a fact anybody needs.
    expect(pageName({ width: 283.46457, height: 283.46457 })).toBe('100 × 100 mm');
  });

  it('says nothing when there is no page to name', () => {
    expect(pageName(undefined)).toBeNull();
  });
});
