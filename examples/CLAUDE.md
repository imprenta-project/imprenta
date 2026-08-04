# examples

Two workspace projects that exist to be run, not to be imported.

```
facturacion/   a project laid out the way one really would be — for the preview
backend/       a controller: React in, PDF bytes out, no CLI anywhere
```

```bash
pnpm --filter facturacion dev     # the preview server
pnpm --filter backend start       # an HTTP controller returning a PDF
```

## Rules

- **This is the only place domain vocabulary belongs.** Invoices, ledgers,
  Spanish — all fine here, none of it in `crates/` or `packages/`. The engine is
  general purpose and these are examples of one use of it.
- **They must work from a clean checkout.** No fonts checked in: `facturacion`
  uses `google('Roboto', …)`, which fetches into `.imprenta/fonts` on first run.
  Nothing to hunt for, nothing binary in git.
- **`backend/` loads its fonts once at process start, never per request.** It is
  a worked example of the recommended shape, so the shape has to be right.
- **`mal-hecho.tsx` gets things wrong on purpose.** It is how the preview's
  rules are seen working. Every new rule in `@imprentajs/cli` adds a case to it,
  and the count in `README.md` moves with it.
- Keep `informe.tsx` table-less. A document with no table in it is a real
  document and every early benchmark forgot that.
