import { useLayoutEffect, useRef, useState } from 'react';
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { cn } from '@/lib/utils';
import type { IrCell, IrPicture, IrRow, IrSheet, IrWorkbook, Side } from './types.js';
import { fillers } from './viewer.js';

/**
 * A workbook, shown as the grid it describes.
 *
 * The document preview shows the real PDF, because a browser can open one.
 * Nothing can open a spreadsheet, so this shows the next honest thing: the
 * grid built from **the same IR the writer is handed**, with the styles
 * resolved exactly as they will be written.
 *
 * That is a faithful view of the input, not a guess at how Excel will draw it.
 * Column widths are Excel's character units and row heights are points, and
 * neither is what a browser lays out in — so the shape here is indicative and
 * the file is the file. The download is one click away for that reason.
 */
export function Grid({ workbook }: { workbook: IrWorkbook }) {
  const [at, setAt] = useState(0);
  const sheet = workbook.sheets[at];

  if (!sheet) {
    return <p className="p-6 text-sm text-muted-foreground">This workbook has no sheets.</p>;
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      {workbook.sheets.length > 1 && (
        <Tabs
          value={String(at)}
          onValueChange={(value) => setAt(Number(value))}
          className="flex-none border-b bg-sidebar px-2"
        >
          <TabsList variant="line" className="h-8">
            {workbook.sheets.map((each, index) => (
              <TabsTrigger key={each.name} value={String(index)}>
                {each.name}
              </TabsTrigger>
            ))}
          </TabsList>
        </Tabs>
      )}
      <Pane sheet={sheet} />
    </div>
  );
}

/**
 * The scroller, and the only thing in this file that measures anything.
 *
 * How many empty columns fit beside the sheet is a question about the window,
 * so it cannot be answered from the IR. The real header cells are tagged and
 * summed — never the fillers — so adding one does not change the measurement
 * that decided to add it.
 */
function Pane({ sheet }: { sheet: IrSheet }) {
  const pane = useRef<HTMLDivElement>(null);
  const [fill, setFill] = useState(0);

  useLayoutEffect(() => {
    const node = pane.current;
    if (!node) {
      return;
    }
    const measure = () => {
      const declared = Array.from(node.querySelectorAll<HTMLElement>('thead th[data-declared]'));
      const used = declared.reduce((total, cell) => total + cell.getBoundingClientRect().width, 0);
      setFill(fillers(node.clientWidth, used));
    };
    measure();
    const watching = new ResizeObserver(measure);
    watching.observe(node);
    return () => watching.disconnect();
  }, []);

  return (
    <div ref={pane} className="min-h-0 flex-1 overflow-auto bg-muted">
      <Sheet sheet={sheet} fill={fill} />
    </div>
  );
}

/** Beyond this the browser is the bottleneck, not the engine. */
const SHOWN = 300;

/**
 * The `Table` from the component library is deliberately not used here.
 *
 * That one is a data table: row hover, a bottom rule per row, padding tuned
 * for prose. A spreadsheet is a ruled grid where every cell carries its own
 * borders and fills straight out of the IR, and those two sets of rules fight.
 * Using the wrong component because it is the one on the shelf would make the
 * grid a worse likeness of the file, which is the only thing it is for.
 */
function Sheet({ sheet, fill }: { sheet: IrSheet; fill: number }) {
  const rows = sheet.rows ?? [];
  const shown = rows.slice(0, SHOWN);
  const widest = rows.reduce((most, row) => Math.max(most, row.cells?.length ?? 0), 0);
  const merged = coverage(sheet);
  // A picture is not in the grid, it floats over it — so it is drawn by the
  // cell it hangs from and allowed to spill out. Without this the preview
  // showed a workbook the file does not contain: the engine writes the
  // letterhead and the author could not see it.
  const hanging = new Map((sheet.pictures ?? []).map((p) => [`${p.row}:${p.column}`, p]));
  // Empty columns carry on the letters, because they are the same sheet:
  // column I exists in the file, it just has nothing in it.
  const empty = Array.from({ length: fill }, (_, n) => name(widest + n));

  return (
    <>
      {/* Paper, in both modes. Everything around it is chrome and follows the
          theme; this is the artefact, and the fills the writer puts in a cell
          are chosen against a white sheet. Under a dark theme they would land
          beneath light text and a shaded row would come out white on white.

          `w-full` so the spacer column at the end of every row can take the
          slack the whole fillers could not — otherwise the last stripe of the
          pane stays the colour of the panel. */}
      <table className="w-full border-collapse bg-sheet text-xs text-sheet-foreground tabular-nums">
        <thead>
          <tr>
            <th className="sticky top-0 left-0 z-40 border border-border bg-muted" />
            {Array.from({ length: widest }, (_, column) => name(column)).map((letter) => (
              // Tagged, because the measurement that decides how many fillers
              // to draw must not include the fillers it drew last time.
              <th key={letter} data-declared className={cn(reference, 'sticky top-0 z-20')}>
                {letter}
              </th>
            ))}
            {empty.map((letter) => (
              <th key={letter} className={cn(reference, 'sticky top-0 z-20 min-w-[70px]')}>
                {letter}
              </th>
            ))}
            <th className={cn(reference, 'sticky top-0 z-20 w-full min-w-0')} />
          </tr>
        </thead>
        <tbody>
          {shown.map((row, index) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: a row's identity is its position in the sheet
            <tr key={index} style={{ height: row.height ? `${row.height}px` : undefined }}>
              <th className={cn(reference, 'sticky left-0 z-20 min-w-11')}>{index + 1}</th>
              {Array.from({ length: widest }, (_, column) => {
                const cover = merged.get(`${index}:${column}`);
                if (cover === 'covered') {
                  return null;
                }
                return (
                  <Cell
                    // biome-ignore lint/suspicious/noArrayIndexKey: likewise, a cell is where it is
                    key={column}
                    cell={row.cells?.[column]}
                    row={row}
                    span={cover}
                    picture={hanging.get(`${index}:${column}`)}
                  />
                );
              })}
              {empty.map((letter) => (
                <td key={letter} className={cn(blank, 'min-w-[70px]')} />
              ))}
              <td className={cn(blank, 'w-full min-w-0')} />
            </tr>
          ))}
        </tbody>
      </table>
      {rows.length > SHOWN && (
        <p className="px-4 pt-2.5 pb-4 text-xs text-muted-foreground">
          {`${(rows.length - SHOWN).toLocaleString('en')} more rows in the file — the grid stops at ${SHOWN} so the browser stays usable.`}
        </p>
      )}
    </>
  );
}

/** The A/B/C and 1/2/3 gutters, which are chrome and not part of the sheet. */
const reference =
  'border border-border bg-muted px-2 py-0.5 text-center text-[11px] font-medium text-muted-foreground';

/** A cell the sheet does not declare. Ruled like the rest, because it is. */
const blank = 'border border-paper-200 px-2 py-0.5';

type Cover = 'covered' | { colSpan: number; rowSpan: number };

/** Which cells a merge swallows, and which one carries the span. */
function coverage(sheet: IrSheet): Map<string, Cover> {
  const map = new Map<string, Cover>();
  for (const merge of sheet.merges ?? []) {
    for (let r = merge.fromRow; r <= merge.toRow; r += 1) {
      for (let c = merge.fromColumn; c <= merge.toColumn; c += 1) {
        map.set(
          `${r}:${c}`,
          r === merge.fromRow && c === merge.fromColumn
            ? {
                colSpan: merge.toColumn - merge.fromColumn + 1,
                rowSpan: merge.toRow - merge.fromRow + 1,
              }
            : 'covered',
        );
      }
    }
  }
  return map;
}

/**
 * A picture, over the grid rather than in it.
 *
 * Positioned out of the flow from the top-left of its anchor cell, exactly
 * where the anchor puts it, and allowed to overlap whatever is beside it.
 * Only the width is set — the height follows the image's own pixels, which is
 * the same rule the engine applies when it works out the extent.
 *
 * Points are used as pixels here, as row heights already are. A browser lays
 * a grid out in neither of Excel's units, so this is indicative and the file
 * is the file.
 */
function Picture({ picture }: { picture: IrPicture }) {
  // Placed with the browser's own centring rather than by copying the
  // engine's arithmetic. The cell here **is** the merged block — the grid
  // draws a merge as one `<td>` — so the browser already knows how big it is,
  // and a second implementation of Excel's character-to-point conversion is a
  // second place for the preview to disagree with the file.
  const [x, dx] = ACROSS[picture.align ?? 'start'];
  const [y, dy] = DOWN[picture.valign ?? 'start'];

  return (
    <img
      src={`/api/image?name=${encodeURIComponent(picture.image)}`}
      alt={picture.image}
      className="pointer-events-none absolute z-10 max-w-none"
      style={{
        left: x,
        top: y,
        width: `${picture.width}px`,
        transform: `translate(${dx}, ${dy}) translate(${picture.dx ?? 0}px, ${picture.dy ?? 0}px)`,
      }}
    />
  );
}

/** Where a picture starts, and how much of itself to pull back. */
const ACROSS = {
  start: ['0', '0'],
  center: ['50%', '-50%'],
  end: ['100%', '-100%'],
} as const;

const DOWN = ACROSS;

/** A number reads right and a string reads left, the way Excel shows them —
    which is what makes a number written as text visible at a glance. */
const ALIGNMENT: Record<string, string> = {
  number: 'text-right',
  date: 'text-right',
  bool: 'text-center text-muted-foreground',
  formula: 'text-signal-info',
};

function Cell({
  cell,
  row,
  span,
  picture,
}: {
  cell: IrCell | undefined;
  row: IrRow;
  span: Cover | undefined;
  picture?: IrPicture;
}) {
  // The most specific style wins whole, which is what the writer does too.
  const style = cell?.style ?? row.style;
  const font = style?.font;
  const align = style?.align;
  const wide = span && span !== 'covered' ? span : undefined;
  // A dropdown on every label the filter covers, and the filter covers exactly
  // the cells the row has — which is how the engine works its width out too, so
  // a column past the last label gets nothing here and nothing in the file.
  const filters = row.filter === true && cell !== undefined;

  return (
    <td
      colSpan={wide?.colSpan}
      rowSpan={wide?.rowSpan}
      className={cn(
        // Paper's own rule, not the theme's. The gutters around this grid are
        // chrome and follow the mode; a cell is on the sheet, and the sheet
        // does not change colour because somebody turned the lights off.
        'max-w-[340px] min-w-[70px] border border-paper-200 px-2 py-0.5 align-bottom text-ellipsis whitespace-nowrap',
        // A cell that holds a picture cannot clip: the picture is meant to
        // spill over the cells beside it, which is what it does in Excel.
        picture ? 'relative overflow-visible' : 'overflow-hidden',
        // Room for the dropdown, so it sits beside the label rather than on it.
        filters && 'relative pr-5',
        ALIGNMENT[kind(cell)],
      )}
      style={{
        background: style?.fill,
        color: font?.color,
        fontWeight: font?.bold ? 700 : undefined,
        fontStyle: font?.italic ? 'italic' : undefined,
        textDecoration: decoration(font),
        fontSize: font?.size ? `${font.size}pt` : undefined,
        textAlign: align?.horizontal,
        verticalAlign: align?.vertical === 'middle' ? 'middle' : align?.vertical,
        whiteSpace: align?.wrap ? 'normal' : undefined,
        paddingLeft: align?.indent ? `${align.indent * 12}px` : undefined,
        ...borders(style?.border),
      }}
      title={cell?.style?.format ? `format ${cell.style.format}` : undefined}
    >
      {picture && <Picture picture={picture} />}
      {shown(cell)}
      {filters && <Dropdown />}
    </td>
  );
}

/**
 * The dropdown Excel draws on a label the autofilter covers.
 *
 * Indicative, like the rest of the grid — it does not filter anything, and the
 * banner above says the file is what Excel opens. What it is for is the slip
 * this feature exists to prevent: marking the title row instead of the labels
 * puts the dropdowns one row up, and here that is visible at a glance instead
 * of after somebody opens the export.
 *
 * Titled rather than left as a glyph, because a chevron on a spreadsheet header
 * could be a sort order, a collapsed group or a dozen other things.
 */
function Dropdown() {
  return (
    <span
      title="filters this column"
      className="pointer-events-none absolute top-1/2 right-1 -translate-y-1/2 rounded-[2px] border border-paper-300 bg-paper-100 px-[3px] text-[8px] leading-[1.5] text-paper-400"
    >
      ▼
    </span>
  );
}

function decoration(
  font: { underline?: boolean; strike?: boolean } | undefined,
): string | undefined {
  const parts = [font?.underline && 'underline', font?.strike && 'line-through'].filter(Boolean);
  return parts.length ? parts.join(' ') : undefined;
}

const WIDTHS = { thin: '1px', medium: '2px', thick: '3px' } as const;

function borders(
  edges: Partial<Record<Side, { style: string; color?: string }>> | undefined,
): Record<string, string> {
  const out: Record<string, string> = {};
  for (const side of ['top', 'right', 'bottom', 'left'] as Side[]) {
    const edge = edges?.[side];
    if (!edge) {
      continue;
    }
    const width = WIDTHS[edge.style as keyof typeof WIDTHS] ?? '1px';
    const line = ['dashed', 'dotted', 'double'].includes(edge.style) ? edge.style : 'solid';
    const key = `border${side[0].toUpperCase()}${side.slice(1)}`;
    // A rule the author did not colour is drawn in paper's own mid grey, not
    // in the theme's border: this cell is on the sheet, and the sheet is paper
    // whatever the chrome is doing.
    out[key] = `${width} ${line} ${edge.color ?? 'var(--color-paper-300)'}`;
  }
  return out;
}

/** What the cell reads as, and what its type says it is. */
function shown(cell: IrCell | undefined): string {
  const value = cell?.value;
  switch (value?.t) {
    case 'text':
      return value.v;
    case 'number':
      return String(value.v);
    case 'bool':
      return value.v ? 'TRUE' : 'FALSE';
    case 'date':
      // The serial back to a date, which is what the format will do in Excel.
      return new Date(Math.round((value.v - 25_569) * 86_400_000)).toISOString().slice(0, 10);
    case 'formula':
      return `=${value.v.formula}`;
    default:
      return '';
  }
}

/** A class per type, so a number written as text is visible at a glance. */
function kind(cell: IrCell | undefined): string {
  return cell?.value?.t ?? 'blank';
}

/** Excel's bijective base-26: A, Z, AA. */
function name(index: number): string {
  let out = '';
  let n = index + 1;
  while (n > 0) {
    out = String.fromCharCode(65 + ((n - 1) % 26)) + out;
    n = Math.floor((n - 1) / 26);
  }
  return out;
}
