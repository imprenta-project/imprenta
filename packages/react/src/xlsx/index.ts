/**
 * Declaring a spreadsheet.
 *
 * `@imprentajs/react/xlsx` — a grid of typed values. Nothing here knows about a
 * page, because a workbook has none, and the one thing it insists on is the
 * thing a document never has to think about: whether a cell holds the text
 * `1200` or the number 1200.
 */
export * from './elements.js';
export * from './ir.js';
export { render, toWorkbook } from './render.js';
export { resolve } from './tailwind.js';
