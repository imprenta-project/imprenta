import ReceiptPrinterRenderer from '@point-of-sale/receipt-printer-renderer';
import { toSvg } from '@point-of-sale/receipt-printer-renderer/svg';
import type { ResolvedProfile } from './types.js';

/** Simulate the emitted commands; printing itself never imports this entry point. */
export async function preview(bytes: Uint8Array, profile: ResolvedProfile): Promise<string> {
  const renderer = new ReceiptPrinterRenderer({
    language: 'esc-pos',
    // The protocol describes the print area, not the physical paper width.
    // Show an equal paper margin on the right when a left margin is configured.
    width: Math.ceil((profile.printableWidth + profile.marginLeft * 2) / 8) * 8,
    codepageMapping: 'epson',
  });
  return toSvg(renderer.layout(bytes));
}
