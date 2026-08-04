import type { ReactElement } from 'react';
import { unwrapTheme } from '../element.js';
import type { Instance } from '../host.js';
import { only, reconcile } from '../reconcile.js';
import { toIr } from './document.js';
import type { IrDocument } from './ir.js';

/**
 * Renders a document and returns the IR the engine reads.
 *
 * Asynchronous because React may suspend — a component that awaits its data is
 * ordinary now — and because a producer that has to change shape later is a
 * worse thing to inflict on callers than an `await` they did not need.
 */
export async function toDocument(element: ReactElement): Promise<IrDocument> {
  const container = await reconcile(element);

  const [node, theme] = unwrapTheme(only(container, '<Document>') as Instance, 'Document');

  if (node.type !== 'document') {
    throw new Error(`render expects a <Document>, and was given <${node.type}>`);
  }

  return toIr(node, theme);
}

/** As {@link toDocument}, as the JSON string the engine takes. */
export async function render(element: ReactElement): Promise<string> {
  return JSON.stringify(await toDocument(element));
}
