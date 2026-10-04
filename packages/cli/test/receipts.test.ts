import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterEach, describe, expect, it } from 'vitest';
import { buildAll } from '../src/build.js';
import { loadConfig } from '../src/config.js';
import { type Preview, startPreview } from '../src/preview.js';

const projects: string[] = [];
const servers: Preview[] = [];
async function project() {
  const dir = await mkdtemp(fileURLToPath(new URL('.scratch-receipt-', import.meta.url)));
  projects.push(dir);
  await mkdir(join(dir, 'documents/sales'), { recursive: true });
  await writeFile(join(dir, 'imprenta.config.ts'), 'export default { documents: "./documents" };');
  await writeFile(
    join(dir, 'documents/sales/ticket.tsx'),
    `
    import { EscPos, Text, Row, Column, QrCode, Feed, Cut } from '@imprentajs/react/escpos';
    export default function Ticket({ number }) {
      return <EscPos profile={{ columns: 32 }}>
        <Text bold justification="center">Compra {number}</Text>
        <Row><Column>España</Column><Column width={10} justification="right">9,50 €</Column></Row>
        <QrCode>https://example.com/receipt/1</QrCode><Feed lines={3} /><Cut />
      </EscPos>;
    }
    Ticket.PreviewProps = { number: 'T-001' };
  `,
  );
  return dir;
}

afterEach(async () => {
  await Promise.all(servers.splice(0).map((server) => server.close()));
  await Promise.all(projects.splice(0).map((dir) => rm(dir, { recursive: true, force: true })));
});

describe('receipt tooling', () => {
  it('builds a native binary ticket without fonts and preserves folders', async () => {
    const dir = await project();
    const [done] = await buildAll(await loadConfig(dir), { out: join(dir, 'out') });
    expect(done.error).toBeUndefined();
    expect(done.format).toBe('escpos');
    expect(done.parts).toBe(1);
    expect(done.checks).toEqual([]);
    const bytes = await readFile(join(dir, 'out/sales/ticket.escpos'));
    expect(Array.from(bytes.subarray(0, 2))).toEqual([0x1b, 0x40]);
    expect(bytes.includes(Buffer.from('Compra T-001'))).toBe(true);
    expect(bytes.length).toBe(done.bytes);
  });

  it('serves SVG for viewing and untouched ESC/POS for downloading', async () => {
    const dir = await project();
    const server = await startPreview(await loadConfig(dir), 0);
    servers.push(server);
    const url = server.url;
    const response = await fetch(`${url}api/render?id=sales/ticket`);
    expect(response.ok).toBe(true);
    const report = await response.json();
    expect(report.format).toBe('escpos');
    expect(report.profile.printableWidth).toBe(384);
    expect(report.checks).toEqual([]);
    const file = await fetch(`${url}api/file?id=sales/ticket&cached=1&format=pdf`);
    expect(file.headers.get('content-type')).toBe('application/octet-stream');
    expect(file.headers.get('content-disposition')).toContain('ticket.escpos');
    const bytes = new Uint8Array(await file.arrayBuffer());
    expect(bytes.slice(0, 2)).toEqual(new Uint8Array([0x1b, 0x40]));
    const image = await fetch(`${url}api/preview?id=sales/ticket&cached=1`);
    expect(image.headers.get('content-type')).toContain('image/svg+xml');
    expect(image.headers.get('cache-control')).toBe('no-store');
    expect(await image.text()).toContain('<svg');
  });

  it('keeps preview and download on the same render snapshot', async () => {
    const dir = await project();
    const server = await startPreview(await loadConfig(dir), 0);
    servers.push(server);
    await fetch(`${server.url}api/render?id=sales/ticket`);
    const before = Buffer.from(
      await (await fetch(`${server.url}api/file?id=sales/ticket&cached=1`)).arrayBuffer(),
    );
    const path = join(dir, 'documents/sales/ticket.tsx');
    await writeFile(path, (await readFile(path, 'utf8')).replace('T-001', 'T-999'));
    const cached = Buffer.from(
      await (await fetch(`${server.url}api/file?id=sales/ticket&cached=1`)).arrayBuffer(),
    );
    expect(cached).toEqual(before);
    const fresh = Buffer.from(
      await (await fetch(`${server.url}api/file?id=sales/ticket`)).arrayBuffer(),
    );
    expect(fresh.includes(Buffer.from('T-999'))).toBe(true);
  });

  it('carries unsupported characters into the checks panel and build diagnostics', async () => {
    const dir = await project();
    await writeFile(
      join(dir, 'documents/unsupported.tsx'),
      `
      import { EscPos, Text } from '@imprentajs/react/escpos';
      export default () => <EscPos><Text>🐈</Text></EscPos>;
    `,
    );
    const [done] = await buildAll(await loadConfig(dir), {
      out: join(dir, 'out'),
      only: 'unsupported',
    });
    expect(done.error).toBeUndefined();
    expect(done.diagnostics.join(' ')).toContain('unsupported-character');
    expect(done.checks.some((finding) => finding.rule === 'unsupported-character')).toBe(true);
  });
});
