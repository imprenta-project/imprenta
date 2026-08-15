import type { ReactElement } from 'react';
import { unwrapTheme } from '../element.js';
import type { Instance } from '../host.js';
import { only, reconcile } from '../reconcile.js';
import { encodeJson } from './encode.js';
import type { IrWorkbook } from './ir.js';
import { toIr } from './workbook.js';

/**
 * Renders a workbook and returns the IR the writer reads.
 *
 * Asynchronous for the same reason the document one is: React may suspend, and
 * a component that awaits the rows it is about to lay out is the normal case
 * for a spreadsheet rather than an exotic one.
 */
export async function toWorkbook(element: ReactElement): Promise<IrWorkbook> {
  const container = await reconcile(element);
  const [node, theme] = unwrapTheme(only(container, '<Workbook>') as Instance, 'Workbook');

  if (node.type !== 'workbook') {
    throw new Error(`render expects a <Workbook>, and was given <${node.type}>`);
  }
  return toIr(node, theme);
}

/**
 * As {@link toWorkbook}, as the UTF-8 JSON bytes the writer takes.
 *
 * Bytes rather than a string, deliberately: V8 caps a string at 512 MiB of
 * characters and a large workbook died there, while serialising, before the
 * engine was involved at all. The writer has always accepted bytes — it
 * re-encoded the string anyway — so encoding here in pieces removes the cap
 * and one full copy of the IR from the heap, and nothing downstream changes.
 */
export async function render(element: ReactElement): Promise<Uint8Array> {
  return encodeJson(await toWorkbook(element));
}
