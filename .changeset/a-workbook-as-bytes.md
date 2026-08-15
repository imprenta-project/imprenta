---
"@imprentajs/react": patch
"@imprentajs/xlsx": patch
---

`render` for a workbook hands back UTF-8 bytes, not one JS string.

V8 caps a string at 512 MiB of characters, and a fourteen-million-cell export
died there — while serialising, before the writer was involved at all (#12).
The IR is now stringified in pieces small enough that no single string
approaches the cap, each piece encoded as it is made, and the result returned
as a `Uint8Array`. The bytes are byte for byte what `JSON.stringify` would
have produced — the test asserts equality against the real thing, because
"equivalent" is where two serialisers start to drift.

`write` and `writeToFile` accept the bytes as they always accepted a string —
their signatures now say so — so a caller that pipes `render` into `write`
changes nothing. A caller that treated the result as a string wraps it in
`TextDecoder` or, better, stops needing to.

Measured end to end: a million declared rows to a finished `.xlsx` in 7.2 s,
where the string cap used to end the run before the engine saw a byte.
