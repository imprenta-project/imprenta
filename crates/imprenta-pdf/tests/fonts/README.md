# Test fonts

Vendored so that text metrics are byte-identical on every machine and every CI
runner. Shaping against a system font would make the same assertion pass on
macOS and fail on Linux, which is worse than having no assertion.

## Roboto-Regular.ttf

- Source: <https://github.com/googlefonts/roboto-2> (`src/hinted/Roboto-Regular.ttf`)
- Copyright 2015 Google Inc. All Rights Reserved.
- Licence: **Apache-2.0** — the same licence as this project.

Used for Latin metrics. RTL and CJK test fonts will be added alongside it when
those code paths get their own tests; they are much larger, so they are not
vendored before they are needed.
