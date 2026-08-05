# imprenta-xlsx-wasm

The spreadsheet writer as a plain WebAssembly module. Compiled into
`packages/xlsx` as `imprenta-xlsx.wasm`. See the root `CLAUDE.md` for the rules
that apply everywhere, and `crates/imprenta-pdf-wasm/CLAUDE.md` for the shape —
this crate is the same one, over a different model.

## Shape of the crate

- `lib.rs` — **glue and nothing else**: the `#[unsafe(no_mangle)]` exports and
  the thread-locals that hold state between calls.
- `job.rs` — one workbook, in ordinary Rust. No pointers.
- `stream.rs` — a workbook fed a batch of rows at a time.

## Rules

- **The module imports nothing.** `packages/xlsx/test/module.test.ts` asserts
  the import list is empty against the built module. Anything that adds one —
  a clock, a random source, a `println!` — costs the portability the whole
  package is for, and it will not fail here. It will fail on somebody's
  runtime.
- **Every export is `imprenta_*`.** Not tidiness: an unprefixed `alloc`
  interposed over the system allocator and segfaulted this crate's own test
  binary before a single test ran, and `write` is POSIX. The same test asserts
  nothing unprefixed leaves the module but `memory`.
- **Never panic across the boundary.** Every export returns `1` or `0` and
  leaves a message at `imprenta_error_ptr`. A panic in WebAssembly is an
  unrecoverable trap: the instance is dead, and with a pool that means a dead
  worker.
- **`imprenta_out_release` is not optional.** WebAssembly memory is never
  returned to the host, so an instance that kept its last workbook would hold
  the largest one it ever produced for as long as it lived.
- **There is no path-taking variant**, unlike the Node binding. A module has no
  filesystem — which is the point of it — so the bytes always come back through
  linear memory and the host decides where they go.

## Why this is a separate module from the page engine

**Share the vocabulary, never the model.** A page is measured and every glyph
on it was placed by the engine; a sheet has no page and nothing is painted, and
a cell carries a value and a type that Excel decides how to draw. One module
serving both would be the first place that stopped being true.

It also removes a trap the native side had to live with: two addons could not
each declare a `#[global_allocator]`, and it showed as the PDF engine aborting
on a font that was perfectly good. Two WebAssembly modules have a linear memory
each and cannot collide.

## After changing anything here

```bash
rustup target add wasm32-unknown-unknown   # once
pnpm --filter @imprentajs/xlsx build
```

A Node test that ran without that rebuild has proved nothing.

## Testing

`cargo test -p imprenta-xlsx-wasm` covers `job.rs`, `stream.rs` and the ABI
itself — the tests in `lib.rs` drive the exports through real pointers, because
the pointers are the contract. They share one module-wide state and take a
mutex rather than relying on `--test-threads=1`, which nobody remembers to
pass.

The test that earns its keep is `a_number_stays_a_number`: it reads the written
package back through `calamine` and checks the cell is a float. Every other
test in this crate passed while the fixtures were writing blank cells, because
comparing two equally empty workbooks is easy.
