# Imprenta

Document engine in Rust, authored in React. PDF first; XLSX and DOCX to follow.
Apache-2.0.

`README.md` says what it does. Each crate and package has its own `CLAUDE.md`
with the rules that apply inside it — read the one nearest what you are
changing before reopening a decision it settles.

## Non-negotiables

**1. Test-first. No exceptions, Rust or Node.**
Write the failing test, watch it fail for the right reason, then write the code.
A test that has never failed has never been shown to test anything. This applies
to bug fixes too: reproduce first.

**2. Rebuild the native addon before trusting a Node test.**
`packages/pdf` and `packages/xlsx` are compiled artefacts of the napi crates. A stale `.node`
means Node tests run against yesterday's engine, and serde silently drops fields
the old binary does not know — so the test passes by comparing two documents
that are both wrong. This has already produced one green test that was a lie.
After touching any Rust:

```bash
pnpm --filter @imprentajs/pdf build      # release, what tests should use
```

**3. Never widen the scope to accounting or Spanish.**
This is a general-purpose engine that happens to have been prompted by an
accounting need. Invoices and ledgers are *examples*. No domain vocabulary in
`crates/` or `packages/` — that belongs in `examples/`.

**4. Do not commit unless asked.** Commits are GPG-signed and the author reviews
first.

## Commands

```bash
# Everything, the way CI runs it — note `run`, since `pnpm ci` is a pnpm builtin
pnpm run ci                      # turbo: test, build, lint, format:check, check-types

# Rust
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo test -p imprenta-pdf pack  # one module's tests

# Node
pnpm --filter @imprentajs/cli test
pnpm --filter @imprentajs/cli test -- checks   # one file
pnpm --filter @imprentajs/pdf build            # rebuild the native addon (see rule 2)
pnpm --filter @imprentajs/xlsx build           # the other one
pnpm --filter @imprentajs/pdf build:debug      # faster, unoptimised — never benchmark on it

# Look at a real PDF, which is the only way to see some bugs
cargo run -p imprenta-pdf  --example invoice --release  # writes into preview/
cargo run -p imprenta-xlsx --example ventas  --release  # and a spreadsheet
pnpm --filter facturacion dev                           # the preview server
pnpm --filter backend start                             # a controller returning bytes
```

## Repository map

```
crates/
  imprenta-core/       units, colour, diagnostics, IR envelope — format-neutral
  imprenta-pdf/        the page engine: measure → pack → paint
  imprenta-xlsx/       the spreadsheet writer: a workbook model → OOXML
  imprenta-pdf-napi/   the Node binding for the first  (cdylib); glue only
  imprenta-xlsx-napi/  the Node binding for the second (cdylib); glue only
packages/
  pdf/             @imprentajs/pdf   — the page addon, and its streaming Printer
  xlsx/            @imprentajs/xlsx  — the sheet addon, and its streaming Book
  react/           @imprentajs/react — one reconciler, a surface per format
  fonts/           @imprentajs/fonts — fetch and cache Google faces; no CLI needed
  cli/             @imprentajs/cli   — init, dev preview, build, and the checks
examples/
  facturacion/     a project laid out the way one really would be
  backend/         React in, PDF or XLSX bytes out, no CLI anywhere
```

Dependencies point one way: `core → {pdf, xlsx} → napi → the addons → the CLI`.
`@imprentajs/react` depends on React and nothing else — keep it that way, so a
document can be declared where the addons cannot be installed.

## One vocabulary, two models

The rule that decides where anything new goes, and it has to survive DOCX:

> **Share the vocabulary, never the model.**
> `Pt`, `Color`, `Edges`, diagnostics, the versioned envelope — shared. What a
> node is, what a page is, what a cell is — never.

> **Shared machinery, separate surfaces.**
> The reconciler, the host tree, the Tailwind lookups: one copy, internal. The
> elements: one set per format, each behind its own import path.

A PDF is measured and paginated here, and every glyph on the page was placed
by the engine. A spreadsheet has no page and nothing is painted: a cell carries
a **value and a type**, and Excel decides what it looks like when somebody
opens it. Writing `1200` as text into a PDF gives you the characters; writing
it as text into a sheet makes `SUM` return zero. That inversion is why the
crates are separate and why `@imprentajs/react` has no `<Document>` at its root.

DOCX will look like the PDF side and must not reuse it. Word paginates, not us,
so measured widows, per-page headers and running totals — the things this
project exists for — cannot exist there. A shared `<Text>` would have props
that silently do nothing in one target, which is the failure this whole design
refuses.

## Architecture in one screen

Three phases, and the seams between them are the design:

- **Measure** (parallel) — text becomes shaped glyph runs, then `Atom`s. An atom
  is one indivisible slice: a line, a table row, a repeated header.
- **Pack** (serial, pure arithmetic) — places atoms on pages. It sees heights and
  break flags, never text, never fonts. Being serial and in document order is
  what makes running totals and "continued from page 12" possible; being pure
  arithmetic is what makes it fast enough to look ahead instead of guessing.
- **Paint** (per page) — placed atoms become PDF content, then are released.

Two rules follow, and most bugs here come from breaking one of them:

- **A new primitive must not touch the packer.** It adds an IR node, a measurer,
  and a `content::Content` variant. If you find yourself adding a case to
  `pack.rs`, the abstraction is wrong.
- **Composition streams.** Pages are painted and dropped as content arrives, so
  memory is flat per page regardless of length. Anything that holds all pages —
  `<PageCount />` is the only one today — must say so out loud.

No HTML, no CSS engine, no browser. The IR is versioned JSON and the engine does
not know React exists.

## Code style

- Rust 2024, toolchain pinned in `rust-toolchain.toml`. Clippy is `-D warnings`.
- TypeScript strict, ESM in source. Biome formats and lints: 2 spaces, width 100,
  single quotes, semicolons. Run `pnpm run format` rather than hand-aligning.
- Node >= 22. `pnpm` with a workspace catalog — add shared versions to
  `pnpm-workspace.yaml`, not to each package.
- **Exact versions. No `^`, no `~`, anywhere.** `.npmrc` sets `save-exact`, so
  `pnpm add` pins by itself; an upgrade is a commit that says which number
  moved and why. The two exceptions are deliberate: `workspace:*`, and
  `peerDependencies`, which are a requirement placed on somebody else's project
  and have to stay a range — pinning one forces every consumer onto that exact
  build.
- **The newest version is not always the one to pin.** `typescript` and
  `@types/node` are both held back on purpose and say why in
  `pnpm-workspace.yaml`. Read the comment before bumping them.
- Prefer a named type over an inline shape once it is used twice.
- No `unwrap()` on anything a caller controls. A malformed document is a
  `Diagnostic` or an `Err`, never a panic across the napi boundary.

## Comment style

The comments in this repository explain **why**, never **what**. They are prose,
in full sentences, and they say what was considered and rejected. Match it.

```rust
// Advances are stored normalised to the em, not in points. Shaping the same
// string at 7 pt and at 14 pt would otherwise be two cache entries for one
// piece of work, and the cache is where the speed comes from.
```

Not `// normalise advance`. A comment that restates the line below it is noise;
delete it. A module gets a `//!` header saying what it is for and what it
deliberately does not do.

## Commit style

Sentence-case summary that says what the commit *does for someone*, not what it
touched. Then a body in prose explaining the reasoning, what was measured, and
what went wrong on the way.

```
Move the font loader out of the CLI, where a server cannot reach it

`google()` was inside `@imprentajs/cli`, and the CLI is no use in a NestJS
controller. It is `@imprentajs/fonts` now, which needs neither the CLI nor a
config file. …
```

No `feat:`/`fix:` prefixes, no bullet lists of files, no "as requested".

## Traps that have already caught us

- **A stale `.node` is a lying green test.** See rule 2 above.
- **`#[serde(tag = "...")]` buffers the whole subtree** before it knows the
  variant (serde-rs/serde#1407). `ir::Node` has a hand-written `Deserialize` for
  that reason and `tests/allocations.rs` holds the line with a counting
  allocator. Do not "simplify" it back to a derive.
  **Adjacent tagging — `tag` *and* `content` — does not buffer**, and a
  hand-written reader for one is wasted work. `#[serde(untagged)]` always does,
  which is what `Color` used to use and what most of a styled cell cost.
- **The size of a hot type is its memory profile.** A `Style` sat inline in
  `xlsx::ir::Cell` and made every cell 168 bytes whether or not it had one;
  boxing it took a row from 2,380 bytes to 1,560. Reason about what is big and
  repeated, not only about what allocates.
- **`react-reconciler`'s published types lag its runtime.** React 19 renamed the
  priority hooks and `createContainer` takes ten arguments; get the order wrong
  and component errors are swallowed silently. `host.ts` declares the shapes it
  actually calls.
- **The build must not need `react/jsx-dev-runtime`.** Vite is configured with
  `esbuild: { jsx: 'automatic', jsxDev: false }`; a production install may not
  have the dev runtime at all.
- **Measure before claiming a speed or memory result.** A number in the README
  or in a comment needs a benchmark anybody can rerun — say where it is and on
  what input, and never benchmark a debug build.
- **Only one `#[global_allocator]` in the whole workspace.** Two addons each
  declaring one do not coexist in a process, and a service that prints and
  exports loads both. It showed as the PDF engine aborting on a font that was
  perfectly good. `packages/xlsx/test/together.test.ts` holds the line.
- **Look at the file.** Several defects — a list marker touching its text, a
  band overlapping the last line, a date showing as 46237 — pass every test
  and are obvious the moment anybody opens the thing. `openpyxl` is stricter
  than the readers used in tests and has already found one.

## Where to write things down

- Why a piece of code is the way it is, and what was rejected → a comment on
  that code. That is what the comment style above is for.
- Something a user must know → `README.md`.
- Something a contributor or an agent must know → the nearest `CLAUDE.md`.

## Releasing

Two workflows and one rule: nothing publishes without a changeset.

```bash
pnpm changeset          # describe the change; the file is the release note
```

Landing that on `main` opens a "chore: version packages" pull request.
Merging **that** cuts the tag and the GitHub release, and the release is what
triggers the addons being compiled — five targets, two crates — and all fifteen
packages going to npm.

- **Fifteen, not five.** `@imprentajs/pdf` and `@imprentajs/xlsx` each ship an
  `@imprentajs/<pkg>-<platform>` alongside them, generated at publish time. The
  `.node` files cannot be built on one machine, which is the whole reason
  `publish.yml` has a matrix.
- **The five share one version**, as a `fixed` group. A `@imprentajs/cli` that
  shipped against a `@imprentajs/pdf` it was never tested with is the failure
  this repository is most careful about.
- **We are in pre-release**, tag `alpha`, published as `next`. So
  `npm i @imprentajs/cli` gets nothing and `npm i @imprentajs/cli@next` gets this.
  Leaving pre-release is `changeset pre exit`, deliberately.
- **Do not count `.changeset/*.md` to work out which phase a release is in.**
  In pre-release `changeset version` leaves them on disk. That is what
  `scripts/pending-changesets.mjs` is for, and the comment in it says why.
