#!/usr/bin/env node
import { relative, resolve } from 'node:path';
import { parseArgs } from 'node:util';
import { buildAll } from './build.js';
import { loadConfig } from './config.js';
import { init } from './init.js';
import { startPreview } from './preview.js';

const HELP = `imprenta — preview and build documents

  imprenta init [dir]        put a project here that renders straight away
  imprenta dev [options]     open the preview
  imprenta build [options]   render every document to a file
  imprenta help              this

Options
  --dir <path>    where the documents are (default: from the config, or ./documents)
  --root <path>   where to look for imprenta.config.ts (default: here)
  --port <n>      dev only (default: from the config, or 4321)
  --out <path>    build only (default: ./out)
  --only <id>     build only: one document, by the name the preview shows
  --strict        build only: fail on anything the checks found
`;

async function main(argv: string[]): Promise<number> {
  const [command = 'help', ...rest] = argv;
  if (command === 'help' || command === '--help' || command === '-h') {
    process.stdout.write(HELP);
    return 0;
  }
  if (command === 'init') {
    const [where = '.'] = rest;
    const home = resolve(where);
    const written = await init(home);
    process.stdout.write(
      `\n  ${written.map((file) => `+ ${file}`).join('\n  ')}\n\n` +
        '  Next: install @imprentajs/cli and @imprentajs/react, then `imprenta dev`.\n\n',
    );
    return 0;
  }

  if (command !== 'dev' && command !== 'build') {
    process.stderr.write(`imprenta: there is no "${command}" command\n\n${HELP}`);
    return 1;
  }

  const { values } = parseArgs({
    args: rest,
    options: {
      dir: { type: 'string' },
      port: { type: 'string' },
      root: { type: 'string' },
      out: { type: 'string' },
      only: { type: 'string' },
      strict: { type: 'boolean' },
    },
    allowPositionals: false,
  });

  const loaded = await loadConfig(values.root ?? process.cwd());
  // A flag beats the config, because a flag is what someone typed just now.
  if (values.dir) {
    loaded.documentsDir = new URL(values.dir, `file://${process.cwd()}/`).pathname;
  }
  if (command === 'build') {
    return await run(loaded, {
      out: resolve(values.out ?? './out'),
      only: values.only,
      strict: values.strict ?? false,
    });
  }

  const port = Number(values.port ?? loaded.config.port ?? 4321);

  const preview = await startPreview(loaded, port);
  process.stdout.write(
    `\n  Imprenta — ${preview.url}\n` +
      `  documents  ${loaded.documentsDir}\n` +
      `  config     ${loaded.path ?? 'none, using defaults'}\n\n`,
  );

  const stop = () => {
    void preview.close().then(() => process.exit(0));
  };
  process.on('SIGINT', stop);
  process.on('SIGTERM', stop);
  return -1;
}

/**
 * Builds every document and reports on it.
 *
 * The exit code is what a pipeline reads: a document that did not render
 * always fails, and `--strict` makes anything the checks found fail too. A
 * build that quietly produced an unreadable PDF would be worse than no build.
 */
async function run(
  loaded: Awaited<ReturnType<typeof loadConfig>>,
  options: { out: string; only?: string; strict: boolean },
): Promise<number> {
  const done = await buildAll(loaded, options);
  if (done.length === 0) {
    process.stderr.write(`imprenta: no documents in ${loaded.documentsDir}\n`);
    return 1;
  }

  let failed = 0;
  let flagged = 0;
  for (const document of done) {
    if (document.error) {
      failed += 1;
      process.stdout.write(`  ✗ ${document.id}\n      ${document.error}\n`);
      continue;
    }
    const errors = document.checks.filter((c) => c.status === 'error').length;
    const warnings = document.checks.length - errors;
    flagged += document.checks.length;
    const notes = document.checks.length
      ? `  ${errors} error${errors === 1 ? '' : 's'}, ${warnings} warning${warnings === 1 ? '' : 's'}`
      : '';
    // Pages for a document, sheets for a workbook: the same count means two
    // different things and calling both "p" would read as a lie.
    const size =
      document.format === 'xlsx'
        ? `${document.parts} sheet${document.parts === 1 ? '' : 's'}`
        : `${document.parts}p`;
    process.stdout.write(
      `  ${document.checks.length ? '!' : '✓'} ${relative(process.cwd(), document.path ?? '')}` +
        `  ${size}  ${(document.bytes / 1024).toFixed(1)} KB${notes}\n`,
    );
    for (const finding of document.checks) {
      process.stdout.write(
        `      ${finding.status === 'error' ? '✗' : '!'} ${finding.rule}: ${finding.detail}\n`,
      );
    }
  }

  const built = done.length - failed;
  process.stdout.write(`\n  ${built} of ${done.length} built into ${options.out}\n\n`);

  if (failed > 0) {
    return 1;
  }
  return options.strict && flagged > 0 ? 1 : 0;
}

main(process.argv.slice(2))
  .then((code) => {
    if (code >= 0) {
      process.exit(code);
    }
  })
  .catch((error: unknown) => {
    process.stderr.write(`imprenta: ${error instanceof Error ? error.message : String(error)}\n`);
    process.exit(1);
  });
