import { type Node as HostNode, type Instance, isText } from '../host.js';
import type { Theme } from '../tailwind.js';
import type {
  CellStyle,
  IrCell,
  IrColumn,
  IrMerge,
  IrPicture,
  IrRow,
  IrSheet,
  IrValue,
  IrWorkbook,
} from './ir.js';
import { resolve } from './tailwind.js';

/** Days from 1899-12-30, which is where Excel counts dates from. */
const EPOCH = Date.UTC(1899, 11, 30);
const MS_PER_DAY = 86_400_000;

/** Excel allows this many characters on a tab, and no more. */
const NAME_LIMIT = 31;

/** Excel forbids these in a sheet name, because they mean something in a reference. */
const FORBIDDEN = /[[\]:*?/\\]/;

export function toIr(root: Instance, theme: Theme = {}): IrWorkbook {
  const sheets: IrSheet[] = [];
  const seen = new Set<string>();

  for (const child of root.children) {
    if (isText(child)) {
      throw new Error('a <Workbook> holds <Sheet>s, and this one has loose text in it');
    }
    if (child.type !== 'sheet') {
      throw new Error(`a <Workbook> holds <Sheet>s, and this one has a <${child.type}>`);
    }
    const sheet = toSheet(child, theme);

    // Two tabs of the same name is a workbook Excel refuses to open, and the
    // message it gives names neither of them.
    const key = sheet.name.toLowerCase();
    if (seen.has(key)) {
      throw new Error(`two sheets are called ${JSON.stringify(sheet.name)}, and Excel allows one`);
    }
    seen.add(key);
    sheets.push(sheet);
  }

  if (sheets.length === 0) {
    throw new Error('a workbook needs at least one <Sheet>, and Excel will not open one without');
  }
  return { sheets };
}

function toSheet(node: Instance, theme: Theme): IrSheet {
  const props = node.props as {
    name?: unknown;
    freeze?: { rows?: number; columns?: number };
  };

  const name = String(props.name ?? '');
  if (!name) {
    throw new Error('a <Sheet> needs a name, which is what goes on the tab');
  }
  if (name.length > NAME_LIMIT) {
    throw new Error(
      `the sheet name ${JSON.stringify(name)} is ${name.length} characters, and Excel allows ${NAME_LIMIT}`,
    );
  }
  if (FORBIDDEN.test(name)) {
    throw new Error(
      `the sheet name ${JSON.stringify(name)} uses a character Excel forbids on a tab: [ ] : * ? / \\`,
    );
  }

  const columns: IrColumn[] = [];
  const rows: IrRow[] = [];
  const merges: IrMerge[] = [];
  const pictures: IrPicture[] = [];

  for (const child of node.children) {
    if (isText(child)) {
      throw new Error(`<Sheet name="${name}"> has loose text in it, outside any <Cell>`);
    }
    switch (child.type) {
      case 'column':
        if (rows.length) {
          // Excel writes `<cols>` before `<sheetData>`, and an author who puts
          // a column half way down has almost certainly mistaken it for a cell.
          throw new Error(
            `<Sheet name="${name}"> declares a <Column> after a <Row>: columns come first, as they do in the file`,
          );
        }
        columns.push(toColumn(child, theme));
        break;
      case 'row':
        rows.push(toRow(child, rows.length, merges, pictures, theme));
        break;
      default:
        throw new Error(
          `<Sheet name="${name}"> holds <Column>s and <Row>s, and this one has a <${child.type}>`,
        );
    }
  }

  return {
    name,
    ...(columns.length ? { columns } : {}),
    ...(rows.length ? { rows } : {}),
    ...(merges.length ? { merges } : {}),
    ...(props.freeze ? { freeze: props.freeze } : {}),
    ...(pictures.length ? { pictures } : {}),
  };
}

function toColumn(node: Instance, theme: Theme): IrColumn {
  const props = node.props as { width?: number; format?: string; className?: string };
  const style = styleOf(props, theme);
  return {
    ...(props.width !== undefined ? { width: props.width } : {}),
    ...(style ? { style } : {}),
  };
}

function toRow(
  node: Instance,
  at: number,
  merges: IrMerge[],
  pictures: IrPicture[],
  theme: Theme,
): IrRow {
  const props = node.props as { height?: number; className?: string };
  const style = styleOf(props, theme);
  const cells: IrCell[] = [];

  for (const child of node.children) {
    if (isText(child)) {
      throw new Error('a <Row> holds <Cell>s, and this one has text sitting loose in it');
    }
    if (child.type !== 'cell') {
      throw new Error(`a <Row> holds <Cell>s, and this one has a <${child.type}>`);
    }

    const column = cells.length;
    // A picture is recorded on the sheet, which is where the format keeps it:
    // it floats over the grid rather than sitting in the cell, and the cell
    // it names stays blank.
    for (const nested of child.children) {
      if (!isText(nested) && nested.type === 'image') {
        pictures.push(toPicture(nested, at, column));
      }
    }
    cells.push(toCell(child, theme));

    // A span is a merge, and a merge is recorded on the sheet rather than on
    // the cell — which is also where Excel keeps it.
    const { colSpan = 1, rowSpan = 1 } = child.props as { colSpan?: number; rowSpan?: number };
    if (colSpan > 1 || rowSpan > 1) {
      merges.push({
        fromRow: at,
        fromColumn: column,
        toRow: at + rowSpan - 1,
        toColumn: column + colSpan - 1,
      });
      // The columns a span covers have to exist and be empty, or the cell
      // after it lands in the wrong place.
      for (let i = 1; i < colSpan; i += 1) {
        cells.push({ value: { t: 'blank' } });
      }
    }
  }

  return {
    ...(cells.length ? { cells } : {}),
    ...(props.height !== undefined ? { height: props.height } : {}),
    ...(style ? { style } : {}),
  };
}

function toPicture(node: Instance, row: number, column: number): IrPicture {
  const props = node.props as {
    src?: unknown;
    width?: unknown;
    align?: IrPicture['align'];
    valign?: IrPicture['valign'];
    offset?: { x?: number; y?: number };
  };

  const src = String(props.src ?? '');
  if (!src) {
    throw new Error('an <Image> needs a src, which is the name its bytes were handed over under');
  }
  if (typeof props.width !== 'number' || !(props.width > 0)) {
    throw new Error(`<Image src="${src}"> needs a width in points, and the height comes from it`);
  }

  const { x = 0, y = 0 } = props.offset ?? {};
  return {
    image: src,
    row,
    column,
    ...(x ? { dx: x } : {}),
    ...(y ? { dy: y } : {}),
    width: props.width,
    // `start` is the default on both sides, and absent is how the IR says so.
    ...(props.align && props.align !== 'start' ? { align: props.align } : {}),
    ...(props.valign && props.valign !== 'start' ? { valign: props.valign } : {}),
  };
}

function toCell(node: Instance, theme: Theme): IrCell {
  const props = node.props as {
    value?: string | number | boolean | Date;
    formula?: string;
    cached?: number;
    format?: string;
    className?: string;
  };

  if (props.value !== undefined && props.formula !== undefined) {
    throw new Error('a <Cell> holds a value or a formula, and this one was given both');
  }

  const style = styleOf(props, theme);
  return {
    value: contentOf(props, node.children),
    ...(style ? { style } : {}),
  };
}

/**
 * What a cell holds, and what type it is.
 *
 * The distinction the whole format turns on. Children are always text, so
 * `<Cell>007</Cell>` keeps its leading zeros; `value` carries the JavaScript
 * type through, so `<Cell value={7} />` is something `SUM` can add up. Making
 * that visible in the JSX is deliberate — it is the mistake everybody makes
 * once, and it does not show until somebody tries to total a column.
 */
function contentOf(
  props: { value?: string | number | boolean | Date; formula?: string; cached?: number },
  children: HostNode[],
): IrValue {
  if (props.formula !== undefined) {
    return {
      t: 'formula',
      v: {
        formula: props.formula,
        ...(props.cached !== undefined ? { cached: props.cached } : {}),
      },
    };
  }

  const { value } = props;
  if (value !== undefined) {
    if (value instanceof Date) {
      return { t: 'date', v: serial(value) };
    }
    switch (typeof value) {
      case 'number':
        // NaN and Infinity have no representation in the file format, and a
        // workbook that will not open is a worse answer than a named error.
        if (!Number.isFinite(value)) {
          throw new Error(`a <Cell> was given ${value}, which a spreadsheet cannot hold`);
        }
        return { t: 'number', v: value };
      case 'boolean':
        return { t: 'bool', v: value };
      default:
        return { t: 'text', v: value };
    }
  }

  const text = flatten(children);
  return text === '' ? { t: 'blank' } : { t: 'text', v: text };
}

/**
 * A `Date` as the number Excel keeps underneath one.
 *
 * Read in UTC, because a serial names a wall clock with no zone and somebody
 * has to choose which. Reading the local components instead would put a
 * transaction on the previous day for anyone west of Greenwich, and the choice
 * belongs to whoever knows where the date came from: pass a UTC instant, or
 * pass the serial as a number.
 */
function serial(date: Date): number {
  const time = date.getTime();
  if (!Number.isFinite(time)) {
    throw new Error('a <Cell> was given an invalid Date');
  }
  return (time - EPOCH) / MS_PER_DAY;
}

/** Everything nested inside a cell, as one string. */
function flatten(children: HostNode[]): string {
  let out = '';
  for (const child of children) {
    if (isText(child)) {
      out += child.text;
    } else if (child.type !== 'image') {
      // An image is the exception, and the only one: it hangs off the cell
      // rather than going into it, so it has already been taken and what is
      // left here is whatever text the author put beside it — usually none.
      throw new Error(`a <Cell> holds text, and this one has a <${child.type}> in it`);
    }
  }
  return out;
}

/** The classes and the number format on a node, as one style, or nothing. */
function styleOf(
  props: { className?: string; format?: string },
  theme: Theme,
): CellStyle | undefined {
  const style: CellStyle = props.className ? resolve(props.className, theme) : {};
  if (props.format !== undefined) {
    style.format = props.format;
  }
  return Object.keys(style).length ? style : undefined;
}
