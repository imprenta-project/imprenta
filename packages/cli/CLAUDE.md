# @imprentajs/cli

`imprenta init`, `imprenta dev`, `imprenta build`, and the rules that say
whether a document is any good. See the root `CLAUDE.md` for the rules that
apply everywhere.

## Files

```
src/bin.ts        the three commands and their flags
src/init.ts       scaffolds a project that renders on the first run
src/config.ts     imprenta.config.ts — bundled with esbuild, then imported
src/documents.ts  finding documents, and PreviewProps
src/preview.ts    the server: renders documents, serves the built UI
src/build.ts      every document to a file, the same compile the preview uses
src/checks.ts     the rules for a document. Nine, plus whatever the engine reported
src/sheets.ts     the rules for a workbook. A different list, because the medium is
vite.config.ts    builds app/ into app/dist. Never reached at the author's run time
components.json   shadcn's config. Aliases point at app/src, not src
app/              the preview UI (React, Tailwind, shadcn/ui on Base UI)
```

`app/dist` is what ships in `files`, not `app/`. `pnpm --filter @imprentajs/cli
build` runs `tsc` **and** `vite build`; a test that starts the preview needs
the second one to have happened, and says so if it has not.

## Rules

- **The UI is a build, and the author never installs it.** Tailwind, Base UI,
  lucide and React DOM are `devDependencies`. They compile into `app/dist` here
  and the server sends those files as bytes. Anything that would make the
  author's `node_modules` carry a piece of this UI is the wrong change: they
  installed a document engine, not a design system.
- **`pnpm --filter @imprentajs/cli dev:app`** is how to work on the UI — Vite
  with hot reload on 4322, proxying `/api` to a real `imprenta dev` on 4321.
  Editing `app/` and reloading the preview does nothing; it serves the build.
- **One Vite server, and it is for documents.** It runs in middleware mode with
  `appType: 'custom'` so it serves nothing of its own — its root is the
  author's project, and letting its static middleware answer would hand out
  their source tree. The UI and the API are middlewares registered inside
  `configureServer`, which is what puts them ahead of Vite's.
- **`build` and `dev` must compile documents identically.** A document cannot
  come out one way on screen and another in CI. Both use `ssrLoadModule` with
  the same options and no plugins: `jsx: 'automatic'` with **`jsxDev: false`**,
  because a build has no business needing `react/jsx-dev-runtime`, which a
  production install may not have at all.
- **The browser hears about changes over SSE**, at `/api/changes`. There is no
  Vite client on the page to carry a custom HMR event any more, and the events
  are coalesced — one save is several filesystem events and each used to cost
  a full render.
- **Invalidate the whole module graph before every render.** Waiting on the file
  watcher is right almost always, and "almost" means a coalesced save leaves the
  preview showing a page that no longer exists — the one thing a preview must
  never do. Recompiling a few TSX files costs milliseconds.
- **The preview shows the real PDF**, not a rendering of the page. If that ever
  becomes an approximation, the tool is lying.
- **One failing document does not stop a build.** A build of forty is worth
  knowing about in one go.
- **Every path resolves against the config file, never the shell.** Running the
  CLI from a subfolder must find the same documents.
- **`init` never overwrites.** It refuses when `imprenta.config.ts` exists,
  skips files already there, and *appends* to `.gitignore` rather than replacing
  it. It scaffolds a project that renders straight away — a font already chosen
  and fetched from Google, an invoice with its own `PreviewProps` — because
  empty scaffolding makes the first run a blank page.
  It does **not** create a `package.json`, install dependencies, or add scripts.

## Writing a check

`check(document, diagnostics, context?)` in `src/checks.ts`. A rule:

- **Reads the IR, not the PDF.** By the time it is bytes, a six-point heading is
  a six-point heading and nothing can tell it was meant to be sixteen.
- **Is about the finished sheet, never taste.** Type too small to read, ink the
  printer cannot reach, a colour that will not survive being printed grey. Not
  "this heading could be bigger".
- **Names what it found and where.** `logo is 240×80 printed 400pt wide, which
  is 43 dpi` — not "low resolution image".
- **Stays quiet when it does not know.** `Context` is optional throughout: a
  rule with no font list must not accuse every document of missing every face.
- **Collapses.** Findings group by `rule + signature` with an `occurrences`
  count. The same fault in three hundred rows is one line saying three hundred.
- **Runs before the write, if the engine would refuse it.** `missing-image` is
  the first of these: the writer will not produce a workbook with a hole where
  the logo was, so a rule checked afterwards can never fire — every workbook
  that would trip it fails first, with the engine's own wording and no sheet
  named. `refuse()` in `sheets.ts` holds the list, and it is one entry long
  because a rule that stops a build has to be worth stopping it for.

Engine diagnostics (`warning[code]: …`) are parsed into the same list, so the
author reads one panel rather than two.

## Testing

```bash
pnpm --filter @imprentajs/cli test
pnpm --filter @imprentajs/cli test -- checks   # one file
```

- Component tests use happy-dom and Testing Library. Call `cleanup()`.
- **Ask the DOM what a user would ask it.** `getByRole('treeitem', { current:
  'page' })`, not `querySelector('.on')`. A class name is a styling decision and
  a test that asserts one breaks on every redesign while proving nothing; a role
  and an accessible name are the thing the component promises.
- **Build before running the server tests.** `preview.test.ts` fetches the UI
  the server serves, and the UI is `app/dist`. Turbo's `test` depends on
  `build`, so `pnpm run ci` is fine; running vitest directly in a clean checkout
  is not, and the server answers with an instruction rather than a 404.
- **Do not use an empty document to isolate a rule.** `empty-document` is itself
  a rule, and four tests that took that shortcut broke when it was added — the
  convenience was hiding the case. Give the fixture real content.
- Test `init` the way it will be met: scaffold into an empty folder, then run
  `build` without touching anything. That is what found the `jsx-dev-runtime`
  failure.
- `examples/facturacion/documents/mal-hecho.tsx` breaks rules on purpose. Add to
  it when you add a rule.

## The look

`app/src/style.css` is the only place a colour is named: the brand primitives
in one block, the semantic roles they resolve to in another, mapped onto
shadcn's variables. Everything else asks for a role. If a component needs a
colour that has no role yet, add the role — do not reach for a hex.

- **A component comes from the CLI, not from a copy-paste.** `pnpm dlx
  shadcn@latest add <name>` with `components.json` as it stands (Base UI, the
  `base-nova` style). Items from other registries can land in `src/components`
  rather than `app/src/components` — check, and move them.
- **Vermilion is the brand, and it is also the error colour here.** The
  brandbook forbids that on marketing surfaces, where the mark is the only red
  thing; in a linter panel the convention won. Warnings are `signal.warn`, and
  both carry an icon and a word so the colour is never the only signal.
- **The sheet is paper in both modes.** `--sheet` and the grid's own rules do
  not follow the theme: the fills a workbook declares were chosen against white,
  and a shaded row under a dark theme comes out white on white. Chrome follows
  the mode; the artefact does not.
- **Look at it.** `pnpm --filter facturacion dev`, both modes, both formats.
  `mal-hecho` is the document that fills the checks panel.
