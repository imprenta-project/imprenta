import { readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { close, renderToFile } from './dist/index.js';
import { Printer } from './dist/stream.js';

const fonts = [{ data: readFileSync('../../crates/imprenta-pdf/tests/fonts/Roboto-Regular.ttf') }];
const page = { width: 595, height: 842, margin: { top: 40, bottom: 40, left: 40, right: 40 } };
const head = {
  columns: [
    { width: { unit: 'pt', value: 60 } },
    {},
    { width: { unit: 'pt', value: 80 }, align: 'end' },
  ],
  header: { cells: [{ text: 'Fecha' }, { text: 'Concepto' }, { text: 'Importe' }] },
};
const rows = (from, to) =>
  Array.from({ length: to - from }, (_, k) => {
    const i = from + k;
    return {
      cells: [
        { text: `2026-08-${String((i % 28) + 1).padStart(2, '0')}` },
        { text: `Asiento contable numero ${i} del ejercicio en curso` },
        { text: `${(((i * 37) % 90000) / 100).toFixed(2)} EUR` },
      ],
    };
  });

const N = Number(process.argv[3] ?? 100_000);
const out = join(tmpdir(), 'imprenta-bench.pdf');
const mb = (n) => (n / 1e6).toFixed(0);

if (process.argv[2] === 'whole') {
  const t0 = performance.now();
  const ir = JSON.stringify({ page, children: [{ t: 'table', ...head, rows: rows(0, N) }] });
  const heap = process.memoryUsage().heapUsed;
  const r = await renderToFile(ir, out, { fonts });
  console.log(
    `whole    rows ${N}  pages ${r.pages}  ${((performance.now() - t0) / 1000).toFixed(2)} s`,
  );
  console.log(
    `         IR string ${(ir.length / 1e6).toFixed(1)} MB  JS heap ${mb(heap)} MB  peak RSS ${(process.resourceUsage().maxRSS / 1024).toFixed(0)} MB`,
  );
} else {
  const BATCH = Number(process.argv[4] ?? 1000);
  const t0 = performance.now();
  const printer = new Printer(page, { fonts });
  await printer.openTable(head);
  let heap = 0;
  for (let sent = 0; sent < N; sent += BATCH) {
    await printer.rows(rows(sent, Math.min(sent + BATCH, N)));
    heap = Math.max(heap, process.memoryUsage().heapUsed);
  }
  await printer.closeTable();
  const r = await printer.finish(out);
  console.log(
    `stream   rows ${N}  pages ${r.pages}  ${((performance.now() - t0) / 1000).toFixed(2)} s  batch ${BATCH}`,
  );
  console.log(
    `         JS heap peak ${mb(heap)} MB  held ${printer.pending} atoms  peak RSS ${(process.resourceUsage().maxRSS / 1024).toFixed(0)} MB`,
  );
}

// The pool keeps its workers warm; nothing here exits without stopping them.
await close();
