import { createHash, randomUUID } from 'node:crypto';
import { existsSync } from 'node:fs';
import { mkdir, rename, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';

/** One face of one family: what a document actually asks for. */
export interface GoogleFace {
  family: string;
  weight: 'regular' | 'bold';
  italic: boolean;
}

export interface GoogleOptions {
  /** Defaults to the regular weight alone. */
  weights?: ('regular' | 'bold')[];
  /** Fetch the italic of every weight as well. */
  italics?: boolean;
}

/**
 * The faces of a Google font, for a config to ask for.
 *
 * Named after what `next/font/google` does, and for the same reason: nobody
 * should have to find a `.ttf`, download it and check it into a repository
 * before a document can be set in it. Unlike Next this does not rewrite any
 * CSS — there is none — it just puts the file where the engine can read it.
 */
export function google(family: string, options: GoogleOptions = {}): GoogleFace[] {
  const weights = options.weights ?? ['regular'];
  return weights.flatMap((weight) =>
    options.italics
      ? [
          { family, weight, italic: false },
          { family, weight, italic: true },
        ]
      : [{ family, weight, italic: false }],
  );
}

/**
 * The agent Google answers with TrueType.
 *
 * It answers a modern browser with woff2, which the engine cannot parse, and
 * something as old as MSIE with EOT, which it cannot parse either. Only a
 * narrow band in between — a browser old enough to lack woff, new enough to
 * take a raw font — is served the `.ttf` this needs. Checked against the real
 * service rather than assumed.
 */
const TRUETYPE_AGENT =
  'Mozilla/5.0 (Linux; U; Android 2.2; en-us) AppleWebKit/533.1 (KHTML, like Gecko) Version/4.0 Mobile Safari/533.1';

const CSS_WEIGHT = { regular: 400, bold: 700 } as const;

export interface ParsedFace {
  weight: number;
  italic: boolean;
  url: string;
}

/** The faces a Google stylesheet declares. */
export function parseFaces(css: string): ParsedFace[] {
  const faces: ParsedFace[] = [];
  for (const block of css.split('@font-face')) {
    const url = /src:\s*url\(([^)]+)\)/.exec(block)?.[1];
    const weight = /font-weight:\s*(\d+)/.exec(block)?.[1];
    if (!url || !weight) {
      continue;
    }
    faces.push({
      weight: Number(weight),
      italic: /font-style:\s*italic/.test(block),
      url,
    });
  }
  return faces;
}

export type Fetcher = (
  url: string,
  options: { userAgent: string },
) => Promise<{ ok: boolean; text: string; bytes: Buffer }>;

const over: Fetcher = async (url, options) => {
  const response = await fetch(url, { headers: { 'user-agent': options.userAgent } });
  if (!response.ok) {
    return { ok: false, text: response.statusText, bytes: Buffer.alloc(0) };
  }
  const bytes = Buffer.from(await response.arrayBuffer());
  return { ok: true, text: bytes.toString('utf8'), bytes };
};

/**
 * Downloads one face, once, and gives back where it landed.
 *
 * Cached on disk because a render must never wait on Google — nor fail
 * because a build machine has no way out to the internet. What is cached is
 * keyed by the exact face and the URL Google gave for it, so a new version of
 * a family is fetched rather than quietly reused.
 */
export async function cacheGoogleFont(
  face: GoogleFace,
  home: string,
  fetcher: Fetcher = over,
): Promise<string> {
  const path = join(home, filename(face));
  if (existsSync(path)) {
    return path;
  }

  // A document that asks for the same face twice — two components, the same
  // heading font — must not fetch it twice, and two fetches racing to the
  // same file is how one of them finds it gone.
  const already = inFlight.get(path);
  if (already) {
    return already;
  }
  const fetching = download(face, path, home, fetcher).finally(() => inFlight.delete(path));
  inFlight.set(path, fetching);
  return fetching;
}

/** Downloads in progress, by where they will land. */
const inFlight = new Map<string, Promise<string>>();

async function download(
  face: GoogleFace,
  path: string,
  home: string,
  fetcher: Fetcher,
): Promise<string> {
  const wanted = CSS_WEIGHT[face.weight];
  const query = new URLSearchParams({
    family: `${face.family}:ital,wght@${face.italic ? 1 : 0},${wanted}`,
  });
  const sheet = await fetcher(`https://fonts.googleapis.com/css2?${query}`, {
    userAgent: TRUETYPE_AGENT,
  });
  if (!sheet.ok) {
    throw new Error(
      `Google Fonts has no family called ${JSON.stringify(face.family)} (${sheet.text})`,
    );
  }

  const named = `${face.weight}${face.italic ? ' italic' : ''}`;
  const found = parseFaces(sheet.text).find(
    (candidate) => candidate.weight === wanted && candidate.italic === face.italic,
  );
  if (!found) {
    throw new Error(
      `${face.family} has no ${named}: Google Fonts serves only the weights a family was drawn in`,
    );
  }

  const file = await fetcher(found.url, { userAgent: TRUETYPE_AGENT });
  if (!file.ok) {
    throw new Error(`${face.family} ${named} could not be downloaded: ${file.text}`);
  }
  reject_unreadable(file.bytes, face.family, named);

  await mkdir(home, { recursive: true });
  // Written aside and moved into place, so a download cut off half way
  // through does not leave a truncated font to be trusted for ever after.
  // The name is unique per attempt as well as per process: two builds
  // sharing a cache would otherwise rename the same file out from under
  // each other.
  const partial = `${path}.${process.pid}.${randomUUID().slice(0, 8)}.part`;
  try {
    await writeFile(partial, file.bytes);
    await rename(partial, path);
  } catch (error) {
    await rm(partial, { force: true });
    throw error;
  }
  return path;
}

/**
 * Refuses anything the engine cannot read, here rather than later.
 *
 * A wrong user agent is answered with woff2 or EOT, and both would surface
 * as an unreadable-font error somewhere far from the cause.
 */
function reject_unreadable(bytes: Buffer, family: string, named: string): void {
  const magic = bytes.subarray(0, 4);
  if (magic.equals(Buffer.from([0x00, 0x01, 0x00, 0x00])) || magic.toString() === 'OTTO') {
    return;
  }
  const what =
    magic.toString() === 'wOF2'
      ? 'woff2'
      : magic.toString() === 'wOFF'
        ? 'woff'
        : bytes.length > 4 && bytes.subarray(34, 38).toString() === 'LP'
          ? 'EOT'
          : 'something that is not a font';
  throw new Error(
    `Google Fonts answered with ${what} for ${family} ${named}, which the engine cannot read`,
  );
}

/** Stable, and different for every face. */
function filename(face: GoogleFace): string {
  const slug = face.family.toLowerCase().replace(/[^a-z0-9]+/g, '-');
  const key = createHash('sha256')
    .update(`${face.family}|${face.weight}|${face.italic}`)
    .digest('hex')
    .slice(0, 8);
  return `${slug}-${face.weight}${face.italic ? '-italic' : ''}-${key}.ttf`;
}
