/**
 * The IR as UTF-8 bytes, produced without ever holding it as one string.
 *
 * `JSON.stringify` of a whole workbook dies at V8's 512 MiB string cap —
 * fourteen million cells got there, while serialising, before the engine was
 * involved at all. So the workbook is stringified in pieces small enough that
 * no single string approaches the cap, each piece encoded as it is made, and
 * the pieces joined as bytes, where the only limit is memory.
 *
 * The output is byte for byte what `JSON.stringify` would have produced —
 * not merely an equivalent document — and the test holds that line, because
 * "equivalent" is where two serialisers start to drift.
 */

const encoder = new TextEncoder();

/**
 * Elements stringified per piece. Small enough that a slice of the widest
 * plausible rows stays far from the string cap, large enough that the
 * per-piece cost disappears; the exact number is not load-bearing.
 */
const SLICE = 4096;

export function encodeJson(value: unknown): Uint8Array {
  const chunks: Uint8Array[] = [];
  let total = 0;
  const push = (piece: string) => {
    const bytes = encoder.encode(piece);
    chunks.push(bytes);
    total += bytes.length;
  };

  walk(value, push);

  const out = new Uint8Array(total);
  let at = 0;
  for (const chunk of chunks) {
    out.set(chunk, at);
    at += chunk.length;
  }
  return out;
}

/**
 * Containers are walked, leaves go through `JSON.stringify` itself — which is
 * what keeps the output identical to the real thing: escaping, number
 * formatting and undefined-skipping are V8's own, never a reimplementation.
 */
function walk(value: unknown, push: (piece: string) => void): void {
  if (Array.isArray(value)) {
    push('[');
    for (let at = 0; at < value.length; at += SLICE) {
      const piece = value
        .slice(at, at + SLICE)
        // `stringify` says `null` for an element it cannot represent, and so
        // does this, because an array's length must survive the trip.
        .map((element) => JSON.stringify(element) ?? 'null')
        .join(',');
      push(at ? `,${piece}` : piece);
    }
    push(']');
    return;
  }

  if (value !== null && typeof value === 'object') {
    // Anything that serialises itself — a Date most of all — must keep doing
    // so; walking its entries would write `{}` where stringify writes a string.
    if (typeof (value as { toJSON?: unknown }).toJSON === 'function') {
      push(JSON.stringify(value) ?? 'null');
      return;
    }
    push('{');
    let first = true;
    for (const [key, held] of Object.entries(value)) {
      // What stringify leaves out of an object, this leaves out of one.
      if (held === undefined || typeof held === 'function') continue;
      push(`${first ? '' : ','}${JSON.stringify(key)}:`);
      first = false;
      walk(held, push);
    }
    push('}');
    return;
  }

  push(JSON.stringify(value) ?? 'null');
}
