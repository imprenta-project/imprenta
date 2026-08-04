import type { ReactElement } from 'react';
import { unwrapTheme } from '../element.js';
import type { Instance } from '../host.js';
import { only, reconcile } from '../reconcile.js';
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

/** As {@link toWorkbook}, as the JSON string the writer takes. */
export async function render(element: ReactElement): Promise<string> {
  return JSON.stringify(await toWorkbook(element));
}
