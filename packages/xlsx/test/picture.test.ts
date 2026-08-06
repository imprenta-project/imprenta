/**
 * A picture, across the boundary: the IR names an image and the bytes travel
 * beside it.
 *
 * The React vocabulary is not tested here — that is `@imprentajs/react` — and
 * neither is the drawing XML, which is `crates/imprenta-xlsx`. What is here is
 * the one thing only this package can get wrong: whether an image handed to a
 * worker reaches the module, and whether it stays behind afterwards.
 */
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { afterAll, describe, expect, it } from 'vitest';
import { close, write } from '../dist/index.js';
import { Book } from '../dist/stream.js';

const LOGO = await readFile(
  fileURLToPath(new URL('../../../crates/imprenta-xlsx/tests/images/logo.png', import.meta.url)),
);

/**
 * The parts in a package, read off its local file headers.
 *
 * A zip reader would be a dependency for one assertion. Every entry begins
 * `PK\x03\x04`, and its name is a length-prefixed run of bytes twenty-six in —
 * which is enough for a package this package wrote, with no encryption and no
 * zip64.
 */
function parts(bytes: Uint8Array): string[] {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const names: string[] = [];

  for (let at = 0; at + 30 <= bytes.length; at += 1) {
    if (view.getUint32(at, true) !== 0x0403_4b50) continue;
    const length = view.getUint16(at + 26, true);
    names.push(new TextDecoder().decode(bytes.subarray(at + 30, at + 30 + length)));
  }
  return names;
}

const ir = (pictures: unknown[]) =>
  JSON.stringify({
    sheets: [
      {
        name: 'Hoja',
        rows: [{ cells: [{ value: { t: 'text', v: 'Concepto' } }] }],
        pictures,
      },
    ],
  });

const AT_ORIGIN = [{ image: 'logo', row: 0, column: 0, width: 120 }];
const IMAGES = [{ name: 'logo', data: LOGO }];

afterAll(async () => {
  await close();
});

describe('a picture', () => {
  it('reaches the package when its bytes are handed over', async () => {
    const { xlsx } = await write(ir(AT_ORIGIN), { images: IMAGES });

    expect(parts(xlsx)).toEqual(
      expect.arrayContaining([
        'xl/media/image1.png',
        'xl/drawings/drawing1.xml',
        'xl/drawings/_rels/drawing1.xml.rels',
        'xl/worksheets/_rels/sheet1.xml.rels',
      ]),
    );
  });

  it('is named in the IR and never carried in it', async () => {
    // The IR is JSON that goes on a queue, into a cache or through an HTTP
    // body. A logo inline would make every one of those carry it.
    expect(ir(AT_ORIGIN)).toContain('"image":"logo"');
    expect(ir(AT_ORIGIN)).not.toContain('base64');
  });

  it('says so when the image it names was never handed over', async () => {
    // Rather than a workbook with a hole where the logo was, which nobody
    // notices until a customer opens it.
    await expect(write(ir(AT_ORIGIN))).rejects.toThrow(/no image of that name/);
  });

  it('leaves a workbook without one exactly as it was', async () => {
    // The whole feature has to cost nothing to everyone not using it.
    const { xlsx } = await write(ir([]));
    const names = parts(xlsx);

    expect(names).toHaveLength(6);
    expect(names.some((name) => name.includes('drawing'))).toBe(false);
  });

  it('reaches a workbook whose rows are streamed', async () => {
    // `Book` takes the same options `write` does, so `images` type-checks on
    // it — and it was accepted and dropped, which surfaced as the engine
    // saying the image had never been handed over when it plainly had. A
    // letterhead on a streamed export is the ordinary case for this feature,
    // not an exotic one: it is the million-row ledger that needs streaming.
    const book = new Book([{ name: 'Hoja', pictures: AT_ORIGIN }], { images: IMAGES });
    await book.rows([{ cells: [{ value: { t: 'text', v: 'uno' } }] }]);
    const { xlsx } = await book.finish();

    expect(xlsx).not.toBeNull();
    expect(parts(xlsx as Uint8Array)).toEqual(
      expect.arrayContaining(['xl/media/image1.png', 'xl/drawings/drawing1.xml']),
    );
  });

  it('does not stay behind on the writer that wrote it', async () => {
    // Workers are reused and the module keeps its images the way the page
    // engine keeps its fonts. A second workbook that inherited the first
    // one's library would carry an image nobody asked for — and, worse, one
    // customer's letterhead into another customer's export.
    await write(ir(AT_ORIGIN), { images: IMAGES });
    const { xlsx } = await write(ir([]));

    expect(parts(xlsx).some((name) => name.startsWith('xl/media/'))).toBe(false);
  });
});
