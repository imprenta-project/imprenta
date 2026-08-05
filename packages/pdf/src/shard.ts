/**
 * One document, painted by every engine at once.
 *
 * A WebAssembly module has no threads, so one document is rendered on one core
 * and a long one takes as long as one core takes. The pool already spreads
 * *different* documents out; this spreads one.
 *
 * # Why it cannot simply cut the rows into four
 *
 * Because pagination is not a function of a row, it is a function of every row
 * before it. Cut a ledger in four and each piece starts a fresh page at the
 * cut, so four pieces come back with more pages than the document has — and
 * the only sign is the page numbers, which is to say the thing a reader looks
 * at. Measured on a 2,113-page ledger split twelve ways: 2,124 pages.
 *
 * So it goes in four passes, and the order is the design:
 *
 * 1. **measure**, on every engine at once, each over its own range of rows.
 *    What comes back is one height per row — four bytes, where the content it
 *    came from is every glyph on the line.
 * 2. **plan**, on one engine, over all the heights. Packing is arithmetic over
 *    heights and break flags; it never sees text, and nine thousand pages take
 *    about ten milliseconds. It says which row each page begins at, and how
 *    many pages there are.
 * 3. **paint**, on every engine at once, each given a contiguous run of pages
 *    and told which number its first one carries. Because every piece starts
 *    where a page started, its pagination cannot drift from the plan's.
 * 4. **merge**, on one engine: one object id space, one page tree, one file.
 *    25 ms on that ledger.
 *
 * Nothing here renumbers a page after the fact. Stamping page numbers onto a
 * finished document is the approach this engine exists to replace; every
 * fragment knew its own number before a glyph was placed.
 */
import type { Pool } from './pool.js';

/** A document this can split, taken apart into the pieces the passes need. */
export interface Shardable {
  page: unknown;
  head: { columns: unknown[] };
  rows: unknown[];
  header?: unknown;
  footer?: unknown;
}

/**
 * Whether a document may be split, and if so its pieces.
 *
 * Deliberately narrow. The pass that plans works on **one atom per row**, and
 * that only holds for a document that is exactly one table with no declared
 * header — a heading before the table, or a header row repeated at the top of
 * every page, is another atom the plan would not know about, and every page
 * boundary after it would be off by one.
 *
 * Running totals are refused for a different reason: the planner packs
 * heights, and a running total is not a height. Until it carries the
 * contributions too, a document that prints "suma y sigue" goes down the
 * one-engine path, where it is correct.
 *
 * Everything refused here still renders. It renders on one engine.
 */
export function shardable(ir: unknown): Shardable | null {
  if (typeof ir !== 'object' || ir === null) return null;
  const document = ir as Record<string, unknown>;

  const accumulators = document.accumulators;
  if (Array.isArray(accumulators) && accumulators.length > 0) return null;

  const children = document.children;
  if (!Array.isArray(children) || children.length !== 1) return null;

  const only = children[0] as Record<string, unknown>;
  if (only?.t !== 'table') return null;
  if (only.header !== undefined && only.header !== null) return null;
  if (only.spaceAfter) return null;
  if (!Array.isArray(only.rows) || !Array.isArray(only.columns)) return null;

  const { t: _t, rows, ...head } = only;
  return {
    page: document.page,
    head: head as { columns: unknown[] },
    rows,
    header: document.header,
    footer: document.footer,
  };
}

/** One page, as the plan describes it. */
interface PlannedPage {
  firstAtom: number;
  lastAtom: number;
  opening: number[];
}

export interface ShardResult {
  pdf: Uint8Array;
  pages: number;
  bytes: number;
  diagnostics: string[];
}

/**
 * Below this many rows there is nothing to win: the measuring pass, the
 * planning pass and the merge together cost more than one engine would have
 * spent on the whole document.
 */
export const WORTH_SHARDING = 1_500;

export async function renderSharded(pool: Pool, document: Shardable): Promise<ShardResult> {
  const shards = Math.min(pool.size, Math.ceil(document.rows.length / 500));
  const setup = JSON.stringify({
    page: document.page,
    ...(document.header ? { header: document.header } : {}),
    ...(document.footer ? { footer: document.footer } : {}),
  });
  const head = JSON.stringify(document.head);

  // Every engine is taken out of circulation for the whole render, because the
  // engine that measures a row has to be the engine that paints it — that is
  // the entire saving. A lease is how one is addressed twice.
  const leases = await Promise.all(Array.from({ length: shards }, () => pool.lease()));
  try {
    // ── 1. measure, everywhere at once ──────────────────────────────────────
    const per = Math.ceil(document.rows.length / shards);
    const ranges = leases.map((_, i) => ({
      from: i * per,
      to: Math.min((i + 1) * per, document.rows.length),
    }));

    const measured = await Promise.all(
      leases.map((lease, i) =>
        lease.send({
          op: 'measure',
          setup,
          head,
          rows: JSON.stringify(document.rows.slice(ranges[i].from, ranges[i].to)),
        }),
      ),
    );
    const heights = concat(measured.map((reply) => new Uint8Array(reply.heights as ArrayBuffer)));

    // ── 2. plan, once ───────────────────────────────────────────────────────
    const planned = await leases[0].send({
      op: 'plan',
      setup,
      heights: heights.buffer as ArrayBuffer,
    });
    const plan = JSON.parse(planned.plan as string) as PlannedPage[];
    const total = plan.length;

    // ── 3. paint, everywhere at once ────────────────────────────────────────
    //
    // A page goes to the engine that measured the row it starts on. Its last
    // page may run past what that engine measured — by at most one page, since
    // only the page on the seam straddles two — and those rows are handed over
    // to be measured there.
    const work = leases.map((lease, i) => {
      const mine = plan.filter(
        (page) => page.firstAtom >= ranges[i].from && page.firstAtom < ranges[i].to,
      );
      if (mine.length === 0) return null;

      const first = mine[0].firstAtom;
      const past = mine[mine.length - 1].lastAtom + 1;
      const page = plan.indexOf(mine[0]) + 1;
      const extra =
        past > ranges[i].to ? JSON.stringify(document.rows.slice(ranges[i].to, past)) : '';

      return lease.send({
        op: 'fragmentMeasured',
        setup: JSON.stringify({
          page: document.page,
          ...(document.header ? { header: document.header } : {}),
          ...(document.footer ? { footer: document.footer } : {}),
          resume: { page, total, opening: [] },
        }),
        head,
        from: first - ranges[i].from,
        to: Math.min(past, ranges[i].to) - ranges[i].from,
        extra,
      });
    });

    const fragments = (await Promise.all(work.filter((job) => job !== null))) as Record<
      string,
      unknown
    >[];

    // ── 4. merge, once ──────────────────────────────────────────────────────
    if (fragments.length === 1) return asShard(fragments[0]);
    const merged = await leases[0].send({
      op: 'merge',
      fragments: fragments.map((reply) => reply.pdf as ArrayBuffer),
    });
    return asShard(merged);
  } finally {
    for (const lease of leases) lease.release();
  }
}

function asShard(reply: Record<string, unknown>): ShardResult {
  const pdf = new Uint8Array(reply.pdf as ArrayBuffer);
  return {
    pdf,
    pages: reply.pages as number,
    bytes: pdf.length,
    diagnostics: (reply.diagnostics as string[]) ?? [],
  };
}

function concat(parts: Uint8Array[]): Uint8Array {
  const out = new Uint8Array(parts.reduce((n, part) => n + part.length, 0));
  let at = 0;
  for (const part of parts) {
    out.set(part, at);
    at += part.length;
  }
  return out;
}
