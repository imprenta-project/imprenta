import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { compile, instantiate, Memory } from '../dist/module.js';

const wasm = readFileSync(fileURLToPath(new URL('../imprenta-pdf.wasm', import.meta.url)));

/**
 * A wasm32 pointer is an `i32` on the wire, and JavaScript reads an `i32` as
 * signed. Once linear memory has grown past 2 GiB every pointer the module
 * hands back is negative unless somebody reinterprets it — and the very next
 * line uses it as an offset into the buffer. A document that was merely large
 * then dies with "offset is out of bounds", which reads like corruption and
 * points away from the real cause. Issue #12 is the afternoon that cost.
 */
describe('pointers past 2 GiB', () => {
  it('come back unsigned, and a write at one lands where it should', {
    timeout: 60_000,
  }, async () => {
    const e = await instantiate(await compile(wasm));
    const memory = new Memory(e);

    // Grow the instance past the signed-i32 line by leaking 256 MiB blocks.
    // The pages are never touched, so the OS lends them lazily and the loop
    // costs milliseconds, not gigabytes of real work.
    const CHUNK = 256 * 1024 * 1024;
    while (e.memory.buffer.byteLength < 2 ** 31 + CHUNK) {
      const ptr = e.imprenta_alloc(CHUNK);
      expect(ptr, 'a pointer read as signed').toBeGreaterThanOrEqual(0);
    }

    const data = new TextEncoder().encode('past the signed line');
    const held = memory.write(data);
    try {
      expect(held[0]).toBeGreaterThanOrEqual(0);
      expect(memory.readText(held[0], held[1])).toBe('past the signed line');
    } finally {
      memory.free(held);
    }
  });
});
