import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterAll, describe, expect, it } from 'vitest';
import { buildAll } from '../src/build.js';
import { loadConfig } from '../src/config.js';

const fixtures = fileURLToPath(new URL('../../../crates/imprenta-pdf/tests', import.meta.url));
const scratch = fileURLToPath(new URL('.build', import.meta.url));
const made: string[] = [];

const project = (documents: Record<string, string>, fonts = true) => {
  const dir = mkdtempSync(`${scratch}-`);
  made.push(dir);
  mkdirSync(join(dir, 'documents'), { recursive: true });
  writeFileSync(
    join(dir, 'imprenta.config.ts'),
    `export default {
       documents: './documents',
       ${fonts ? `fonts: [{ path: ${JSON.stringify(join(fixtures, 'fonts/Roboto-Regular.ttf'))} }],` : ''}
     };`,
  );
  for (const [name, body] of Object.entries(documents)) {
    mkdirSync(join(dir, 'documents', name, '..'), { recursive: true });
    writeFileSync(join(dir, 'documents', name), body);
  }
  return dir;
};

const hello = (text: string) => `
  import { Document, Text } from '@imprentajs/react/pdf';
  export default function D() { return <Document><Text>${text}</Text></Document>; }
`;

afterAll(() => {
  for (const dir of made) rmSync(dir, { recursive: true, force: true });
});

describe('buildAll', () => {
  it('writes a PDF for every document', async () => {
    const dir = project({ 'factura.tsx': hello('a'), 'ventas/recibo.tsx': hello('b') });

    const done = await buildAll(await loadConfig(dir), { out: join(dir, 'out') });

    expect(done.map((d) => d.id).sort()).toEqual(['factura', 'ventas/recibo']);
    expect(existsSync(join(dir, 'out/factura.pdf'))).toBe(true);
    expect(readFileSync(join(dir, 'out/factura.pdf')).subarray(0, 5).toString()).toBe('%PDF-');
  }, 60_000);

  it('keeps the folders the documents were in', async () => {
    // Flattening would collide two `factura.tsx` in different folders, and
    // silently: one would overwrite the other.
    const dir = project({ 'ventas/factura.tsx': hello('a'), 'compras/factura.tsx': hello('b') });

    await buildAll(await loadConfig(dir), { out: join(dir, 'out') });

    expect(existsSync(join(dir, 'out/ventas/factura.pdf'))).toBe(true);
    expect(existsSync(join(dir, 'out/compras/factura.pdf'))).toBe(true);
  }, 60_000);

  it('reports how many pages each one came to', async () => {
    const dir = project({ 'factura.tsx': hello('a') });

    const [done] = await buildAll(await loadConfig(dir), { out: join(dir, 'out') });

    expect(done.format).toBe('pdf');
    expect(done.parts).toBe(1);
    expect(done.bytes).toBeGreaterThan(1000);
  }, 60_000);

  it('writes a spreadsheet for a document that declares one', async () => {
    // The folder holds both kinds and one command covers it. Which a file is
    // cannot be known from its name — only from what its component returns.
    const dir = project({
      'factura.tsx': hello('a'),
      'ventas.tsx': `
        import { Cell, Row, Sheet, Workbook } from '@imprentajs/react/xlsx';
        export default function Ventas() {
          return (
            <Workbook>
              <Sheet name="Ventas">
                <Row><Cell value={1200} /></Row>
              </Sheet>
            </Workbook>
          );
        }
      `,
    });

    const done = await buildAll(await loadConfig(dir), { out: join(dir, 'out') });

    const sheet = done.find((d) => d.id === 'ventas');
    expect(sheet?.format).toBe('xlsx');
    expect(sheet?.parts).toBe(1);
    expect(existsSync(join(dir, 'out/ventas.xlsx'))).toBe(true);
    expect(existsSync(join(dir, 'out/factura.pdf'))).toBe(true);
  }, 60_000);

  it('builds a spreadsheet in a project with no fonts at all', async () => {
    // A project of nothing but exports has no use for a typeface, and being
    // made to configure one to get past the check would be a nuisance with no
    // reason behind it.
    const dir = project(
      {
        'ventas.tsx': `
          import { Cell, Row, Sheet, Workbook } from '@imprentajs/react/xlsx';
          export default function Ventas() {
            return (
              <Workbook><Sheet name="V"><Row><Cell>a</Cell></Row></Sheet></Workbook>
            );
          }
        `,
      },
      false,
    );

    const [done] = await buildAll(await loadConfig(dir), { out: join(dir, 'out') });

    expect(done.error).toBeUndefined();
    expect(done.format).toBe('xlsx');
  }, 60_000);

  it('builds one document when asked for one', async () => {
    const dir = project({ 'factura.tsx': hello('a'), 'recibo.tsx': hello('b') });

    const done = await buildAll(await loadConfig(dir), { out: join(dir, 'out'), only: 'factura' });

    expect(done.map((d) => d.id)).toEqual(['factura']);
    expect(existsSync(join(dir, 'out/recibo.pdf'))).toBe(false);
  }, 60_000);

  it('carries what the engine noticed, per document', async () => {
    const dir = project({ 'japones.tsx': hello('日本語') });

    const [done] = await buildAll(await loadConfig(dir), { out: join(dir, 'out') });

    expect(done.diagnostics.join(' ')).toContain('missing-glyph');
  }, 60_000);

  it('carries the checks too, so CI can refuse a bad document', async () => {
    // The point of running them here rather than only in the preview: a
    // document nobody can read should be able to fail a build.
    const dir = project({
      'ilegible.tsx': `
        import { Document, Text } from '@imprentajs/react/pdf';
        export default () => <Document><Text size={3}>a</Text></Document>;
      `,
    });

    const [done] = await buildAll(await loadConfig(dir), { out: join(dir, 'out') });

    expect(done.checks.map((c) => c.rule)).toContain('tiny-text');
  }, 60_000);

  it('goes on after one document fails, and says which', async () => {
    // Stopping at the first would hide how much else is broken, and a build
    // of forty documents is worth knowing about in one go.
    const dir = project({ 'bueno.tsx': hello('a'), 'roto.tsx': 'export const nope = 1;' });

    const done = await buildAll(await loadConfig(dir), { out: join(dir, 'out') });

    const roto = done.find((d) => d.id === 'roto');
    expect(roto?.error).toMatch(/default export/);
    expect(done.find((d) => d.id === 'bueno')?.error).toBeUndefined();
    expect(existsSync(join(dir, 'out/bueno.pdf'))).toBe(true);
  }, 60_000);

  it('names the sheet and the image when a picture has no image behind it', async () => {
    // The engine refuses this write outright, and rightly — a workbook with a
    // hole where the logo was is not worth producing. But its message names
    // neither the sheet nor the document, so the rule has to be reached before
    // the write is attempted or it can never fire at all: every workbook that
    // would trip it fails first.
    const dir = project({
      'hoja.tsx': `
        import { Cell, Image, Row, Sheet, Workbook } from '@imprentajs/react/xlsx';
        export default () => (
          <Workbook><Sheet name="Ventas"><Row>
            <Cell><Image src="membrete" width={90} />Concepto</Cell>
          </Row></Sheet></Workbook>
        );
      `,
    });

    const [done] = await buildAll(await loadConfig(dir), { out: join(dir, 'out') });

    expect(done.error).toMatch(/Ventas/);
    expect(done.error).toMatch(/membrete/);
  }, 60_000);

  it('says which setting is missing when there are no fonts', async () => {
    const dir = project({ 'factura.tsx': hello('a') }, false);

    const [done] = await buildAll(await loadConfig(dir), { out: join(dir, 'out') });

    expect(done.error).toMatch(/fonts/);
  }, 60_000);

  it('gives back nothing for a project with no documents', async () => {
    const dir = project({});

    expect(await buildAll(await loadConfig(dir), { out: join(dir, 'out') })).toEqual([]);
  }, 60_000);
});
