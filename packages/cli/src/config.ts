import { existsSync } from 'node:fs';
import { rm } from 'node:fs/promises';
import { isAbsolute, join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { cacheGoogleFont, type GoogleFace } from '@imprentajs/fonts';
import { build } from 'esbuild';

/** What a project tells the CLI about itself. */
export interface Config {
  /** Where the documents live, relative to the config. */
  documents?: string;
  /** Port for `imprenta dev`. */
  port?: number;
  /**
   * Fonts every document is rendered with.
   *
   * The engine embeds and subsets what it is given and has no system fonts to
   * fall back on, which is what makes a document print the same everywhere.
   */
  fonts?: FontSource[];
  /** Images documents refer to by name. */
  images?: Record<string, string>;
}

export interface FontConfig {
  path: string;
  weight?: 'regular' | 'bold';
  italic?: boolean;
}

/** Either a file the project has, or a face to fetch from Google. */
export type FontSource = FontConfig | GoogleFace;

const isGoogle = (font: FontSource): font is GoogleFace => 'family' in font;

/**
 * Types a config file that nothing imports a type from.
 *
 * It returns its argument and does nothing else. That is the point: an editor
 * can check the object, and the file stays a plain default export.
 */
export function defineConfig(config: Config): Config {
  return config;
}

export interface Loaded {
  config: Config;
  /** Where the config was found, or null if there was none. */
  path: string | null;
  /** Absolute, resolved against the config rather than the shell. */
  documentsDir: string;
  fonts: { path: string; weight: 'regular' | 'bold'; italic: boolean }[];
  images: { name: string; path: string }[];
}

const NAMES = [
  'imprenta.config.ts',
  'imprenta.config.mts',
  'imprenta.config.js',
  'imprenta.config.mjs',
];

/**
 * Reads the config next to `from`, or settles for the defaults.
 *
 * Every path in it is resolved against the config file, never the working
 * directory: running the CLI from a subfolder must find the same documents.
 */
export async function loadConfig(from: string): Promise<Loaded> {
  const root = resolve(from);
  const path = NAMES.map((name) => join(root, name)).find((candidate) => existsSync(candidate));
  const config = path ? await read(path) : {};

  const against = (relative: string) => (isAbsolute(relative) ? relative : join(root, relative));

  // Fetched once, into a folder beside the project rather than inside it —
  // gitignorable, and shared between runs so a build does not go to Google
  // once per document.
  const cache = join(root, '.imprenta/fonts');
  const fonts = await Promise.all(
    (config.fonts ?? []).map(async (font) => ({
      path: isGoogle(font) ? await cacheGoogleFont(font, cache) : against(font.path),
      weight: font.weight ?? 'regular',
      italic: font.italic ?? false,
    })),
  );

  return {
    // The defaults are filled in rather than left implicit: whatever reads
    // this should not have to know them a second time.
    config: { documents: './documents', ...config },
    path: path ?? null,
    documentsDir: against(config.documents ?? './documents'),
    fonts,
    images: Object.entries(config.images ?? {}).map(([name, at]) => ({
      name,
      path: against(at),
    })),
  };
}

/**
 * Compiles the config and imports it.
 *
 * Bundled rather than merely transpiled, so a config can import from the
 * project it configures — a shared theme, a list of fonts — which is most of
 * the reason for it being code instead of JSON.
 */
async function read(path: string): Promise<Config> {
  // Written beside the config rather than in a temp folder: what the bundle
  // leaves as an import — `@imprentajs/cli`, anything else installed — is
  // resolved by Node from wherever the file sits, and a folder in /tmp has
  // no `node_modules` above it. Vite compiles its own config the same way.
  const out = `${path}.${Date.now()}.mjs`;
  try {
    await build({
      entryPoints: [path],
      outfile: out,
      bundle: true,
      format: 'esm',
      platform: 'node',
      target: 'node22',
      // Anything installed stays installed: bundling `@imprentajs/cli` into a
      // throwaway file would hand the config a second copy of this module.
      packages: 'external',
      logLevel: 'silent',
    });

    // Cache-busted, so a restarted `dev` reads the edited config rather than
    // the one Node happens to have imported already.
    const loaded = await import(`${pathToFileURL(out).href}?t=${Date.now()}`);
    const config = loaded.default;
    if (!config || typeof config !== 'object') {
      throw new Error('it has no default export, and that is where the config goes');
    }
    return config as Config;
  } catch (cause) {
    throw new Error(`${path} could not be read: ${message(cause)}`, { cause });
  } finally {
    await rm(out, { force: true });
  }
}

function message(cause: unknown): string {
  if (cause && typeof cause === 'object' && 'errors' in cause) {
    // esbuild reports the line and column; a bare "Build failed" does not.
    const errors = (cause as { errors: { text: string; location?: { line: number } }[] }).errors;
    return errors
      .map((e) => (e.location ? `line ${e.location.line}: ${e.text}` : e.text))
      .join('; ');
  }
  return cause instanceof Error ? cause.message : String(cause);
}
