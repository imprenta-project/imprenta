import { readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { renderToFile } from './index.js';

const fonts = [{ data: readFileSync('../../crates/imprenta-pdf/tests/fonts/Roboto-Regular.ttf') }];
const rows = Number(process.argv[2] ?? 100_000);

const t0 = performance.now();
const ir = JSON.stringify({
  page: { width: 595, height: 842, margin: { top: 40, bottom: 40, left: 40, right: 40 } },
  children: [
    {
      t: 'table',
      repeatHeader: true,
      columns: [
        { width: { unit: 'pt', value: 60 } },
        { width: { unit: 'auto' } },
        { width: { unit: 'pt', value: 80 }, align: 'end' },
      ],
      rows: [
        { heading: true, cells: [{ text: 'Fecha' }, { text: 'Concepto' }, { text: 'Importe' }] },
        ...Array.from({ length: rows }, (_, i) => ({
          cells: [
            { text: `2026-08-${String((i % 28) + 1).padStart(2, '0')}` },
            { text: `Asiento contable numero ${i} del ejercicio en curso` },
            { text: `${(((i * 37) % 90000) / 100).toFixed(2)} EUR` },
          ],
        })),
      ],
    },
  ],
});
const built = performance.now() - t0;
const afterIr = process.memoryUsage();
const rssAfterIr = process.resourceUsage().maxRSS / 1024;

const path = join(tmpdir(), 'imprenta-bench.pdf');
const t1 = performance.now();
const out = await renderToFile(ir, path, { fonts });
const rendered = performance.now() - t1;

const peak = process.resourceUsage().maxRSS / 1024;
console.log(`rows          ${rows.toLocaleString()}`);
console.log(`pages         ${out.pages.toLocaleString()}`);
console.log(`IR string     ${(ir.length / 1e6).toFixed(1)} MB, built in ${built.toFixed(0)} ms`);
console.log(`JS heap (IR)  ${(afterIr.heapUsed / 1e6).toFixed(0)} MB`);
console.log(`peak before   ${rssAfterIr.toFixed(0)} MB  (all of it Node's own)`);
console.log(
  `render        ${(rendered / 1000).toFixed(2)} s  (${(rendered / out.pages).toFixed(3)} ms/page)`,
);
console.log(`output        ${(out.bytes / 1e6).toFixed(1)} MB`);
console.log(
  `peak RSS      ${peak.toFixed(0)} MB  (${((peak / out.pages) * 1000).toFixed(1)} KB/page)`,
);
