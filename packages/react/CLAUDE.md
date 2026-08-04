# @imprentajs/react

A real React reconciler that produces the IR. See the root `CLAUDE.md` for the
rules that apply everywhere.

## Layout

Shared machinery at the top, one folder per output format below it. Nothing at
the root exports an element: `@imprentajs/react` is types and the Tailwind
vocabulary, and everything else is behind `/pdf` or `/xlsx`, so the import line
always says which target it means.

| | |
|---|---|
| `host.ts` | the `react-reconciler` host config. Builds a tree of `Instance`s |
| `reconcile.ts` | runs React, captures errors, returns the host tree |
| `element.ts` | the `host()` trick, and `<Theme>`, which belongs to neither format |
| `tailwind.ts` | the lookups no format owns: colours, lengths, the type scale |
| `theme.ts` | **generated** from Tailwind 4.3.3, oklch converted to sRGB |
| `pdf/` | `<Document>`, bands, tables, `toChunks`, and what a **page** can honour |
| `xlsx/` | `<Workbook>`, typed cells, merges, and what a **cell** can honour |

Each format folder has its own `ir.ts`, `elements.ts`, `tailwind.ts` and
`render.ts`. They do not import each other.

## The one rule that shapes everything else

**This package depends on React and nothing else.** No native addon, no CLI, no
file system. That is what lets a document be declared in a place the engine
cannot be installed, and it is why fonts are *not* declared in a component —
resolving `<Font family="Roboto"/>` would drag the addon in. A document says
what it looks like; which files it is set in belongs to whoever prints it.

## Rules

- **`ir.ts` and `ir.rs` are one contract in two languages.** Change one and you
  change both, in the same commit, with an end-to-end test that renders. The
  React side once invented its own border shape and only the end-to-end test
  caught it — serde had been quietly dropping the field. There are two of these
  contracts now, one per format.
- **Neither format is the default.** A new format gets a folder and a subpath,
  and the root keeps exporting no elements at all. Putting one at the root
  would make the others look like afterthoughts, and hide which target an
  import means.
- **Tailwind has one resolver and a capability table per format.**
  `bg-slate-100` is a fill in both; `p-4` is padding on a page and has no
  counterpart in a cell. Both directions are refused **by name**, and the
  message says where the thing the author reached for actually lives.
- **A class the engine cannot honour is an error, by name, at render time.**
  `flex` has no meaning on a page, `hover:` needs a state paper is never in,
  `w-1/2` cannot be expressed for a box. Never silently ignore one: a document
  that quietly came out wrong is the failure mode this whole project exists to
  avoid.
- **`prune` before emitting.** The IR carries no `undefined` fields; a smaller
  document is fewer allocations on the Rust side, and absent is meaningful.
- **Text is styled runs, never a bare string.** `runs`/`inline` flatten what is
  nested inside `<Text>` and **join neighbours that match**, so where JSX
  happened to split a string never reaches the shaper.
- **`theme.ts` is generated.** Do not hand-edit a colour. Regenerate from
  Tailwind, and keep `PT_PER_REM` and the rem→pt arithmetic where a `<Theme>`
  can rescale the whole document with one number.
- **`toDocument` is async on purpose** — a component may suspend, and a producer
  that has to change shape later is worse to inflict on callers than an `await`
  they did not need.
- **A component that throws must reach the caller.** React's default is to log
  and commit a tree with a hole in it, which is right for a screen and wrong for
  a document nobody looks at until it is printed.

## `react-reconciler` traps

The published types lag the runtime. React 19 renamed the priority hooks
(`resolveUpdatePriority`) and `createContainer` now takes **ten** arguments —
get the order wrong and the error handlers land in the wrong slots, so
component errors vanish silently. `host.ts` declares the shapes it actually
calls in one place, deliberately, instead of casting at each call site.

Every method in the host config is there because leaving it out made a test
fail. Do not delete one because it looks unused.

## Testing

```bash
pnpm --filter @imprentajs/react test
```

- `end-to-end.test.tsx` renders through the real engine. It is the only test
  that can catch an IR field the two sides disagree about, so keep it covering
  every prop you add.
- `stream-to-pdf.test.tsx` pins that `toChunks` produces byte-identical output
  to declaring the document whole. It needs a **freshly built** `@imprentajs/pdf`
  — a stale addon makes it pass by comparing two equally wrong documents.
- Measure points, not shapes, when a prop is a length: `spaceAfter` was silently
  dropped inside `<Row>` and only an assertion in points found it.
