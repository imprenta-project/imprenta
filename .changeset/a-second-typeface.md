---
"@imprentajs/pdf": patch
"@imprentajs/react": patch
"@imprentajs/fonts": patch
"@imprentajs/cli": patch
---

A second typeface, and the end of letters set in one font and drawn in
another.

A face could only be told apart by weight and slant. Handing a monospaced
font over as, say, the italic face looked like it worked and printed the wrong
letters: every layout asked for the family of the first font registered, so the
text was shaped in that font and drawn with the other one's file. Nothing said
so. Each face is now shaped in its own family, and the shaper settles on the
face it really used before anything is shaped, so the glyphs and the font they
are drawn with always agree.

On top of that, a family can be named:

```rust
Assets::new()
    .with_font(Face::REGULAR, geist)
    .with_font(Face::family("mono"), geist_mono)
    .with_font(Face::family("mono").bold(), geist_mono_semibold)
```

```json
{ "text": "llm", "family": "mono", "weight": "bold" }
```

`family` on a run and on a table cell; `<Text family="mono">` and
`<Span family="mono">` in React, with `family=""` back to the default inside
a mono paragraph. Fonts are handed over with `family` in `render(ir, { fonts })`,
with `name` in `google('Roboto Mono', { name: 'mono' })` and in a CLI config's
font list — `name`, because `family` there already means Google's family.

A family nobody handed over is reported as `unknown-family` and set in the
default one. A face a family lacks — bold mono when only regular mono was
given — is set in that family's regular, and the CLI's `missing-face` check
now looks for it in the family the run names.
