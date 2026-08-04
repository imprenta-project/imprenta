import { mkdtempSync, rmSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { afterAll, describe, expect, it, vi } from 'vitest';
import { type Fetcher, google, loadFonts } from '../src/index.js';

const scratch = fileURLToPath(new URL('.load', import.meta.url));
const made: string[] = [];
const dir = () => {
  const d = mkdtempSync(`${scratch}-`);
  made.push(d);
  return d;
};
afterAll(() => {
  for (const d of made) rmSync(d, { recursive: true, force: true });
});

const CSS = `
@font-face { font-family: 'Roboto'; font-style: normal; font-weight: 400;
  src: url(https://fonts.gstatic.com/regular.ttf) format('truetype'); }
@font-face { font-family: 'Roboto'; font-style: normal; font-weight: 700;
  src: url(https://fonts.gstatic.com/bold.ttf) format('truetype'); }
`;
const TTF = Buffer.from([0x00, 0x01, 0x00, 0x00]);
const fake = (): Fetcher =>
  vi.fn(async (url: string) =>
    url.includes('googleapis')
      ? { ok: true, text: CSS, bytes: Buffer.alloc(0) }
      : { ok: true, text: '', bytes: Buffer.concat([TTF, Buffer.from(url)]) },
  );

describe('loadFonts', () => {
  it('gives back exactly what the engine takes', async () => {
    // The shape `render(ir, { fonts })` wants, so a controller can hand it
    // straight over without a line in between.
    const fonts = await loadFonts(google('Roboto', { weights: ['regular', 'bold'] }), {
      cache: dir(),
      fetcher: fake(),
    });

    expect(fonts).toHaveLength(2);
    expect(fonts[0]).toMatchObject({ weight: 'regular', italic: false });
    expect(fonts[1]).toMatchObject({ weight: 'bold', italic: false });
    expect(Buffer.isBuffer(fonts[0].data)).toBe(true);
    expect(fonts[0].data.subarray(0, 4)).toEqual(TTF);
  });

  it('reads a file the caller already has', async () => {
    // A brand's own typeface is not on Google, and mixing the two is the
    // common case rather than an exception.
    const fixtures = fileURLToPath(
      new URL('../../../crates/imprenta-pdf/tests/fonts/Roboto-Regular.ttf', import.meta.url),
    );

    const fonts = await loadFonts([{ path: fixtures, weight: 'bold' }], { cache: dir() });

    expect(fonts[0].weight).toBe('bold');
    expect(fonts[0].data.subarray(0, 4)).toEqual(TTF);
  });

  it('mixes the two in the order they were given', async () => {
    const own = fileURLToPath(
      new URL('../../../crates/imprenta-pdf/tests/fonts/Roboto-Bold.ttf', import.meta.url),
    );

    const fonts = await loadFonts([{ path: own, weight: 'bold' }, ...google('Roboto')], {
      cache: dir(),
      fetcher: fake(),
    });

    expect(fonts.map((f) => f.weight)).toEqual(['bold', 'regular']);
  });

  it('fetches each face once even when it is asked for twice', async () => {
    const fetcher = fake();

    await loadFonts([...google('Roboto'), ...google('Roboto')], { cache: dir(), fetcher });

    // Two calls for the one face: the stylesheet and the file.
    expect(fetcher).toHaveBeenCalledTimes(2);
  });

  it('says which file it could not read', async () => {
    const asked = loadFonts([{ path: '/no/such/font.ttf' }], { cache: dir() });

    await expect(asked).rejects.toThrow(/no\/such\/font\.ttf/);
  });

  it('needs no cache directory of its own choosing', async () => {
    // A server with a read-only working directory should still be able to
    // say where fonts go.
    const home = dir();

    const fonts = await loadFonts(google('Roboto'), { cache: home, fetcher: fake() });

    expect(fonts).toHaveLength(1);
  });
});
