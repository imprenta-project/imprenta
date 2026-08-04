import { existsSync } from 'node:fs';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';

/**
 * Puts a project on disk that renders on the first `dev`.
 *
 * Not empty scaffolding: a config with a font already chosen and a document
 * with sample data of its own, so the first thing an author sees is an
 * invoice rather than a blank pane and a list of things still to do. The
 * engine has no system fonts, so a config without one renders nothing at all
 * — which is a poor way to meet a tool.
 *
 * Nothing already there is overwritten. Running this twice in a real project
 * must not throw away what someone configured.
 */
export async function init(home: string): Promise<string[]> {
  const config = join(home, 'imprenta.config.ts');
  if (existsSync(config)) {
    throw new Error(`${config} is already there — delete it first if you meant to start again`);
  }

  const written: string[] = [];
  const put = async (relative: string, body: string) => {
    const path = join(home, relative);
    if (existsSync(path)) {
      return;
    }
    await mkdir(dirname(path), { recursive: true });
    await writeFile(path, body);
    written.push(relative);
  };

  await put('imprenta.config.ts', CONFIG);
  await put('documents/factura.tsx', DOCUMENT);
  await ignore(home, written);
  return written;
}

/** Adds to a `.gitignore` rather than replacing one. */
async function ignore(home: string, written: string[]): Promise<void> {
  const path = join(home, '.gitignore');
  const existing = existsSync(path) ? await readFile(path, 'utf8') : '';
  if (existing.split(/\r?\n/).some((line) => line.trim() === '.imprenta')) {
    return;
  }
  const body = existing && !existing.endsWith('\n') ? `${existing}\n` : existing;
  await writeFile(path, `${body}${existing ? '' : ''}.imprenta\n`);
  written.push('.gitignore');
}

const CONFIG = `import { defineConfig, google } from '@imprentajs/cli';

export default defineConfig({
  documents: './documents',

  // Fetched once into .imprenta/fonts and used from there. The engine has no
  // system fonts — that is what makes a document print the same everywhere —
  // so a project has to say which ones it is set in.
  fonts: google('Roboto', { weights: ['regular', 'bold'] }),

  // Anything a document refers to by name.
  // images: { logo: './assets/logo.png' },
});
`;

const DOCUMENT = `import { B, Document, Footer, PageCount, PageNumber, Table, Text } from '@imprentajs/react/pdf';

interface Line {
  ref: string;
  concept: string;
  price: number;
}

interface Props {
  number: string;
  lines: Line[];
}

const euros = (n: number) => \`\${n.toLocaleString('es-ES', { minimumFractionDigits: 2 })} €\`;

export default function Factura({ number, lines }: Props) {
  const total = lines.reduce((sum, line) => sum + line.price, 0);

  return (
    <Document className="p-10">
      <Footer height={20}>
        <Text className="text-xs text-slate-500">
          {number} · Página <PageNumber /> de <PageCount />
        </Text>
      </Footer>

      <Text className="text-2xl font-bold mb-1">FACTURA</Text>
      <Text className="text-sm text-slate-600 mb-6">{number}</Text>

      <Table
        columns={[{ width: 46 }, { width: 'auto' }, { width: 90, align: 'end' }]}
        header={{
          cells: [
            { text: 'Ref.', color: '#ffffff', weight: 'bold' },
            { text: 'Concepto', color: '#ffffff', weight: 'bold' },
            { text: 'Importe', color: '#ffffff', weight: 'bold' },
          ],
          style: { background: '#1b3a5c' },
        }}
        rows={[
          ...lines.map((line) => ({
            cells: [{ text: line.ref }, { text: line.concept }, { text: euros(line.price) }],
          })),
          {
            cells: [
              { text: '' },
              { text: 'TOTAL', weight: 'bold' as const },
              { text: euros(total), weight: 'bold' as const },
            ],
            style: { background: '#e8eef4' },
          },
        ]}
        padding={5}
      />
    </Document>
  );
}

/**
 * What the preview renders it with.
 *
 * Sample data lives beside the document and ships nowhere: the preview does
 * \`<Factura {...Factura.PreviewProps} />\` and production passes the real thing.
 */
Factura.PreviewProps = {
  number: 'FV-2026-00001',
  lines: [
    { ref: '001', concept: 'Licencia anual, plan profesional', price: 1200 },
    { ref: '002', concept: 'Implantación y puesta en marcha', price: 3400 },
  ],
} satisfies Props;
`;
