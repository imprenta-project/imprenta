import { readFileSync, rmSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { type RenderOptions, render, renderToFile } from '../dist/index.js';

const fixtures = fileURLToPath(new URL('../../../crates/imprenta-pdf/tests', import.meta.url));
const font = (name: string) => readFileSync(join(fixtures, 'fonts', name));
const image = (name: string) => readFileSync(join(fixtures, 'images', name));

const roman: RenderOptions = {
  fonts: [{ weight: 'regular', data: font('Roboto-Regular.ttf') }],
};

const page = { width: 595, height: 842 };

const hello = JSON.stringify({
  page,
  children: [{ t: 'text', runs: [{ text: 'Hola' }] }],
});

/** A document long enough that rendering it takes real time. */
const long = (rows: number) =>
  JSON.stringify({
    page,
    children: [
      {
        t: 'table',
        columns: [
          { width: { unit: 'percent', value: 0.5 } },
          { width: { unit: 'percent', value: 0.5 } },
        ],
        rows: Array.from({ length: rows }, (_, i) => ({
          cells: [{ text: `Asiento ${i}` }, { text: '1.200,00' }],
        })),
      },
    ],
  });

const scratch = (name: string) => {
  const path = join(tmpdir(), `imprenta-${name}-${process.pid}.pdf`);
  rmSync(path, { force: true });
  return path;
};

describe('render', () => {
  it('returns a PDF for a document declared as JSON', async () => {
    const out = await render(hello, roman);

    expect(Buffer.from(out.pdf.subarray(0, 5)).toString()).toBe('%PDF-');
    expect(out.pages).toBe(1);
    expect(out.bytes).toBe(out.pdf.length);
  });

  it('reports nothing when the fonts cover the text', async () => {
    const out = await render(hello, roman);

    expect(out.diagnostics).toEqual([]);
  });

  it('takes a second face and uses it for bold runs', async () => {
    const out = await render(
      JSON.stringify({
        page,
        children: [
          {
            t: 'text',
            runs: [{ text: 'Total ' }, { text: '7.400,00', weight: 'bold' }],
          },
        ],
      }),
      {
        fonts: [
          { weight: 'regular', data: font('Roboto-Regular.ttf') },
          { weight: 'bold', data: font('Roboto-Bold.ttf') },
        ],
      },
    );

    expect(out.diagnostics).toEqual([]);
    expect(out.pages).toBe(1);
  });

  it('takes an image as bytes and asks for nothing else about it', async () => {
    const out = await render(
      JSON.stringify({ page, children: [{ t: 'image', src: 'logo', width: 120 }] }),
      { ...roman, images: [{ name: 'logo', data: image('logo.png') }] },
    );

    expect(out.pages).toBe(1);
    expect(out.diagnostics).toEqual([]);
  });

  it('tells the caller what it could not draw', async () => {
    const out = await render(
      JSON.stringify({ page, children: [{ t: 'text', runs: [{ text: '日本語' }] }] }),
      roman,
    );

    expect(out.diagnostics.join(' ')).toContain('missing-glyph');
  });

  it('paginates a document too long for one page', async () => {
    const out = await render(long(400), roman);

    expect(out.pages).toBeGreaterThan(1);
  });
});

describe('the event loop', () => {
  it('keeps turning while a document renders', async () => {
    // The whole reason both calls are promises. A NestJS service that stopped
    // answering health checks for twenty seconds per report would be worse
    // than the Chromium it replaced, so the work has to leave the main thread.
    let ticks = 0;
    const timer = setInterval(() => {
      ticks += 1;
    }, 1);

    const started = performance.now();
    const out = await render(long(20_000), roman);
    const elapsed = performance.now() - started;
    clearInterval(timer);

    expect(out.pages).toBeGreaterThan(100);
    // The document has to be slow enough for the question to mean anything;
    // if a later optimisation makes this instant, make the document longer
    // rather than dropping the check.
    expect(elapsed).toBeGreaterThan(50);
    // Blocking would leave this at zero. It sits in the hundreds, and gets
    // higher rather than lower on a slower machine, so the floor is safe.
    expect(ticks).toBeGreaterThan(20);
  });

  it('runs several documents at once', async () => {
    const outs = await Promise.all([
      render(long(500), roman),
      render(long(500), roman),
      render(long(500), roman),
      render(long(500), roman),
    ]);

    for (const out of outs) {
      expect(Buffer.from(out.pdf.subarray(0, 5)).toString()).toBe('%PDF-');
    }
  });
});

describe('renderToFile', () => {
  it('writes the document and hands back no bytes at all', async () => {
    const path = scratch('to-file');

    const out = await renderToFile(hello, path, roman);

    expect(out).not.toHaveProperty('pdf');
    expect(out.path).toBe(path);
    expect(statSync(path).size).toBe(out.bytes);
    expect(readFileSync(path).subarray(0, 5).toString()).toBe('%PDF-');
  });

  it('produces the same document as the buffered route', async () => {
    const path = scratch('same');

    const buffered = await render(hello, roman);
    await renderToFile(hello, path, roman);

    expect(readFileSync(path).equals(buffered.pdf)).toBe(true);
  });
});

describe('what the caller gets wrong', () => {
  it('rejects malformed JSON, saying where', async () => {
    await expect(render('{ "page": { "width": 595, }}', roman)).rejects.toThrow(/line|column/);
  });

  it('rejects a document with no fonts', async () => {
    await expect(render(hello, { fonts: [] })).rejects.toThrow(/font/i);
  });

  it('rejects a font weight it does not know, naming the ones it does', async () => {
    const wrong = render(hello, {
      fonts: [{ weight: 'semibold', data: font('Roboto-Regular.ttf') }],
    });

    await expect(wrong).rejects.toThrow(/semibold/);
    await expect(wrong).rejects.toThrow(/bold/);
  });

  it('rejects an unreadable image by the name it came in as', async () => {
    const wrong = render(hello, {
      ...roman,
      images: [{ name: 'sello', data: Buffer.from('<svg/>') }],
    });

    await expect(wrong).rejects.toThrow(/sello/);
  });

  it('rejects a path it cannot write, saying which', async () => {
    const nowhere = '/no/such/directory/out.pdf';

    await expect(renderToFile(hello, nowhere, roman)).rejects.toThrow(nowhere);
  });

  it('does not take the process down when handed nonsense', async () => {
    await expect(render('', roman)).rejects.toThrow();
    await expect(render('null', roman)).rejects.toThrow();
    await expect(render('[]', roman)).rejects.toThrow();

    // Still alive, and still correct.
    expect((await render(hello, roman)).pages).toBe(1);
  });
});

describe('defaults', () => {
  it('treats a font with no declared weight as regular', async () => {
    const out = await render(hello, { fonts: [{ data: font('Roboto-Regular.ttf') }] });

    expect(out.pages).toBe(1);
  });

  it('accepts a document with no images at all', async () => {
    const out = await render(hello, { fonts: roman.fonts });

    expect(out.pages).toBe(1);
  });
});
