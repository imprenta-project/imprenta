import { readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Printer } from './dist/stream.js';
import { renderToFile } from './index.js';

const fonts = [{ data: readFileSync('../../crates/imprenta-pdf/tests/fonts/Roboto-Regular.ttf') }];
const page = { width: 595, height: 842, margin: { top: 40, bottom: 40, left: 40, right: 40 } };

/** A transcript: no table anywhere, just thousands of paragraphs. */
const para = (i) => ({
  t: 'text',
  runs: [
    {
      text: `${i}. Intervención registrada en el acta, con el detalle acordado por las partes y la referencia del expediente correspondiente.`,
    },
  ],
  style: { size: 9, spaceAfter: 3 },
});

const N = Number(process.argv[3] ?? 40_000);
const out = join(tmpdir(), 'imprenta-prose.pdf');
const mb = (n) => (n / 1e6).toFixed(0);

if (process.argv[2] === 'whole') {
  const t0 = performance.now();
  const ir = JSON.stringify({ page, children: Array.from({ length: N }, (_, i) => para(i)) });
  const heap = process.memoryUsage().heapUsed;
  const r = await renderToFile(ir, out, { fonts });
  console.log(
    `whole    paras ${N}  pages ${r.pages}  ${((performance.now() - t0) / 1000).toFixed(2)} s`,
  );
  console.log(
    `         IR ${(ir.length / 1e6).toFixed(1)} MB  heap ${mb(heap)} MB  RSS ${(process.resourceUsage().maxRSS / 1024).toFixed(0)} MB`,
  );
} else {
  const t0 = performance.now();
  const BATCH = Number(process.argv[4] ?? 1000);
  const printer = new Printer(page, { fonts });
  let heap = 0;
  for (let sent = 0; sent < N; sent += BATCH) {
    const n = Math.min(BATCH, N - sent);
    await printer.nodes(Array.from({ length: n }, (_, k) => para(sent + k)));
    heap = Math.max(heap, process.memoryUsage().heapUsed);
  }
  const r = await printer.finish(out);
  console.log(
    `stream   paras ${N}  pages ${r.pages}  ${((performance.now() - t0) / 1000).toFixed(2)} s  batch ${BATCH}`,
  );
  console.log(
    `         heap peak ${mb(heap)} MB  held ${printer.pending}  RSS ${(process.resourceUsage().maxRSS / 1024).toFixed(0)} MB`,
  );
}
