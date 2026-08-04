import type { Format, PageSetup } from './types.js';

/**
 * What the viewer is allowed to point at, and how big to draw it.
 *
 * Both of these were wrong in the same way: the pane knew what document was
 * selected and drew whatever bytes it happened to be holding, at whatever size
 * the window happened to be. Neither is a property of the window.
 */

/** A render that finished, and the document it finished for. */
export interface Rendered {
  id: string;
  format: Format;
  url: string;
}

/**
 * The bytes for this document, or nothing.
 *
 * Rendering is two round trips — the report, then the file — and the report
 * comes back first. For the frame in between, the viewer used to be pointed at
 * the previous document's blob. A PDF viewer handed a spreadsheet cannot show
 * it, so the browser did the next thing it knows: it downloaded it. Picking
 * documents in the sidebar left a trail of `.xlsx` in Downloads.
 */
export function bytesFor(
  rendered: Rendered | null,
  id: string | null,
  format: Format,
): string | null {
  if (!rendered || rendered.id !== id || rendered.format !== format) {
    return null;
  }
  return rendered.url;
}

/** The engine's own default, so an unset page and the frame agree. */
const A4: PageSetup = { width: 595.2756, height: 841.8898 };

/**
 * The page, at its own proportions, as large as the pane allows.
 *
 * `object-fit: contain` for something that is not a replaced element. The pane
 * declares `container-type: size`, so `100cqw` and `100cqh` are its content
 * box, and taking the smaller of the two fits is what keeps a whole page on
 * screen whichever way the window is dragged.
 */
export function fit(page: PageSetup | undefined): { aspectRatio: string; height: string } {
  const { width, height } = page ?? A4;
  return {
    aspectRatio: `${width} / ${height}`,
    height: `min(100cqh, calc(100cqw * ${height} / ${width}))`,
  };
}

/**
 * How wide an empty column is drawn, in CSS pixels.
 *
 * The same `min-width` a cell has in `Grid.tsx`. A filler has no content to
 * widen it, so this is exactly what it comes out at.
 */
export const FILLER_WIDTH = 72;

/**
 * How many empty columns to put after the last one the sheet declares.
 *
 * Floor rather than round: one column too many is a horizontal scrollbar over
 * nothing, and the few pixels left over go to a spacer cell that takes the
 * slack. Excel rules its whole viewport and a grid that stops mid-window reads
 * as the end of the window rather than the end of the data.
 */
export function fillers(paneWidth: number, sheetWidth: number): number {
  const spare = paneWidth - sheetWidth;
  return spare > 0 ? Math.floor(spare / FILLER_WIDTH) : 0;
}

/** The sizes a printer has a tray for, in points. */
const NAMED: [string, number, number][] = [
  ['A3', 841.8898, 1190.5512],
  ['A4', 595.2756, 841.8898],
  ['A5', 419.5276, 595.2756],
  ['Letter', 612, 792],
  ['Legal', 612, 1008],
  ['Tabloid', 792, 1224],
];

/** Within a point, because a page set in millimetres never lands exactly. */
const near = (a: number, b: number) => Math.abs(a - b) < 1;

const MM = 25.4 / 72;

/**
 * What to call this page.
 *
 * Worth showing because it is the one number the preview knows and the author
 * cannot see: a document that came out Letter when they meant A4 looks exactly
 * like a document that came out A4.
 */
export function pageName(page: PageSetup | undefined): string | null {
  if (!page) {
    return null;
  }
  for (const [name, width, height] of NAMED) {
    if (near(page.width, width) && near(page.height, height)) {
      return name;
    }
    if (near(page.width, height) && near(page.height, width)) {
      return `${name} landscape`;
    }
  }
  return `${Math.round(page.width * MM)} × ${Math.round(page.height * MM)} mm`;
}
