import type { RenderResult } from '@imprentajs/escpos';
import type { Finding } from './checks.js';

/** Receipt findings are about native capabilities, not PDF pages or spreadsheet cells. */
export function receiptChecks(out: RenderResult): Finding[] {
  const findings: Finding[] = out.diagnostics.map((detail) => ({
    rule: detail.split(':', 1)[0],
    status: 'warning',
    source: 'engine',
    detail,
    occurrences: 1,
  }));
  if (out.tickets === 0)
    findings.push({
      rule: 'empty-receipt',
      status: 'warning',
      source: 'document',
      detail: 'this ticket has no printable content',
      occurrences: 1,
    });
  return findings;
}
