# imprenta-pdf-wasm

The engine as a plain WebAssembly module. Compiled into `packages/pdf` as
`imprenta-pdf.wasm`. See the root `CLAUDE.md` for the rules that apply everywhere.

## Shape of the crate

- `lib.rs` — **glue and nothing else**: the `#[unsafe(no_mangle)]` exports,
  pointers in, numbers out, and the thread-locals that hold state between calls.
- `job.rs` — one render, in ordinary Rust. No pointers.
- `stream.rs` — a document fed in pieces.

The split is the same one `imprenta-pdf-napi` makes and for the same reason:
everything that can go wrong is below `lib.rs`, where `cargo test` reaches it
without a WebAssembly runtime anywhere near it.

## Rules

- **The module imports nothing.** That is the entire argument for this crate
  over the napi WebAssembly target: no Node-API, no WASI, no shim, so one
  artefact runs in Node, a browser, Deno, Bun and on an edge worker. Anything
  that adds an import — `std::time::Instant`, a random source, a `println!`
  reaching for stderr — takes that away, and it will not fail here. It will
  fail on somebody's runtime. `packages/pdf/test/module.test.ts` asserts the
  import list is empty.
- **Every export is `imprenta_*`.** Not tidiness: an unprefixed `alloc`
  interposed over the system allocator and segfaulted this crate's own test
  binary before a single test ran, and `write` is POSIX. The same test asserts
  nothing unprefixed leaves the module but `memory`.
- **Never panic across the boundary.** Every export returns `1` or `0` and a
  failure leaves a message at `error_ptr`. A panic in a WebAssembly module is
  an unrecoverable trap: the instance is dead, and with a pool that means a
  worker is dead. A malformed document must never do that.
- **The library outlives a call.** Fonts and images are loaded once with
  `assets_*` and kept. A pooled instance renders one document after another
  and re-copying a typeface into linear memory each time is pure waste.
- **`out_release` is not optional.** WebAssembly memory is never returned to
  the host, so an instance that kept its last PDF holds the largest one it ever
  produced for as long as it lives. The binding calls it as soon as it has
  read; if you add another way out, it calls it too.
- **`Session` not being `Send` costs nothing here.** A WebAssembly instance is
  single-threaded by construction, so the session lives in a thread-local
  between calls — no thread, no channel, no busy flag, and the ordering the
  napi crate enforces at run time is structural. Do not port that machinery
  across; it solves a problem this target does not have.
- The crate is `publish = false`. It ships as the `.wasm` inside
  `@imprentajs/pdf`.

## Threads

There are none, and not for want of trying:

- `wasm32-wasip1-threads` — `std::thread::spawn` **returns an error** with the
  pinned toolchain, `available_parallelism()` reports 0, and the module does
  not even declare a `wasi.thread-spawn` import. rayon builds a one-thread
  pool and falls back to running in line.
- `wasm32-unknown-unknown` + `+atomics` — needs a nightly rebuild of the
  standard library (`-Zbuild-std`) and a hand-written rayon spawn handler,
  since `std::thread` does not work there either. It compiles and workers do
  run Rust against a shared memory, but the pool handshake is `wasm-bindgen-rayon`'s
  problem to have solved and this crate does not carry it.

So `rayon` inside the module runs sequentially. That is fine, and it is why the
parallelism lives one level up in `packages/pdf`'s pool: whole documents
across instances, which measured faster than subdividing one document even
natively.

## After changing anything here

Rebuild before trusting a Node test — the same rule as the napi crates, and for
the same reason:

```bash
pnpm --filter @imprentajs/pdf build
```

The build needs the target installed:

```bash
rustup target add wasm32-unknown-unknown
```

## Testing

`cargo test -p imprenta-wasm` covers `job.rs`, `stream.rs` and the ABI itself —
the tests in `lib.rs` drive the exports through real pointers, because the
pointers are the contract. They share one module-wide state and take a mutex
rather than relying on `--test-threads=1`, which nobody remembers to pass.

What genuinely needs a runtime — that the module has no imports, what it costs
to instantiate, that the bytes match the native addon's — is tested from
`packages/pdf/test/`.
