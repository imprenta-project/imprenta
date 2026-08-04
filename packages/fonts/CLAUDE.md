# @imprentajs/fonts

Fetch and cache the faces a document is set in. See the root `CLAUDE.md` for
the rules that apply everywhere.

```
src/google.ts   google(), parseFaces(), cacheGoogleFont()
src/load.ts     loadFonts() — gives back exactly the shape render(ir, { fonts }) takes
```

## Why it is its own package

`google()` used to live in the CLI, and the CLI is no use in a NestJS
controller. This package needs neither the CLI nor a config file, so a server
can load its fonts once at startup and a build can load the same ones. The CLI
re-exports it so a config still needs one import.

`loadFonts` returns `{ weight, italic, data }[]` — the argument `render` takes,
with nothing to write in between. It mixes Google faces with local files in the
order given, because a brand's own typeface is not on Google and its body text
usually is.

## Rules

- **Ask Google for TrueType explicitly.** The CSS API serves woff2 to a modern
  browser and EOT to something old enough; the engine reads neither. Only an
  Android 2.2 User-Agent yields TTF. This is fragile by nature — hence the next
  rule.
- **Validate the bytes before caching them.** A woff2 or an EOT written into the
  cache surfaces later as an unreadable-font error a long way from the cause.
  The EOT case is real: it arrived with the magic `1ce40100` and "Roboto" in
  UTF-16.
- **Downloads dedupe and temp names are unique.** Two components asking for the
  same heading face started two downloads that wrote the same `.part` file; one
  renamed it and the other found it gone. Two fixes for two problems — an
  in-flight map so a face is fetched once, and a temp name unique per attempt as
  well as per process, since two builds sharing a cache would race the same way.
- **A build with no network must work off the cache.** Fetch once into
  `.imprenta/fonts`, use it from there — the way `next/font/google` self-hosts.
- **The `fetcher` is injectable** so tests never touch the network. Keep it that
  way; a test that reaches Google is a test that fails on a train.

## Testing

```bash
pnpm --filter @imprentajs/fonts test
```

Tests pass a fake fetcher. The race above was found by a test that asked for
the same face twice concurrently — write that kind of test for anything that
touches the cache directory.
