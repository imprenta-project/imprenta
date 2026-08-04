import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterAll, describe, expect, it } from 'vitest';
import { init } from '../src/init.js';

const scratch = fileURLToPath(new URL('.init', import.meta.url));
const made: string[] = [];
const dir = () => {
  const d = mkdtempSync(`${scratch}-`);
  made.push(d);
  return d;
};
afterAll(() => {
  for (const d of made) rmSync(d, { recursive: true, force: true });
});

describe('init', () => {
  it('writes a project that runs', async () => {
    const home = dir();

    const written = await init(home);

    expect(written).toContain('imprenta.config.ts');
    expect(existsSync(join(home, 'imprenta.config.ts'))).toBe(true);
    expect(existsSync(join(home, 'documents/factura.tsx'))).toBe(true);
  });

  it('writes a config that needs no font hunting', async () => {
    // A project that cannot render on the first `dev` is a project nobody
    // gets past, and the engine has no system fonts to fall back on.
    const home = dir();

    await init(home);

    const config = readFileSync(join(home, 'imprenta.config.ts'), 'utf8');
    expect(config).toContain("google('Roboto'");
    expect(config).toContain('defineConfig');
  });

  it('writes a document with sample data of its own', async () => {
    // So the very first preview shows an invoice rather than empty scaffolding.
    const home = dir();

    await init(home);

    const document = readFileSync(join(home, 'documents/factura.tsx'), 'utf8');
    expect(document).toContain('PreviewProps');
    expect(document).toContain('export default');
  });

  it('keeps the cache out of version control', async () => {
    const home = dir();

    await init(home);

    expect(readFileSync(join(home, '.gitignore'), 'utf8')).toContain('.imprenta');
  });

  it('refuses to overwrite a config that is already there', async () => {
    // Running it twice in a real project would otherwise throw away whatever
    // the author had configured.
    const home = dir();
    writeFileSync(join(home, 'imprenta.config.ts'), 'export default { documents: "./mine" };');

    await expect(init(home)).rejects.toThrow(/imprenta\.config\.ts/);
    expect(readFileSync(join(home, 'imprenta.config.ts'), 'utf8')).toContain('./mine');
  });

  it('leaves a gitignore that already exists alone but adds to it', async () => {
    const home = dir();
    writeFileSync(join(home, '.gitignore'), 'node_modules\n');

    await init(home);

    const ignored = readFileSync(join(home, '.gitignore'), 'utf8');
    expect(ignored).toContain('node_modules');
    expect(ignored).toContain('.imprenta');
  });

  it('does not write the same ignore twice', async () => {
    const home = dir();
    writeFileSync(join(home, '.gitignore'), '.imprenta\n');

    await init(home);

    const ignored = readFileSync(join(home, '.gitignore'), 'utf8');
    expect(ignored.match(/\.imprenta/g)).toHaveLength(1);
  });

  it('does not overwrite a document of the same name', async () => {
    const home = dir();
    await init(home);
    const mine = 'export default () => null;';
    writeFileSync(join(home, 'documents/factura.tsx'), mine);
    rmSync(join(home, 'imprenta.config.ts'));

    await init(home);

    expect(readFileSync(join(home, 'documents/factura.tsx'), 'utf8')).toBe(mine);
  });
});
