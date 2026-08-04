import { readFile } from 'node:fs/promises';
import { cacheGoogleFont, type Fetcher, type GoogleFace } from './google.js';

/** A font file the caller already has. */
export interface FontFile {
  path: string;
  weight?: 'regular' | 'bold';
  italic?: boolean;
}

export type FontSource = FontFile | GoogleFace;

/** Exactly what the engine's `render` takes. */
export interface LoadedFont {
  weight: 'regular' | 'bold';
  italic: boolean;
  data: Buffer;
}

export interface LoadOptions {
  /** Where fetched fonts are kept. */
  cache: string;
  /** For tests, and for anyone who has to go through a proxy. */
  fetcher?: Fetcher;
}

const isGoogle = (font: FontSource): font is GoogleFace => 'family' in font;

/**
 * Reads the fonts a document is to be set in.
 *
 * The bytes come back in the shape the engine takes, so a controller can hand
 * them straight over. Anything from Google is fetched once and cached; a file
 * the caller already has is simply read, and the two mix in the order they
 * were given — which is the common case, since a brand's own typeface is not
 * on Google and its body text usually is.
 */
export async function loadFonts(fonts: FontSource[], options: LoadOptions): Promise<LoadedFont[]> {
  return Promise.all(
    fonts.map(async (font) => ({
      weight: font.weight ?? 'regular',
      italic: font.italic ?? false,
      data: isGoogle(font)
        ? await readFile(await cacheGoogleFont(font, options.cache, options.fetcher))
        : await readFile(font.path).catch(() => {
            throw new Error(`the font at ${font.path} could not be read`);
          }),
    })),
  );
}
