import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { afterAll, describe, expect, it, vi } from 'vitest';
import { cacheGoogleFont, type Fetcher, google, parseFaces } from '../src/index.js';

const scratch = fileURLToPath(new URL('.fonts', import.meta.url));
const made: string[] = [];
const dir = () => {
  const made_ = mkdtempSync(`${scratch}-`);
  made.push(made_);
  return made_;
};

afterAll(() => {
  for (const d of made) rmSync(d, { recursive: true, force: true });
});

const CSS = `
@font-face {
  font-family: 'Roboto';
  font-style: normal;
  font-weight: 400;
  src: url(https://fonts.gstatic.com/s/roboto/v51/regular.ttf) format('truetype');
}
@font-face {
  font-family: 'Roboto';
  font-style: italic;
  font-weight: 400;
  src: url(https://fonts.gstatic.com/s/roboto/v51/italic.ttf) format('truetype');
}
@font-face {
  font-family: 'Roboto';
  font-style: normal;
  font-weight: 700;
  src: url(https://fonts.gstatic.com/s/roboto/v51/bold.ttf) format('truetype');
}
`;

/** The four bytes that make a file a TrueType font. */
const TTF = Buffer.from([0x00, 0x01, 0x00, 0x00]);

/** A stand-in for the network, so the tests neither need it nor wait for it. */
const fake = (css = CSS): Fetcher =>
  vi.fn(async (url: string) => {
    if (url.startsWith('https://fonts.googleapis.com')) {
      return { ok: true, text: css, bytes: Buffer.alloc(0) };
    }
    // Real magic bytes, then the URL, so a test can tell which face it got
    // and the format check has something honest to look at.
    return { ok: true, text: '', bytes: Buffer.concat([TTF, Buffer.from(url)]) };
  });

describe('google', () => {
  it('asks for the regular face when nothing else is said', () => {
    expect(google('Roboto')).toEqual([{ family: 'Roboto', weight: 'regular', italic: false }]);
  });

  it('asks for every weight it was given', () => {
    expect(google('Roboto', { weights: ['regular', 'bold'] })).toEqual([
      { family: 'Roboto', weight: 'regular', italic: false },
      { family: 'Roboto', weight: 'bold', italic: false },
    ]);
  });

  it('adds the italics of each weight when asked', () => {
    // The engine has four faces at most, and this is how a document gets all
    // of them without naming each one.
    expect(google('Roboto', { weights: ['regular', 'bold'], italics: true })).toEqual([
      { family: 'Roboto', weight: 'regular', italic: false },
      { family: 'Roboto', weight: 'regular', italic: true },
      { family: 'Roboto', weight: 'bold', italic: false },
      { family: 'Roboto', weight: 'bold', italic: true },
    ]);
  });
});

describe('parseFaces', () => {
  it('reads the style, the weight and the file out of the stylesheet', () => {
    expect(parseFaces(CSS)).toEqual([
      { weight: 400, italic: false, url: 'https://fonts.gstatic.com/s/roboto/v51/regular.ttf' },
      { weight: 400, italic: true, url: 'https://fonts.gstatic.com/s/roboto/v51/italic.ttf' },
      { weight: 700, italic: false, url: 'https://fonts.gstatic.com/s/roboto/v51/bold.ttf' },
    ]);
  });

  it('gives back nothing for a stylesheet with no faces in it', () => {
    expect(parseFaces('/* nothing */')).toEqual([]);
  });
});

describe('cacheGoogleFont', () => {
  it('downloads a face and puts it where it can be found again', async () => {
    const home = dir();
    const fetcher = fake();

    const path = await cacheGoogleFont(
      { family: 'Roboto', weight: 'regular', italic: false },
      home,
      fetcher,
    );

    expect(existsSync(path)).toBe(true);
    expect(readFileSync(path).toString()).toContain('regular.ttf');
  });

  it('asks Google for a format the engine can actually read', async () => {
    // The default answer is woff2, which the engine cannot parse at all, and
    // an old enough agent gets EOT. Only one narrow band gets TrueType.
    const home = dir();
    const fetcher = fake();

    await cacheGoogleFont({ family: 'Roboto', weight: 'bold', italic: false }, home, fetcher);

    const [, options] = (fetcher as ReturnType<typeof vi.fn>).mock.calls[0];
    expect(options.userAgent).toMatch(/Android 2\.2/);
  });

  it('picks the face that was asked for, not the first one', async () => {
    const home = dir();

    const italic = await cacheGoogleFont(
      { family: 'Roboto', weight: 'regular', italic: true },
      home,
      fake(),
    );
    const bold = await cacheGoogleFont(
      { family: 'Roboto', weight: 'bold', italic: false },
      home,
      fake(),
    );

    expect(readFileSync(italic).toString()).toContain('italic.ttf');
    expect(readFileSync(bold).toString()).toContain('bold.ttf');
  });

  it('does not go to the network for something it already has', async () => {
    // A render must never wait on Google, and a build in CI with no network
    // should work off whatever was cached.
    const home = dir();
    const first = fake();
    const path = await cacheGoogleFont(
      { family: 'Roboto', weight: 'regular', italic: false },
      home,
      first,
    );

    const second = fake();
    const again = await cacheGoogleFont(
      { family: 'Roboto', weight: 'regular', italic: false },
      home,
      second,
    );

    expect(again).toBe(path);
    expect(second).not.toHaveBeenCalled();
  });

  it('keeps the faces apart on disk', async () => {
    const home = dir();
    const one = await cacheGoogleFont(
      { family: 'Roboto', weight: 'regular', italic: false },
      home,
      fake(),
    );
    const two = await cacheGoogleFont(
      { family: 'Roboto', weight: 'bold', italic: false },
      home,
      fake(),
    );

    expect(one).not.toBe(two);
  });

  it('says so when the family does not exist', async () => {
    const home = dir();
    const missing: Fetcher = async () => ({ ok: false, text: 'Not Found', bytes: Buffer.alloc(0) });

    const asked = cacheGoogleFont(
      { family: 'Nonesuch', weight: 'regular', italic: false },
      home,
      missing,
    );

    await expect(asked).rejects.toThrow(/Nonesuch/);
  });

  it('says so when the family has no such weight', async () => {
    // Not every family ships a bold, and silently falling back to regular
    // would print a heading nobody notices is wrong.
    const home = dir();

    const asked = cacheGoogleFont({ family: 'Roboto', weight: 'bold', italic: true }, home, fake());

    await expect(asked).rejects.toThrow(/bold italic/);
  });

  it('refuses bytes that are not a font the engine can read', async () => {
    // Google answers a wrong user agent with woff2 or EOT, and the failure
    // would otherwise surface as a shaping error pages later.
    const home = dir();
    const woff2: Fetcher = async (url) => ({
      ok: true,
      text: CSS,
      bytes: url.includes('googleapis') ? Buffer.alloc(0) : Buffer.from('wOF2rest'),
    });

    const asked = cacheGoogleFont(
      { family: 'Roboto', weight: 'regular', italic: false },
      home,
      woff2,
    );

    await expect(asked).rejects.toThrow(/woff2/i);
  });

  it('does not leave half a file behind when a download fails', async () => {
    // A truncated font in the cache would be used for ever after.
    const home = dir();
    const dies: Fetcher = async (url) => {
      if (url.includes('googleapis')) return { ok: true, text: CSS, bytes: Buffer.alloc(0) };
      throw new Error('the network went away');
    };

    await expect(
      cacheGoogleFont({ family: 'Roboto', weight: 'regular', italic: false }, home, dies),
    ).rejects.toThrow();

    const left = await cacheGoogleFont(
      { family: 'Roboto', weight: 'regular', italic: false },
      home,
      fake(),
    );
    expect(readFileSync(left).toString()).toContain('regular.ttf');
  });
});
