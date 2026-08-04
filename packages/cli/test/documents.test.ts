import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterAll, describe, expect, it } from 'vitest';
import { findDocuments, previewProps } from '../src/documents.js';

const scratch = fileURLToPath(new URL('.docs', import.meta.url));
const made: string[] = [];

const tree = (files: Record<string, string>) => {
  const dir = mkdtempSync(`${scratch}-`);
  made.push(dir);
  for (const [name, body] of Object.entries(files)) {
    mkdirSync(join(dir, name, '..'), { recursive: true });
    writeFileSync(join(dir, name), body);
  }
  return dir;
};

afterAll(() => {
  for (const dir of made) rmSync(dir, { recursive: true, force: true });
});

describe('findDocuments', () => {
  it('finds the documents in a folder', async () => {
    const dir = tree({ 'factura.tsx': '', 'recibo.tsx': '' });

    const found = await findDocuments(dir);

    expect(found.map((d) => d.id)).toEqual(['factura', 'recibo']);
  });

  it('looks inside folders, and says where it found things', async () => {
    // A project with fifty documents groups them, and the grouping is worth
    // keeping: it is how the author already thinks about them.
    const dir = tree({ 'ventas/factura.tsx': '', 'compras/recibo.tsx': '' });

    const found = await findDocuments(dir);

    expect(found.map((d) => d.id)).toEqual(['compras/recibo', 'ventas/factura']);
    expect(found.map((d) => d.group)).toEqual(['compras', 'ventas']);
  });

  it('sorts them so the list does not jump about between reloads', async () => {
    const dir = tree({ 'z.tsx': '', 'a.tsx': '', 'm/b.tsx': '' });

    const found = await findDocuments(dir);

    expect(found.map((d) => d.id)).toEqual(['a', 'm/b', 'z']);
  });

  it('ignores what is not a document', async () => {
    // Helpers, styles and tests live beside documents and are not documents.
    const dir = tree({
      'factura.tsx': '',
      'helpers.ts': '',
      'factura.test.tsx': '',
      'README.md': '',
      '_partial.tsx': '',
    });

    const found = await findDocuments(dir);

    expect(found.map((d) => d.id)).toEqual(['factura']);
  });

  it('gives back nothing for a folder that is not there', async () => {
    // `imprenta dev` in a fresh project should say so, not crash.
    expect(await findDocuments(join(scratch, 'missing'))).toEqual([]);
  });

  it('carries the file path, so something can load it', async () => {
    const dir = tree({ 'factura.tsx': '' });

    const [found] = await findDocuments(dir);

    expect(found.path).toBe(join(dir, 'factura.tsx'));
  });
});

describe('previewProps', () => {
  it('takes the props a component declares for its preview', () => {
    // Sample data lives next to the document rather than inside the tool, and
    // ships nowhere.
    const Factura = () => null;
    Factura.PreviewProps = { number: 'FV-1', total: 7400 };

    expect(previewProps(Factura)).toEqual({ number: 'FV-1', total: 7400 });
  });

  it('gives an empty object when there are none', () => {
    const Factura = () => null;

    expect(previewProps(Factura)).toEqual({});
  });

  it('ignores anything that is not an object', () => {
    // A typo — `PreviewProps = 'invoice'` — should not become the props.
    const Factura = () => null;
    (Factura as { PreviewProps?: unknown }).PreviewProps = 'invoice';

    expect(previewProps(Factura)).toEqual({});
  });

  it('does not mind being handed something that is not a component', () => {
    expect(previewProps(undefined)).toEqual({});
    expect(previewProps(null)).toEqual({});
  });
});
