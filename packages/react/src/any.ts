import type { ReactElement } from 'react';
import { unwrapTheme } from './element.js';
import type { Instance } from './host.js';
import { toIr as toDocumentIr } from './pdf/document.js';
import type { IrDocument } from './pdf/ir.js';
import { only, reconcile } from './reconcile.js';
import type { IrWorkbook } from './xlsx/ir.js';
import { toIr as toWorkbookIr } from './xlsx/workbook.js';

/**
 * Rendering something without knowing which format it declares.
 *
 * For tooling, and only for tooling. The CLI is handed a `.tsx` file and
 * cannot tell whether it declares a page or a sheet until it has run it —
 * a default export is a function, and what it returns is the answer.
 *
 * Deliberately its own entry point rather than part of the root. It reaches
 * into both surfaces, so anything that imports it gets both; a controller that
 * only ever writes spreadsheets should not carry the page elements around, and
 * `@imprentajs/react/xlsx` still does not.
 */
export type Rendered = { format: 'pdf'; ir: IrDocument } | { format: 'xlsx'; ir: IrWorkbook };

export async function renderAny(element: ReactElement): Promise<Rendered> {
  const container = await reconcile(element);
  const [node, theme] = unwrapTheme(
    only(container, '<Document> or <Workbook>') as Instance,
    'file',
  );

  switch (node.type) {
    case 'document':
      return { format: 'pdf', ir: toDocumentIr(node, theme) };
    case 'workbook':
      return { format: 'xlsx', ir: toWorkbookIr(node, theme) };
    default:
      throw new Error(
        `a file declares a <Document> or a <Workbook> at its root, and this one has a <${node.type}>`,
      );
  }
}
