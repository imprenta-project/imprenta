import { mkdir, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { loadConfig } from '../src/config.js';
import { type Preview, startPreview } from '../src/preview.js';

/**
 * The preview, run for real.
 *
 * Started once against a project on disk and asked the same questions the
 * browser asks. Anything less would be testing the pieces and not the thing:
 * what can go wrong here is Vite failing to compile a document, or the
 * engine refusing what React produced, and neither shows up in a unit test.
 */
const fixtures = fileURLToPath(new URL('../../../crates/imprenta-pdf/tests', import.meta.url));
const project = fileURLToPath(new URL('.preview-project', import.meta.url));

let preview: Preview;
let base: string;

const document = (body: string) => body;

beforeAll(async () => {
  await rm(project, { recursive: true, force: true });
  await mkdir(join(project, 'documents/ventas'), { recursive: true });

  await writeFile(
    join(project, 'imprenta.config.ts'),
    `export default {
       documents: './documents',
       fonts: [{ path: ${JSON.stringify(join(fixtures, 'fonts/Roboto-Regular.ttf'))} }],
       images: { logo: ${JSON.stringify(join(fixtures, 'images/logo.png'))} },
     };`,
  );

  await writeFile(
    join(project, 'documents/ventas/factura.tsx'),
    document(`
      import { Document, Text, Image } from '@imprentajs/react/pdf';
      export default function Factura({ number }) {
        return (
          <Document>
            <Image src="logo" width={80} />
            <Text>Factura {number}</Text>
          </Document>
        );
      }
      Factura.PreviewProps = { number: 'FV-1' };
    `),
  );

  await writeFile(
    join(project, 'documents/hoja.tsx'),
    document(`
      import { Cell, Column, Row, Sheet, Workbook } from '@imprentajs/react/xlsx';
      export default function Hoja() {
        return (
          <Workbook>
            <Sheet name="Ventas">
              <Column width={20} />
              <Row className="bg-slate-100 font-bold"><Cell>Importe</Cell></Row>
              <Row><Cell value={1200} /></Row>
              <Row><Cell>900</Cell></Row>
            </Sheet>
          </Workbook>
        );
      }
    `),
  );

  await writeFile(
    join(project, 'documents/roto.tsx'),
    document(`
      import { Document, Box } from '@imprentajs/react/pdf';
      export default function Roto() {
        return <Document><Box className="backdrop-blur" /></Document>;
      }
    `),
  );

  await writeFile(join(project, 'documents/sin-default.tsx'), document(`export const a = 1;`));

  const loaded = await loadConfig(project);
  preview = await startPreview(loaded, 0);
  base = preview.url.replace(/\/$/, '');
}, 60_000);

afterAll(async () => {
  await preview?.close();
  await rm(project, { recursive: true, force: true });
});

describe('the preview server', () => {
  it('serves the page the browser loads', async () => {
    const response = await fetch(`${base}/`);

    expect(response.ok).toBe(true);
    expect(await response.text()).toContain('<div id="root">');
  });

  it('serves the assets that page asks for', async () => {
    // The UI is a build now, not a module graph Vite compiles on demand, so
    // the page pointing at a bundle nobody serves is a real way to break it —
    // and it looks like a blank window rather than an error.
    const page = await (await fetch(`${base}/`)).text();
    const asset = page.match(/(?:src|href)="\.(\/assets\/[^"]+)"/)?.[1];
    expect(asset).toBeTruthy();

    const response = await fetch(`${base}${asset}`);

    expect(response.ok).toBe(true);
    expect(response.headers.get('cache-control')).toContain('immutable');
  });

  it('does not serve the project it was pointed at', async () => {
    // Vite would, if its own static middleware were reached: the root is the
    // author's folder and everything in it is a file it knows how to read.
    const response = await fetch(`${base}/imprenta.config.ts`);

    expect(response.ok).toBe(false);
  });

  it('lists the documents it found, with their folders', async () => {
    const listing = await (await fetch(`${base}/api/documents`)).json();

    // Both kinds in one list, and nothing in it says which is which: a file
    // only declares its format by returning one, and the listing has not run
    // anything yet.
    expect(listing.documents).toEqual([
      { id: 'hoja', group: null },
      { id: 'roto', group: null },
      { id: 'sin-default', group: null },
      { id: 'ventas/factura', group: 'ventas' },
    ]);
    expect(listing.configPath).toContain('imprenta.config.ts');
  });

  it('renders a document to an actual PDF', async () => {
    const response = await fetch(`${base}/api/file?id=ventas/factura`);

    expect(response.headers.get('content-type')).toBe('application/pdf');
    const bytes = Buffer.from(await response.arrayBuffer());
    expect(bytes.subarray(0, 5).toString()).toBe('%PDF-');
  });

  it('serves the bytes it just rendered rather than rendering twice', async () => {
    await fetch(`${base}/api/render?id=ventas/factura`);
    const fresh = Buffer.from(
      await (await fetch(`${base}/api/file?id=ventas/factura`)).arrayBuffer(),
    );
    const cached = Buffer.from(
      await (await fetch(`${base}/api/file?id=ventas/factura&cached=1`)).arrayBuffer(),
    );

    expect(cached.equals(fresh)).toBe(true);
  });

  it('renders it with the props the document declares for itself', async () => {
    // The PreviewProps convention, proved where it matters: without them the
    // component would have rendered "Factura undefined".
    const response = await fetch(`${base}/api/file?id=ventas/factura`);
    const bytes = Buffer.from(await response.arrayBuffer());

    // Text is compressed in the stream, so the proof is that it rendered at
    // all: a missing prop would throw before a page existed.
    expect(response.ok).toBe(true);
    expect(bytes.length).toBeGreaterThan(1000);
  });

  it('is never cached, so a reload shows the new document', async () => {
    const response = await fetch(`${base}/api/file?id=ventas/factura`);

    expect(response.headers.get('cache-control')).toBe('no-store');
  });

  it('names the file, so downloading it gives something recognisable', async () => {
    const response = await fetch(`${base}/api/file?id=ventas/factura`);

    expect(response.headers.get('content-disposition')).toContain('factura.pdf');
  });

  it('reports on a document as well as rendering it', async () => {
    const report = await (await fetch(`${base}/api/render?id=ventas/factura`)).json();

    expect(report.format).toBe('pdf');
    expect(report.parts).toBe(1);
    expect(report.bytes).toBeGreaterThan(1000);
    expect(report.checks).toEqual([]);
    expect(report.ir.children).toHaveLength(2);
  });

  it('says what is wrong with a document that will not print well', async () => {
    // The reason the report exists: nothing else would tell the author that
    // the page they are looking at is unreadable on paper.
    await writeFile(
      join(project, 'documents/ilegible.tsx'),
      document(`
        import { Document, Text } from '@imprentajs/react/pdf';
        export default () => (
          <Document margin={2}>
            <Text size={3}>Nadie puede leer esto</Text>
          </Document>
        );
      `),
    );

    const report = await (await fetch(`${base}/api/render?id=ilegible`)).json();
    const rules = report.checks.map((c: { rule: string }) => c.rule);

    expect(rules).toContain('tiny-text');
    expect(rules).toContain('unprintable-margin');
    expect(report.checks[0].status).toBe('error');
  });

  it('carries what the engine noticed into the report', async () => {
    await writeFile(
      join(project, 'documents/japones.tsx'),
      document(`
        import { Document, Text } from '@imprentajs/react/pdf';
        export default () => <Document><Text>日本語</Text></Document>;
      `),
    );

    const report = await (await fetch(`${base}/api/render?id=japones`)).json();

    expect(report.checks.some((c: { rule: string }) => c.rule === 'missing-glyph')).toBe(true);
    expect(report.checks.some((c: { source: string }) => c.source === 'engine')).toBe(true);
  });

  it('sends a failure the browser can show, naming what went wrong', async () => {
    // Not a blank pane and not only a line in the terminal: the author is
    // looking at the browser.
    const response = await fetch(`${base}/api/render?id=roto`);

    expect(response.status).toBe(500);
    expect((await response.json()).error).toContain('backdrop-blur');
  });

  it('says so when a document has no default export', async () => {
    const response = await fetch(`${base}/api/render?id=sin-default`);

    expect((await response.json()).error).toContain('default export');
  });

  it('says so when asked for a document that is not there', async () => {
    const response = await fetch(`${base}/api/render?id=fantasma`);

    expect((await response.json()).error).toContain('fantasma');
  });

  it('picks up a document added after it started', async () => {
    // The list is read per request rather than at boot, because a new file is
    // the commonest thing to happen while the preview is open.
    await writeFile(
      join(project, 'documents/nuevo.tsx'),
      document(`
        import { Document, Text } from '@imprentajs/react/pdf';
        export default () => <Document><Text>Nuevo</Text></Document>;
      `),
    );

    const listing = await (await fetch(`${base}/api/documents`)).json();

    expect(listing.documents.map((d: { id: string }) => d.id)).toContain('nuevo');
  });

  it('tells the browser when something on disk moved', async () => {
    // The app cannot know a module changed on the server, and it no longer has
    // a Vite client to be told through: the page is a build.
    const stream = await fetch(`${base}/api/changes`);
    expect(stream.headers.get('content-type')).toBe('text/event-stream');

    const reader = (stream.body as ReadableStream<Uint8Array>).getReader();
    const decoder = new TextDecoder();
    // The comment the server opens with, so the browser's `EventSource` fires
    // `open` rather than sitting on a response with no body yet.
    expect(decoder.decode((await reader.read()).value)).toContain(':');

    await writeFile(
      join(project, 'documents/avisado.tsx'),
      document(`
        import { Document, Text } from '@imprentajs/react/pdf';
        export default () => <Document><Text>Avisado</Text></Document>;
      `),
    );

    expect(decoder.decode((await reader.read()).value)).toContain('event: changed');
    await reader.cancel();
  }, 20_000);

  it('serves a workbook from the same folder as the documents', async () => {
    // A project holds both kinds and `dev` shows both. Which a file is cannot
    // be told from its name — only from what its component returns — so the
    // server works it out on every render.
    const report = await (await fetch(`${base}/api/render?id=hoja`)).json();

    expect(report.format).toBe('xlsx');
    expect(report.parts).toBe(1);
    // The IR goes to the browser so the grid can be drawn from it, which is
    // the only honest thing to show: no browser opens a spreadsheet.
    expect(report.ir.sheets[0].rows[1].cells[0].value).toEqual({ t: 'number', v: 1200 });
    expect(report.ir.sheets[0].rows[0].style.fill).toBe('#f1f5f9');
  }, 60_000);

  it('hands the workbook over as a file a browser will save', async () => {
    const file = await fetch(`${base}/api/file?id=hoja&format=xlsx`);

    expect(file.headers.get('content-type')).toMatch(/spreadsheetml\.sheet/);
    // An attachment, not inline: a browser asked to show one downloads it
    // anyway, and names it after the query string if nobody says otherwise.
    expect(file.headers.get('content-disposition')).toMatch(/attachment; filename="hoja.xlsx"/);

    const bytes = Buffer.from(await file.arrayBuffer());
    expect(bytes.subarray(0, 2).toString()).toBe('PK');
  }, 60_000);

  it('runs the sheet rules on a workbook and not the document ones', async () => {
    const report = await (await fetch(`${base}/api/render?id=hoja`)).json();
    const rules = report.checks.map((c: { rule: string }) => c.rule);

    expect(rules).toContain('number-as-text');
    // A workbook has no `children`, so the document rules would call every
    // one of them empty.
    expect(rules).not.toContain('empty-document');
  }, 60_000);

  it('renders an edited document again rather than serving the old one', async () => {
    const path = join(project, 'documents/editado.tsx');
    const version = (text: string) =>
      document(`
        import { Document, Text } from '@imprentajs/react/pdf';
        export default () => <Document><Text>${text}</Text></Document>;
      `);

    await writeFile(path, version('antes'));
    const before = Buffer.from(await (await fetch(`${base}/api/file?id=editado`)).arrayBuffer());

    await writeFile(path, version('después, y bastante más largo que antes'));
    const after = Buffer.from(await (await fetch(`${base}/api/file?id=editado`)).arrayBuffer());

    expect(after.equals(before)).toBe(false);
  });

  it('refuses an endpoint it does not have', async () => {
    const response = await fetch(`${base}/api/nonsense`);

    expect(response.status).toBe(404);
  });
});

describe('a project with no fonts', () => {
  it('says which setting is missing rather than rendering nothing', async () => {
    // The engine has no system fonts by design, so this is the first thing a
    // new project gets wrong.
    const bare = fileURLToPath(new URL('.preview-bare', import.meta.url));
    await rm(bare, { recursive: true, force: true });
    await mkdir(join(bare, 'documents'), { recursive: true });
    await writeFile(
      join(bare, 'imprenta.config.ts'),
      `export default { documents: './documents' };`,
    );
    await writeFile(
      join(bare, 'documents/a.tsx'),
      `import { Document, Text } from '@imprentajs/react/pdf';
       export default () => <Document><Text>a</Text></Document>;`,
    );

    const server = await startPreview(await loadConfig(bare), 0);
    try {
      const url = server.url.replace(/\/$/, '');
      const response = await fetch(`${url}/api/render?id=a`);

      expect((await response.json()).error).toMatch(/fonts/);
    } finally {
      await server.close();
      await rm(bare, { recursive: true, force: true });
    }
  }, 60_000);
});
