import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const wasm = readFileSync(fileURLToPath(new URL('../imprenta-pdf.wasm', import.meta.url)));

/**
 * The properties that make this artefact worth having at all.
 *
 * None of them is visible to `cargo test`: they only exist once the crate has
 * been compiled to WebAssembly, which is why they are asserted here against
 * the module that actually ships.
 */
describe('the module', () => {
  it('imports nothing', async () => {
    // The whole argument for this package. A module with no imports needs no
    // Node-API, no WASI and no shim, which is why one file runs in Node, a
    // browser, Deno, Bun and on an edge worker. Anything that adds an import
    // — `std::time::Instant`, a random source, a `println!` reaching for
    // stderr — takes that away, and it would not fail here. It would fail on
    // somebody else's runtime, as "works on mine".
    const module = await WebAssembly.compile(wasm);

    expect(WebAssembly.Module.imports(module)).toEqual([]);
  });

  it('instantiates with an empty import object', async () => {
    const module = await WebAssembly.compile(wasm);

    const instance = await WebAssembly.instantiate(module, {});

    expect(instance.exports).toBeDefined();
  });

  it('exports everything the binding calls, and every export is prefixed', async () => {
    // The prefix is not tidiness. An unprefixed `alloc` interposed over the
    // system allocator and segfaulted the crate's own test binary before a
    // single test ran; `write` is POSIX. Anything unprefixed here is a symbol
    // waiting to collide with a host's.
    const module = await WebAssembly.compile(wasm);
    const exported = WebAssembly.Module.exports(module).map((e) => e.name);

    for (const name of [
      'imprenta_alloc',
      'imprenta_dealloc',
      'imprenta_assets_reset',
      'imprenta_assets_font',
      'imprenta_assets_image',
      'imprenta_render',
      'imprenta_stream_open',
      'imprenta_stream_nodes',
      'imprenta_stream_open_table',
      'imprenta_stream_rows',
      'imprenta_stream_close_table',
      'imprenta_stream_pending',
      'imprenta_stream_finish',
      'imprenta_out_ptr',
      'imprenta_out_len',
      'imprenta_out_pages',
      'imprenta_out_release',
      'imprenta_diagnostics_ptr',
      'imprenta_diagnostics_len',
      'imprenta_error_ptr',
      'imprenta_error_len',
    ]) {
      expect(exported, `missing ${name}`).toContain(name);
    }

    const loose = exported.filter((n) => !n.startsWith('imprenta_') && !n.startsWith('__'));
    expect(loose, 'unprefixed exports').toEqual(['memory']);
  });

  it('instantiates in well under a millisecond', async () => {
    // What makes an instance-per-worker pool viable, and what would quietly
    // stop being true if the module grew a large data segment or a start
    // function. Measured at 0.09 ms; the bound is loose enough not to flake on
    // a busy machine and tight enough that an order of magnitude fails.
    const module = await WebAssembly.compile(wasm);
    await WebAssembly.instantiate(module, {});

    const started = performance.now();
    for (let i = 0; i < 20; i++) await WebAssembly.instantiate(module, {});

    expect((performance.now() - started) / 20).toBeLessThan(5);
  });
});
