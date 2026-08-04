# imprenta-pdf-napi

The Node binding. Compiled into `packages/pdf` as `imprenta.<platform>.node`.
See the root `CLAUDE.md` for the rules that apply everywhere.

## Shape of the crate

- `lib.rs` — **glue and nothing else**: `#[napi]` signatures, JS values in,
  results out, and the global allocator.
- `job.rs` — one render, in ordinary Rust. No napi types.
- `stream.rs` — a document fed in pieces, and the thread that owns the session.

The split is deliberate: everything that can go wrong is in `job.rs` and
`stream.rs`, where it is tested with `cargo test` and no Node process at all.
If a new binding needs logic, the logic goes below `lib.rs`.

## Rules

- **Nothing runs on the main thread.** Rendering is arithmetic-bound and a long
  document takes tens of seconds. Every job goes to libuv's pool (`AsyncTask`)
  or to the session thread. A service must stay answerable while it prints —
  that is the whole reason this crate exists rather than a CLI.
- **The document crosses as a JSON string, not a JS object.** Walking an object
  property by property costs more than `JSON.stringify` plus serde, and a
  string is also what arrives from a file, a queue or an HTTP body. Do not add
  an object-taking overload "for convenience".
- **Never panic across the boundary.** A malformed document is a
  `JobError`/`napi::Error`, so Node gets a rejected promise rather than a dead
  process.
- **`Session` is not `Send`** — krilla holds its fonts behind an `Rc`. It
  therefore lives on one dedicated thread for the life of the document, fed
  down a channel. Do not try to put batches on the pool; the pool picks a
  different thread each time.
- **One call in flight at a time.** A stream is read in order and two promises
  in flight have no order at all, so a second call while one is running is
  refused — a rejection, never a silent queue. The channel is FIFO, which is
  what gives ordering; the `busy` flag is backpressure only.
- **`renderToFile` writes from Rust.** A 128 MB ledger must never become a
  128 MB Buffer on its way to disk. Any new output path keeps that property.
- **mimalloc is the global allocator** and is load-bearing. A long document is
  millions of short allocations and the system allocator keeps rather than
  returns that memory; peak RSS is where it shows.
- The crate is `publish = false`. It ships as the `.node` inside
  `@imprentajs/pdf`, and the platform matrix lives in that package's `napi.targets`.

## After changing anything here

The TypeScript typings in `packages/pdf/index.d.ts` are **generated** by the
napi build. Change a `#[napi]` signature and you must rebuild, or Node keeps
the old shape and serde silently drops the fields the old binary does not know:

```bash
pnpm --filter @imprentajs/pdf build
```

A Node test that passed without that rebuild has proved nothing. This has
already happened once — a streaming test compared two documents that both
lacked a footer, and both were wrong.

## Testing

`cargo test -p imprenta-pdf-napi` covers `job.rs` and `stream.rs`. Behaviour that
genuinely needs a running Node — promise rejection, backpressure, the ordering
refusal — is tested from `packages/pdf/test/`.
