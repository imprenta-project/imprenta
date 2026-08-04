/**
 * What every output format shares, and nothing that belongs to one.
 *
 * There is deliberately no `<Document>` here. A document and a workbook are
 * not the same thing with different options — one is measured and paginated,
 * the other is a grid of typed values that Excel formats when somebody opens
 * it — and putting either at the root would make the other look like an
 * afterthought. Import the one you mean:
 *
 * ```ts
 * import { Document, Text } from '@imprentajs/react/pdf';
 * import { Workbook, Sheet } from '@imprentajs/react/xlsx';
 * ```
 *
 * What is here is the vocabulary they have in common, which is the same split
 * the Rust side makes between `imprenta-core` and the format crates.
 */

export { Theme, type ThemeProps } from './element.js';
export { PT_PER_REM, type Theme as ThemeTokens } from './tailwind.js';
export { COLORS, RADIUS, TEXT } from './theme.js';
