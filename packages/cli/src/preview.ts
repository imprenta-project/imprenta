import { createReadStream } from 'node:fs';
import { readFile, stat } from 'node:fs/promises';
import { createServer as createHttpServer, type ServerResponse } from 'node:http';
import { extname, join, normalize, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createServer, type ViteDevServer } from 'vite';
import { type Context, check } from './checks.js';
import type { Loaded } from './config.js';
import { type Found, findDocuments, previewProps } from './documents.js';
import { checkWorkbook, refuse } from './sheets.js';

/**
 * The preview server.
 *
 * One Vite server, and it exists for the author's documents: `ssrLoadModule`
 * compiles their TSX and resolves their imports, with exactly the settings
 * `build.ts` uses, so a document cannot come out one way on screen and another
 * in CI. What the browser gets back is the PDF itself, which is the only way
 * to be sure the preview shows what the engine produced rather than an
 * approximation of it.
 *
 * The browser app is no longer part of that graph. It is compiled by
 * `vite.config.ts` into `app/dist` and served from there as plain files, so a
 * project that wants a PDF does not have to install Tailwind and a component
 * library to see one. Vite runs in middleware mode for the same reason: it
 * should serve nothing on its own — only what the handlers below hand it.
 */
export interface Preview {
  server: ViteDevServer;
  url: string;
  close(): Promise<void>;
}

/** The built UI, which ships in the package. Absent until `build` has run. */
const UI = fileURLToPath(new URL('../app/dist', import.meta.url));

export async function startPreview(loaded: Loaded, port: number): Promise<Preview> {
  const server = await createServer({
    configFile: false,
    server: {
      middlewareMode: true,
      // The project is where every document is, and it need not be under the
      // directory the CLI was started from.
      fs: { allow: [process.cwd(), loaded.documentsDir] },
    },
    appType: 'custom',
    // The automatic runtime, so a document does not have to import React to
    // use JSX. Not the development one: `build` cannot depend on
    // `react/jsx-dev-runtime`, which a production install may not have, and
    // the two compiles have to be the same compile.
    esbuild: { jsx: 'automatic', jsxDev: false },
    plugins: [api(loaded)],
    logLevel: 'silent',
  });

  const http = createHttpServer(server.middlewares);
  await new Promise<void>((ready, failed) => {
    http.once('error', failed);
    http.listen(port, ready);
  });

  const address = http.address();
  const chosen = typeof address === 'object' && address ? address.port : port;

  return {
    server,
    url: `http://localhost:${chosen}/`,
    close: async () => {
      await new Promise<void>((done) => http.close(() => done()));
      await server.close();
    },
  };
}

/** Everything the browser asks for that is not a file. */
function api(loaded: Loaded) {
  let assets: Promise<Assets> | null = null;
  const load = () => {
    assets ??= readAssets(loaded);
    return assets;
  };

  // The last render, kept so that asking for the report and then the bytes
  // does not render twice. One entry: the preview shows one document.
  let last: { id: string; bytes: Buffer } | null = null;

  return {
    name: 'imprenta:preview',
    configureServer(server: ViteDevServer) {
      // Added here rather than after `createServer`, because a middleware
      // registered inside this hook runs before Vite's own — and Vite's own
      // would happily serve the project's source tree at the URLs the app
      // asks for.
      const changed = watchDocuments(server, loaded);

      server.middlewares.use(async (req, res, next) => {
        const url = new URL(req.url ?? '/', 'http://localhost');
        if (!url.pathname.startsWith('/api/')) {
          return serve(url.pathname, res, next);
        }
        try {
          if (url.pathname === '/api/changes') {
            return changed.subscribe(res);
          }
          if (url.pathname === '/api/documents') {
            return json(res, await listing(loaded));
          }
          if (url.pathname === '/api/render') {
            const id = url.searchParams.get('id') ?? '';
            const done = await render(server, loaded, await load(), id);
            last = { id, bytes: done.bytes };
            return json(res, done.report);
          }
          if (url.pathname === '/api/image') {
            // The grid draws a sheet's pictures, and the IR carries only the
            // name — which is the whole point of it. This is where the bytes
            // behind that name live, and the only reason the preview can show
            // a letterhead at all.
            const wanted = url.searchParams.get('name') ?? '';
            const image = (await load()).images.find((each) => each.name === wanted);
            if (!image) {
              res.statusCode = 404;
              return res.end('no image of that name is configured');
            }
            res.setHeader('content-type', kind(image.data));
            res.setHeader('cache-control', 'no-store');
            return res.end(image.data);
          }
          if (url.pathname === '/api/file') {
            const id = url.searchParams.get('id') ?? '';
            const done =
              last?.id === id && url.searchParams.has('cached')
                ? null
                : await render(server, loaded, await load(), id);
            const format = (url.searchParams.get('format') ?? 'pdf') as 'pdf' | 'xlsx';
            return send(
              res,
              id,
              done?.bytes ?? (last?.bytes as Buffer),
              done?.report.format ?? format,
            );
          }
          res.statusCode = 404;
          res.end('no such endpoint');
        } catch (error) {
          // Shown in the preview rather than only in the terminal: the author
          // is looking at the browser, and a blank pane explains nothing.
          res.statusCode = 500;
          res.setHeader('content-type', 'application/json');
          res.end(JSON.stringify({ error: describe(error) }));
        }
      });
    },
  };
}

/**
 * Telling the browser that something on disk moved.
 *
 * This used to ride on Vite's HMR socket as a custom event, which worked
 * because the app was served from source and had a Vite client in it. It is a
 * build now, so it does not — and a one-way stream of one kind of message is
 * the smaller thing to want anyway. `EventSource` reconnects by itself, which
 * the socket did not do when the server restarted.
 *
 * Coalesced, because one save is several filesystem events and each one used
 * to cost a full render. 40ms is under the time it takes to notice and well
 * over the time an editor takes to finish writing.
 */
function watchDocuments(server: ViteDevServer, loaded: Loaded) {
  const listeners = new Set<ServerResponse>();
  let pending: NodeJS.Timeout | null = null;

  // The documents need not be under the directory Vite was rooted at, and the
  // watcher only follows the root and whatever the module graph has pulled in.
  server.watcher.add(loaded.documentsDir);

  server.watcher.on('all', () => {
    if (pending) {
      return;
    }
    pending = setTimeout(() => {
      pending = null;
      for (const listener of listeners) {
        listener.write('event: changed\ndata: {}\n\n');
      }
    }, 40);
    // Nothing else is waiting on this timer, and holding the process open for
    // it would keep `imprenta dev` alive after Ctrl-C.
    pending.unref();
  });

  return {
    subscribe(res: ServerResponse) {
      res.writeHead(200, {
        'content-type': 'text/event-stream',
        'cache-control': 'no-store',
        connection: 'keep-alive',
      });
      // Sent immediately so the browser's `EventSource` opens rather than
      // sitting on a response with no body yet.
      res.write(': open\n\n');
      listeners.add(res);
      res.on('close', () => listeners.delete(res));
    },
  };
}

const CONTENT_TYPES: Record<string, string> = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.woff2': 'font/woff2',
  '.png': 'image/png',
  '.map': 'application/json',
  '.json': 'application/json',
};

/**
 * The built UI, off disk.
 *
 * Everything under `app/dist` is content-hashed by Vite except `index.html`
 * and what is copied from `public/`, so the hashed assets are immutable and
 * the page is not cached at all — otherwise an upgrade of the CLI would leave
 * yesterday's page pointing at assets that no longer exist.
 */
async function serve(pathname: string, res: ServerResponse, next: () => void) {
  const wanted = pathname === '/' ? '/index.html' : pathname;
  // `..` in a URL is a request for a file outside the package, and there is
  // never a good reason for one.
  const file = resolve(join(UI, normalize(wanted)));
  if (file !== UI && !file.startsWith(UI + sep)) {
    return next();
  }

  const found = await stat(file).catch(() => null);
  if (!found?.isFile()) {
    if (wanted !== '/index.html') {
      return next();
    }
    // The one failure worth explaining rather than 404ing: somebody is running
    // from a checkout where `vite build` has not run.
    res.statusCode = 503;
    res.setHeader('content-type', 'text/plain; charset=utf-8');
    return res.end(
      'The preview UI has not been built.\n' +
        'Run `pnpm --filter @imprentajs/cli build` and start `imprenta dev` again.\n',
    );
  }

  res.statusCode = 200;
  res.setHeader('content-type', CONTENT_TYPES[extname(file)] ?? 'application/octet-stream');
  res.setHeader(
    'cache-control',
    wanted === '/index.html' ? 'no-store' : 'public, max-age=31536000, immutable',
  );
  createReadStream(file).pipe(res);
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

async function readAssets(loaded: Loaded): Promise<Assets> {
  const fonts = await Promise.all(
    loaded.fonts.map(async (font) => ({
      weight: font.weight,
      italic: font.italic,
      data: await readFile(font.path).catch(() => {
        throw new Error(`the font at ${font.path} could not be read`);
      }),
    })),
  );
  const images = await Promise.all(
    loaded.images.map(async (image) => ({
      name: image.name,
      data: await readFile(image.path).catch(() => {
        throw new Error(`the image at ${image.path} could not be read`);
      }),
    })),
  );
  return { fonts, images };
}

async function listing(loaded: Loaded) {
  const documents = await findDocuments(loaded.documentsDir);
  return {
    documentsDir: loaded.documentsDir,
    configPath: loaded.path,
    documents: documents.map((d: Found) => ({ id: d.id, group: d.group })),
  };
}

interface Report {
  id: string;
  /** Which of the two the component turned out to declare. */
  format: 'pdf' | 'xlsx';
  /** Pages, for a document. Sheets, for a workbook. */
  parts: number;
  bytes: number;
  checks: ReturnType<typeof check>;
  /** The IR the engine was handed, for the source view and for the grid. */
  ir: unknown;
}

/**
 * Renders one document and looks it over.
 *
 * The checks run on the IR rather than on the PDF: by the time it is bytes,
 * a six point heading is a six point heading and nothing can tell it was
 * meant to be sixteen.
 */
async function render(
  server: ViteDevServer,
  loaded: Loaded,
  assets: Assets,
  id: string,
): Promise<{ bytes: Buffer; report: Report }> {
  const documents = await findDocuments(loaded.documentsDir);
  const found = documents.find((d) => d.id === id);
  if (!found) {
    throw new Error(`there is no document called ${JSON.stringify(id)}`);
  }

  // Dropped from the module graph first, then loaded. Waiting for the file
  // watcher would be enough almost always, and "almost" is the problem: a
  // save the watcher coalesced or missed leaves the preview showing a page
  // that no longer exists, which is the one thing a preview must never do.
  // Recompiling a handful of TSX files costs milliseconds, and a render only
  // happens on a click or a save.
  for (const module of server.moduleGraph.idToModuleMap.values()) {
    server.moduleGraph.invalidateModule(module);
  }
  const module = await server.ssrLoadModule(found.path);
  const Component = module.default;
  if (typeof Component !== 'function') {
    throw new Error(`${found.id} has no default export, and that is where the document goes`);
  }

  const { createElement } = await import('react');
  const { renderAny } = await import('@imprentajs/react/any');

  // Which format it is is only knowable once the component has run.
  const rendered = await renderAny(createElement(Component, previewProps(Component)));

  if (rendered.format === 'xlsx') {
    // Before the write, not after: `missing-image` is a fault the engine
    // refuses outright, so checked afterwards it would never be reached and
    // the author would get the engine's message with no sheet named.
    const checks = checkWorkbook(rendered.ir, { images: assets.images.map((i) => i.name) });
    refuse(checks);

    const { write } = await import('@imprentajs/xlsx');
    // The same images the page side gets. A sheet's picture names one, and
    // the CLI is the only thing that knows where the project keeps it.
    const out = await write(JSON.stringify(rendered.ir), { images: assets.images });
    return {
      bytes: Buffer.from(out.xlsx),
      report: {
        id,
        format: 'xlsx',
        parts: out.sheets,
        bytes: out.bytes,
        checks,
        ir: rendered.ir,
      },
    };
  }

  if (assets.fonts.length === 0) {
    throw new Error(
      'no fonts are configured, and the engine has none of its own: add `fonts` to imprenta.config.ts',
    );
  }

  const { render: toPdf } = await import('@imprentajs/pdf');
  const out = await toPdf(JSON.stringify(rendered.ir), {
    fonts: assets.fonts.map((f) => ({ weight: f.weight, italic: f.italic, data: f.data })),
    images: assets.images,
  });

  return {
    bytes: Buffer.from(out.pdf),
    report: {
      id,
      format: 'pdf',
      parts: out.pages,
      bytes: out.bytes,
      checks: check(rendered.ir, out.diagnostics, context(assets)),
      ir: rendered.ir,
    },
  };
}

/**
 * What an image turned out to be, from its own first bytes.
 *
 * The extension the project happened to give the file is not evidence: the
 * engine identifies an image by its header and so does this, or the preview
 * would show a broken picture for a PNG somebody called `.jpg`.
 */
function kind(data: Buffer): string {
  return data[0] === 0x89 && data[1] === 0x50 ? 'image/png' : 'image/jpeg';
}

const TYPES = {
  pdf: 'application/pdf',
  xlsx: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
} as const;

function send(
  res: import('node:http').ServerResponse,
  id: string,
  bytes: Buffer,
  format: 'pdf' | 'xlsx',
) {
  res.statusCode = 200;
  res.setHeader('content-type', TYPES[format]);
  res.setHeader('cache-control', 'no-store');
  // Named, so "Download" saves `factura.pdf` rather than the endpoint's query.
  // A spreadsheet is an attachment because no browser can show one, and
  // pretending otherwise gets you a download named after the query string.
  const how = format === 'pdf' ? 'inline' : 'attachment';
  res.setHeader('content-disposition', `${how}; filename="${id.split('/').pop()}.${format}"`);
  res.end(bytes);
}

function json(res: import('node:http').ServerResponse, body: unknown) {
  res.statusCode = 200;
  res.setHeader('content-type', 'application/json');
  res.setHeader('cache-control', 'no-store');
  res.end(JSON.stringify(body));
}

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
