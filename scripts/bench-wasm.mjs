// What a render costs the WebAssembly module, in the only unit that traps.
//
// `Engine` rather than `render()`: the package index renders through a worker
// pool sized to the machine, and a fixed few hundred megabytes of pool has
// nothing to do with the document. One engine, one document, and the module's
// own `memory.buffer.byteLength` read either side of the call.
//
// Linear memory never shrinks, so one document per process — a second render
// here would report roughly nothing and be believed.
//
//   node scripts/bench-wasm.mjs declared <ir.json>
//   node scripts/bench-wasm.mjs stream <rows> [total]
//
// The two modes answer different questions. `declared` is what a caller pays
// today, IR and all; `stream` feeds the same ledger in batches and never
// holds it, so what is left in linear memory is the engine and nothing else.

import { readFileSync } from 'node:fs';
import { Engine } from '../packages/pdf/dist/engine.js';

const fonts = [
  { data: readFileSync('crates/imprenta-pdf/tests/fonts/Roboto-Regular.ttf') },
  { weight: 'bold', data: readFileSync('crates/imprenta-pdf/tests/fonts/Roboto-Bold.ttf') },
];

const engine = await Engine.load({
  wasm: readFileSync('packages/pdf/imprenta-pdf.wasm'),
  fonts,
});

const mb = () => engine.e.memory.buffer.byteLength / 1048576;

/** The same five-column ledger row `bench_engine` declares in Rust. */
const row = (i) => ({
  cells: [
    { text: `${String((i % 28) + 1).padStart(2, '0')}/${String((i % 12) + 1).padStart(2, '0')}/2024` },
    { text: `FV-2026-${String(i).padStart(6, '0')} Prestacion de servicios profesionales a cliente ${i % 400}` },
    { text: (100 + (i % 9000) / 3).toFixed(2) },
    { text: '0,00' },
    { text: (1000 + (i % 7000) / 7).toFixed(2) },
  ],
  ...(i % 2 === 0 ? { style: { background: '#f9fafb' } } : {}),
  totals: [{ accumulator: 0, value: 1 }],
});

const headCell = (text) => ({ text, weight: 'bold' });
const head = {
  columns: [{}, {}, {}, {}, {}],
  header: [
    { cells: ['430000 · Clientes', '', '', '', ''].map(headCell) },
    { cells: ['Fecha', 'Concepto', 'Debe', 'Haber', 'Saldo'].map(headCell) },
  ],
  repeatHeader: true,
  padding: { top: 2, right: 2, bottom: 2, left: 2 },
};

const band = (text, size) => ({
  height: size === 10 ? 28 : 20,
  children: [{ t: 'text', runs: [{ text }], style: { size } }],
});

function report(label, pages, ms, before, after, bytes) {
  console.log(
    `${label} · ${pages} pages · ${ms.toFixed(0)} ms · linear memory ${before.toFixed(1)} → ${after.toFixed(1)} MB` +
      ` (+${(after - before).toFixed(1)} MB, ${(((after - before) * 1024) / pages).toFixed(2)} KB/page)` +
      ` · pdf ${(bytes / 1e6).toFixed(2)} MB`,
  );
}

const mode = process.argv[2] ?? 'declared';

if (mode === 'declared') {
  const ir = readFileSync(process.argv[3], 'utf8');
  const before = mb();
  const started = performance.now();
  let result;
  try {
    result = engine.render(ir);
  } catch (error) {
    console.log(`TRAP after ${(performance.now() - started).toFixed(0)} ms at ${mb().toFixed(0)} MB`);
    console.log(`  ${error.message}`);
    process.exit(2);
  }
  report('declared', result.pages, performance.now() - started, before, mb(), result.bytes);
} else {
  const rows = Number(process.argv[3] ?? 40000);
  const total = process.argv[4] === 'total';
  const footer = total
    ? 'Pagina {{page}} de {{pages}} · suma y sigue {{debe}}'
    : 'Pagina {{page}} · suma y sigue {{debe}}';

  const before = mb();
  const started = performance.now();
  const doc = engine.printer({
    page: { width: 595.2756, height: 841.8898 },
    header: {
      height: 28,
      children: [
        { t: 'text', runs: [{ text: 'Libro mayor · ejercicio 2024', weight: 'bold' }], style: { size: 10 } },
      ],
    },
    footer: band(footer, 8),
    accumulators: ['debe'],
  });
  doc.openTable(head);
  const BATCH = 1000;
  for (let i = 0; i < rows; i += BATCH) {
    const batch = [];
    for (let k = i; k < Math.min(i + BATCH, rows); k++) batch.push(row(k));
    doc.rows(batch);
  }
  doc.closeTable();
  let result;
  try {
    result = doc.finish();
  } catch (error) {
    console.log(`TRAP after ${(performance.now() - started).toFixed(0)} ms at ${mb().toFixed(0)} MB`);
    console.log(`  ${error.message}`);
    process.exit(2);
  }
  report(
    total ? 'streamed + {{pages}}' : 'streamed',
    result.pages,
    performance.now() - started,
    before,
    mb(),
    result.bytes,
  );
}
