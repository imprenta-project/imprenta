import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { createServer } from 'vite';
import { type Context, check, type Finding } from './checks.js';
import type { Loaded } from './config.js';
import { findDocuments, previewProps } from './documents.js';
import { checkWorkbook, refuse } from './sheets.js';

export interface BuildOptions {
  /** Where the files go. */
  out: string;
  /** One document's id, when only one is wanted. */
  only?: string;
}

/** What became of one document. */
export interface BuiltDocument {
  id: string;
  /** Which of the two it turned out to declare. */
  format: 'pdf' | 'xlsx';
  path?: string;
  /** Pages, for a document. Sheets, for a workbook. */
  parts: number;
  bytes: number;
  diagnostics: string[];
  checks: Finding[];
  /** Set when this document did not build. The rest still did. */
  error?: string;
}

/**
 * Renders every document in a project to a file.
 *
 * The same compile the preview uses, so a document cannot come out one way on
 * screen and another in CI: Vite loads the TSX, React produces the IR, the
 * engine prints it, and the checks run on what it was handed.
 *
 * One document failing does not stop the rest. A build of forty is worth
 * knowing about in one go, and stopping at the first hides how much else is
 * broken.
 */
export async function buildAll(config: Loaded, options: BuildOptions): Promise<BuiltDocument[]> {
  const documents = await findDocuments(config.documentsDir);
  const wanted = options.only ? documents.filter((d) => d.id === options.only) : documents;
  if (wanted.length === 0) {
    return [];
  }

  const server = await createServer({
    configFile: false,
    server: { middlewareMode: true, fs: { allow: [process.cwd(), config.documentsDir] } },
    appType: 'custom',
    // The automatic runtime, so a document does not have to import React to
    // use JSX — the preview does the same through its React plugin, and the
    // two compiles have to agree. Not the development one: a build has no
    // business needing `react/jsx-dev-runtime`, which a production install
    // may not even have.
    esbuild: { jsx: 'automatic', jsxDev: false },
    logLevel: 'silent',
  });

  try {
    const assets = await readAssets(config);
    const done: BuiltDocument[] = [];

    for (const document of wanted) {
      try {
        done.push(await one(server, document, assets, options.out));
      } catch (error) {
        done.push({
          id: document.id,
          // Unknown, and pdf is the honest guess for a file that did not get
          // far enough to say. Nothing reads it on a failed build.
          format: 'pdf',
          parts: 0,
          bytes: 0,
          diagnostics: [],
          checks: [],
          error: error instanceof Error ? error.message : String(error),
        });
      }
    }
    return done;
  } finally {
    await server.close();
  }
}

interface Assets {
  fonts: { weight: string; italic: boolean; data: Buffer }[];
  images: { name: string; data: Buffer }[];
}

/**
 * What the rules need to know about the project.
 *
 * A picture's pixel dimensions are read here rather than asked of the author:
 * the file says so, and the engine reads the same header for the same reason.
 */
function context(assets: Assets): Context {
  return {
    faces: assets.fonts.map((font) => ({
      weight: font.weight === 'bold' ? ('bold' as const) : ('regular' as const),
      italic: font.italic,
    })),
    images: Object.fromEntries(
      assets.images.flatMap((image) => {
        const size = pixels(image.data);
        return size ? [[image.name, size] as const] : [];
      }),
    ),
  };
}

/** PNG and JPEG dimensions, from the header alone. */
function pixels(bytes: Buffer): { width: number; height: number } | null {
  if (bytes.length > 24 && bytes.subarray(12, 16).toString() === 'IHDR') {
    return { width: bytes.readUInt32BE(16), height: bytes.readUInt32BE(20) };
  }
  if (bytes.length > 4 && bytes[0] === 0xff && bytes[1] === 0xd8) {
    let i = 2;
    while (i + 9 < bytes.length) {
      while (bytes[i] === 0xff) i += 1;
      const marker = bytes[i];
      i += 1;
      const length = bytes.readUInt16BE(i);
      if (marker >= 0xc0 && marker <= 0xcf && ![0xc4, 0xc8, 0xcc].includes(marker)) {
        return { width: bytes.readUInt16BE(i + 5), height: bytes.readUInt16BE(i + 3) };
      }
      if (marker === 0xda) break;
      i += length;
    }
  }
  return null;
}

async function readAssets(config: Loaded): Promise<Assets> {
  return {
    fonts: await Promise.all(
      config.fonts.map(async (font) => ({
        weight: font.weight,
        italic: font.italic,
        data: await readFile(font.path).catch(() => {
          throw new Error(`the font at ${font.path} could not be read`);
        }),
      })),
    ),
    images: await Promise.all(
      config.images.map(async (image) => ({
        name: image.name,
        data: await readFile(image.path).catch(() => {
          throw new Error(`the image at ${image.path} could not be read`);
        }),
      })),
    ),
  };
}

async function one(
  server: Awaited<ReturnType<typeof createServer>>,
  document: { id: string; path: string },
  assets: Assets,
  out: string,
): Promise<BuiltDocument> {
  const module = await server.ssrLoadModule(document.path);
  const Component = module.default;
  if (typeof Component !== 'function') {
    throw new Error(`${document.id} has no default export, and that is where the document goes`);
  }

  const { createElement } = await import('react');
  const { renderAny } = await import('@imprentajs/react/any');

  // Which format it is is only knowable once the component has run: a default
  // export is a function, and what it returns is the answer.
  const rendered = await renderAny(createElement(Component, previewProps(Component)));

  // The folders the author used are kept: flattening would have two
  // `factura.tsx` in different folders overwrite each other, silently.
  const path = join(out, `${document.id}.${rendered.format}`);
  await mkdir(dirname(path), { recursive: true });

  if (rendered.format === 'xlsx') {
    // The document rules are not the sheet rules: run them on a workbook
    // and `empty-document` fires on every one, because a workbook has no
    // children. A panel that says nonsense is worse than no panel.
    const checks = checkWorkbook(rendered.ir, { images: assets.images.map((i) => i.name) });
    refuse(checks);

    const { write } = await import('@imprentajs/xlsx');
    // The same images the page side gets. A sheet's picture names one, and
    // the CLI is the only thing that knows where the project keeps it.
    const built = await write(JSON.stringify(rendered.ir), { images: assets.images });
    await writeFile(path, built.xlsx);

    return {
      id: document.id,
      format: 'xlsx',
      path,
      parts: built.sheets,
      bytes: built.bytes,
      // The writer reports nothing: a spreadsheet has no clipped cell or
      // missing glyph to notice, because nothing is laid out here.
      diagnostics: [],
      checks,
    };
  }

  // Fonts are asked for here rather than up front, because a project of
  // nothing but spreadsheets has no use for one and should not have to
  // configure it.
  if (assets.fonts.length === 0) {
    throw new Error(
      'no fonts are configured, and the engine has none of its own: add `fonts` to imprenta.config.ts',
    );
  }

  const { render: toPdf } = await import('@imprentajs/pdf');
  const built = await toPdf(JSON.stringify(rendered.ir), {
    fonts: assets.fonts.map((f) => ({ weight: f.weight, italic: f.italic, data: f.data })),
    images: assets.images,
  });
  await writeFile(path, built.pdf);

  return {
    id: document.id,
    format: 'pdf',
    path,
    parts: built.pages,
    bytes: built.bytes,
    diagnostics: built.diagnostics,
    checks: check(rendered.ir, built.diagnostics, context(assets)),
  };
}
