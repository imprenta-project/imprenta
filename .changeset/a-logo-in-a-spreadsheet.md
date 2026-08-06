---
"@imprentajs/xlsx": minor
"@imprentajs/react": minor
"@imprentajs/cli": minor
---

A sheet can carry a picture.

```tsx
<Cell>
  <Image src="logo" width={120} />
</Cell>
```

```ts
await write(ir, { images: [{ name: 'logo', data: bytes }] });
```

Written **inside a cell** rather than declared beside the sheet with a row and
a column. Coordinates would be a second thing to keep in step with the rows:
insert a header above and the logo stays where it was, which is the bug an
anchor exists to prevent. It floats over the grid rather than sitting in the
cell, so the cell it names stays blank and `COUNTA` is unaffected.

There is a width and no height, exactly as on a page. The image's own pixels
give the ratio, because asking for both is the one way to squash a logo and it
is always somebody copying the numbers off the last one. The anchor is
`oneCellAnchor` for the same reason — the two-cell form stretches a picture
between two corners, so widening a column distorts it.

The bytes go beside the IR and never in it. A workbook is JSON that goes on a
queue, into a cache or through an HTTP body, and a logo inline would make every
one of those carry it. An image the sheets never name is not written into the
package at all, and a picture naming an image nobody handed over stops the
write — rather than producing a workbook with a hole where the logo was, which
nobody notices until a customer opens it.

Four parts go into the package for one picture — the media, the drawing, the
drawing's relationships and the sheet's — and Excel opens a repair dialog
naming none of them if any is missing. A workbook without a picture is byte for
byte the workbook it was before: no extra parts, no extra namespace on the
worksheet, no extra content type.

`imprenta dev` and `imprenta build` hand the project's configured images to the
sheet side as they already did to the page side, and the checks gained
`missing-image` for a workbook, so a picture with no image behind it is named
with its sheet instead of surfacing as a write that failed.

That rule runs **before** the write, which is the only place it can. The engine
refuses to produce a workbook with a hole where the logo was, so a rule checked
after the write can never fire: every workbook that would trip it fails first,
with the engine's wording, naming neither the sheet nor the document. `refuse()`
holds the short list of rules the writer will not get past.

A workbook whose rows are streamed takes its images the same way — `new Book(
sheets, { images })`. It has to, because a letterhead on a million-row ledger is
the case streaming exists for, and a picture is placed from the rows and merges
that were *written* rather than the ones declared up front: a session keeps the
heights of the rows a placed picture's block covers, and forgets every other row
as it goes, so a centred logo lands where the same workbook declared whole puts
it without the sheet costing anything to hold. The one thing it cannot recover
from is a merge declared after its own rows have gone past — there is nothing
left to measure by then, so it says so rather than guessing.

The header reader that turns eight bytes of PNG or JPEG into a size has moved
to `imprenta-core`. An image's own size is vocabulary rather than model, and
two readers of the same eight bytes would be two places for a JPEG with an EXIF
segment in front of its frame header to be got wrong.

`imprenta dev` draws it too. The grid is built from the IR, the IR carries only
a name, so a sheet with a letterhead showed a workbook the file did not
contain — the engine wrote the picture and the author could not see it. The
preview now serves the project's configured images and hangs each one off its
anchor cell, spilling over the cells beside it the way Excel does.

A picture can be placed inside the block it hangs from, with `align` and
`valign`. This has to be the engine's arithmetic and cannot be the author's:
centring needs the picture's *height*, and the height comes from the image's
own pixels, which only the engine has read. Somebody computing an offset by
hand gets it right for the logo in front of them and wrong for the next one —
silently, because the picture is still on the page.

```tsx
<Cell colSpan={2} rowSpan={4}>
  <Image src="logo" width={120} align="center" valign="center" />
</Cell>
```

The block is the **merge** that swallowed the anchor, not the anchor cell: a
letterhead hangs off `A1` and the author combined `A1:B4` to make room for it,
so centring in `A1` alone would put it in the corner of what the eye reads as
one cell. `offset` still applies on top, as a nudge from wherever the placement
put it. A picture larger than its block is left in the corner rather than given
a negative offset, which would push it off the edge of the sheet where it can
neither be seen nor dragged back.

Sizing a merged block means converting Excel's column unit — characters of the
body font — into points, which is the one measurement in a workbook that is not
a length. `imprenta dev` does not repeat that arithmetic: the grid draws a merge
as one element, so the browser already knows how big it is.
