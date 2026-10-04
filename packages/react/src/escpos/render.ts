import type { Receipt } from '@imprentajs/escpos';
import type { ReactElement } from 'react';
import type { Instance } from '../host.js';
import { only, reconcile } from '../reconcile.js';
import { toIr } from './receipt.js';

/** Resolve React components without loading an encoder or connecting a printer. */
export async function toReceipt(element: ReactElement): Promise<Receipt> {
  const root = only(await reconcile(element), '<EscPos>') as Instance;
  if (root.type !== 'escpos')
    throw new Error(`render expects an <EscPos>, and was given <${root.type}>`);
  return toIr(root);
}

/** JSON suitable for a queue, stored template output, or the ESC/POS engine. */
export async function render(element: ReactElement): Promise<string> {
  return JSON.stringify(await toReceipt(element));
}
