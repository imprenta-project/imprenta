/**
 * Declaring a paginated document.
 *
 * `@imprentajs/react/pdf` — everything here is about a page: margins, bands that
 * repeat, totals that carry across a break. None of it means anything to a
 * spreadsheet, which is why it is behind its own import rather than shared
 * with one.
 */
export type { Chunk, ChunkOptions } from './chunks.js';
export { toChunks } from './chunks.js';
export * from './elements.js';
export * from './ir.js';
export { render, toDocument } from './render.js';
