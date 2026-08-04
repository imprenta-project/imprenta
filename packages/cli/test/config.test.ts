import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterAll, describe, expect, it } from 'vitest';
import { defineConfig, loadConfig } from '../src/config.js';

const scratch = fileURLToPath(new URL('.scratch', import.meta.url));
const packageRoot = fileURLToPath(new URL('..', import.meta.url));
const made: string[] = [];

/**
 * A project as one would really be laid out.
 *
 * With the `node_modules` link, because a config resolves its imports from
 * where it sits: `import { defineConfig } from '@imprentajs/cli'` only works
 * because the package is installed above it, and a fixture without that
 * would pass while the real thing failed.
 */
const project = (files: Record<string, string>) => {
  const dir = mkdtempSync(`${scratch}-`);
  made.push(dir);
  mkdirSync(join(dir, 'node_modules/@imprentajs'), { recursive: true });
  symlinkSync(packageRoot, join(dir, 'node_modules/@imprentajs/cli'), 'dir');
  for (const [name, body] of Object.entries(files)) {
    writeFileSync(join(dir, name), body);
  }
  return dir;
};

afterAll(() => {
  for (const dir of made) {
    rmSync(dir, { recursive: true, force: true });
  }
});

describe('defineConfig', () => {
  it('hands back what it was given, typed', () => {
    // Its whole job. It exists so an editor can check the object in a file
    // that nothing imports a type from.
    const config = defineConfig({ documents: './facturas' });

    expect(config).toEqual({ documents: './facturas' });
  });
});

describe('loadConfig', () => {
  it('reads a TypeScript config beside the project', async () => {
    const dir = project({
      'imprenta.config.ts': `
        import { defineConfig } from '@imprentajs/cli';
        export default defineConfig({ documents: './facturas', port: 4000 });
      `,
    });

    const { config, path } = await loadConfig(dir);

    expect(config.documents).toBe('./facturas');
    expect(config.port).toBe(4000);
    expect(path).toBe(join(dir, 'imprenta.config.ts'));
  });

  it('takes a plain default export as well as defineConfig', async () => {
    const dir = project({ 'imprenta.config.ts': `export default { documents: './docs' };` });

    const { config } = await loadConfig(dir);

    expect(config.documents).toBe('./docs');
  });

  it('resolves the documents directory against the config, not the shell', async () => {
    // Run the CLI from anywhere and it must find the same folder.
    const dir = project({ 'imprenta.config.ts': `export default { documents: './facturas' };` });

    const { documentsDir } = await loadConfig(dir);

    expect(documentsDir).toBe(join(dir, 'facturas'));
  });

  it('falls back to a documents folder when there is no config at all', async () => {
    const dir = project({});

    const { config, path, documentsDir } = await loadConfig(dir);

    expect(path).toBeNull();
    expect(config.documents).toBe('./documents');
    expect(documentsDir).toBe(join(dir, 'documents'));
  });

  it('reads fonts and resolves their paths too', async () => {
    const dir = project({
      'imprenta.config.ts': `export default {
        fonts: [{ path: './fonts/Roboto.ttf' }, { path: './fonts/Roboto-Bold.ttf', weight: 'bold' }],
      };`,
    });

    const { fonts } = await loadConfig(dir);

    expect(fonts).toEqual([
      { path: join(dir, 'fonts/Roboto.ttf'), weight: 'regular', italic: false },
      { path: join(dir, 'fonts/Roboto-Bold.ttf'), weight: 'bold', italic: false },
    ]);
  });

  it('reads images the same way', async () => {
    const dir = project({
      'imprenta.config.ts': `export default { images: { logo: './assets/logo.png' } };`,
    });

    const { images } = await loadConfig(dir);

    expect(images).toEqual([{ name: 'logo', path: join(dir, 'assets/logo.png') }]);
  });

  it('resolves a Google font into a file on disk', async () => {
    // What makes it usable: nobody has to find a `.ttf`, download it and
    // check it into a repository before a document can be set in it.
    const dir = project({
      'imprenta.config.ts': `
        import { google } from '@imprentajs/cli';
        export default { fonts: [...google('Roboto', { weights: ['regular', 'bold'] })] };
      `,
    });

    const { fonts } = await loadConfig(dir);

    expect(fonts).toHaveLength(2);
    expect(fonts.map((f) => f.weight)).toEqual(['regular', 'bold']);
    for (const font of fonts) {
      expect(existsSync(font.path)).toBe(true);
      expect(readFileSync(font.path).subarray(0, 4)).toEqual(Buffer.from([0x00, 0x01, 0x00, 0x00]));
    }
  }, 60_000);

  it('caches a downloaded font beside the project, not in the project', async () => {
    // Somewhere gitignorable and shared between runs, so a build in CI does
    // not go to Google once per document.
    const dir = project({
      'imprenta.config.ts': `
        import { google } from '@imprentajs/cli';
        export default { fonts: google('Roboto') };
      `,
    });

    const { fonts } = await loadConfig(dir);

    expect(fonts[0].path).toContain('.imprenta');
    expect(fonts[0].path.startsWith(dir)).toBe(true);
  }, 60_000);

  it("mixes a Google font with one of the project's own", async () => {
    const dir = project({
      'imprenta.config.ts': `
        import { google } from '@imprentajs/cli';
        export default { fonts: [{ path: './assets/Mine.ttf' }, ...google('Roboto')] };
      `,
    });

    const { fonts } = await loadConfig(dir);

    expect(fonts[0].path).toBe(join(dir, 'assets/Mine.ttf'));
    expect(fonts[1].path).toContain('roboto');
  }, 60_000);

  it('says which line of the config it could not read', async () => {
    // A config is code, and code has mistakes in it. "Cannot read properties
    // of undefined" with no file name is the worst way to learn that.
    const dir = project({ 'imprenta.config.ts': `export default { documents: (` });

    await expect(loadConfig(dir)).rejects.toThrow(/imprenta\.config\.ts/);
  });

  it('refuses a config that exports nothing', async () => {
    const dir = project({ 'imprenta.config.ts': `const unused = 1; export { unused };` });

    await expect(loadConfig(dir)).rejects.toThrow(/default/);
  });

  it('takes a config written in plain JavaScript', async () => {
    const dir = project({ 'imprenta.config.js': `export default { documents: './js' };` });

    const { config } = await loadConfig(dir);

    expect(config.documents).toBe('./js');
  });
});
