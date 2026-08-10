/**
 * @vitest-environment happy-dom
 *
 * Declared here rather than for the whole package: the server tests need a
 * real Node, and only these need a document to render into.
 */
import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { Grid } from '../../app/src/Grid.js';
import type { IrRow, IrWorkbook } from '../../app/src/types.js';

const text = (...values: string[]): IrRow => ({
  cells: values.map((v) => ({ value: { t: 'text' as const, v } })),
});

const book = (rows: IrRow[]): IrWorkbook => ({ sheets: [{ name: 'Hoja', rows }] });

const LABEL = /filters this column/i;

afterEach(cleanup);

describe('the grid, on a header that filters', () => {
  it('marks every label the autofilter covers', () => {
    // The grid is built from the IR and the IR carries a flag on the row, so
    // without this a header looked identical marked or not. The slip the
    // feature exists to prevent is marking the *wrong* row, and the preview is
    // the one place the author would catch that before opening the file.
    render(
      <Grid
        workbook={book([
          { filter: true, ...text('Fecha', 'Concepto', 'Importe') },
          text('01/08', 'Licencia', '1200'),
        ])}
      />,
    );

    expect(screen.getAllByTitle(LABEL)).toHaveLength(3);
  });

  it('marks nothing on a sheet nobody asked to filter', () => {
    // The whole thing has to cost nothing to every sheet not using it.
    render(<Grid workbook={book([text('Fecha', 'Concepto'), text('01/08', 'Licencia')])} />);

    expect(screen.queryAllByTitle(LABEL)).toHaveLength(0);
  });

  it('marks only the row that asked, not the rows under it', () => {
    render(
      <Grid
        workbook={book([
          text('MOVIMIENTOS'),
          { filter: true, ...text('Fecha', 'Concepto') },
          text('01/08', 'Licencia'),
        ])}
      />,
    );

    // Two labels, and neither the title row above nor the data row below.
    expect(screen.getAllByTitle(LABEL)).toHaveLength(2);
  });
});
