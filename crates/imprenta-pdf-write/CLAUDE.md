# imprenta-pdf-write

The bytes. Objects out as they are finished, cross-reference table at the end.
See the root `CLAUDE.md` for the rules that apply everywhere.

## Why this crate exists rather than a dependency

The engine used [krilla](https://github.com/LaurenzV/krilla), which is very
good and does far more than this. Two things were wrong with it *here*, and
neither is a defect in krilla:

1. **It keeps every finished page.** `register_page` pushes onto a `Vec` and
   `ChunkContainer::finish` walks the whole collection twice — once to
   renumber every object reference, once to write them — into a buffer
   preallocated the size of the file. Measured on a ten thousand page ledger:
   **5.25 KB retained per page against 2.22 KB of output**. Patching
   `register_page` to serialise eagerly was tried and yields 22%, because the
   chunk container is not an output stream, it is another `Vec`.
2. **Its `rayon` feature recursed once per page on a target with no threads.**
   `Deferred::new` spawns, `Deferred::wait` drains the queue with
   `yield_now()`, and each job it runs re-enters `wait`. Every document
   trapped at around 2 400 pages, whatever was on them. Gating the feature
   fixed it, and it is the kind of thing that is invisible until it is a
   stack overflow in production.

What is here is **the subset this engine can reach and nothing else**, built
on the same two crates krilla is: `pdf-writer` for the object syntax and
`subsetter` for the fonts. So the risky parts — CFF and TrueType subsetting,
CID fonts, `ToUnicode` — are not re-derived.

## Deliberately absent

No transparency group, blend mode, pattern, shading, clip path, tagged
structure tree, PDF/A conformance, encryption, outline, or form field. Every
one of them is a real feature of the format; none is reachable from
`imprenta-pdf`'s IR. Adding one because it seems useful adds a second thing to
keep working. **If the IR cannot express it, it does not belong here.**

## The three things that are easy to get wrong

**1. A subset renumbers the glyphs, and the content stream is written first.**
Roboto's `P` is glyph 51 and might be glyph 1 in the subset. The page names
the *subset* id — which is decided as pages are painted, by
`GlyphRemapper::remap`, and the font built from that same remapper at the end
agrees by construction. Glyph zero is claimed for `.notdef` before anything
else can take the slot.

**2. `ToUnicode` fails silently.** A page with no map, or the wrong one,
renders perfectly and its text cannot be copied, searched, indexed or read
aloud. The map comes from the byte ranges the shaper recorded per glyph; those
ranges have their own tests in `imprenta-pdf/src/shape.rs`, because getting
them wrong looks like nothing at all. `beginbfchar` takes **at most a hundred
entries** — a reader that enforces the limit drops the rest.

**3. An offset recorded before the bytes moved is a file that opens blank.**
Readers follow the cross-reference table rather than scanning. `tests/writing.rs`
walks every entry and checks the object really starts there; keep that test.

## Blocks, and why the output is not a `Vec<u8>`

`blocks.rs`, and the comment there is the argument. Short version: growing a
`Vec` to thirty-two megabytes allocates sixteen, asks for thirty-two, copies,
and frees the sixteen — and inside a WebAssembly module every hole left behind
is held for the life of the instance. The output buffer alone was **fifty-seven
megabytes of a seventy-nine megabyte footprint** for a twenty-two megabyte
file.

A block that is full is never touched again. The one copy that remains is
`into_vec`, which is the price of handing back something contiguous; it is made
at exactly the right size and frees each block as it drains.

## Testing

- Unit tests in the module for the parts with an answer you can write down:
  subset ids, kerning nudges, the CMap's batching, what a PNG's alpha becomes.
- `tests/writing.rs` reads the *file* back. That is the product; a writer whose
  every method behaved and whose output no reader could open would pass a unit
  test of each method in turn.
- `tests/memory.rs` is its own binary because its allocator is global, and it
  asserts the shape rather than a number: what is live at the end, over the
  file itself, must not grow with the page count.
- `examples/sample.rs` writes a document to look at. **Look at it.** Text
  drawn upside down, a page that opens blank and a logo on a black square all
  pass every assertion here.

```bash
cargo run -p imprenta-pdf-write --example sample -- /tmp/sample.pdf
```

Open it, and check the text copies out of it.
