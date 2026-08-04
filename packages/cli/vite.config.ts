import { fileURLToPath } from 'node:url';
import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

const here = (path: string) => fileURLToPath(new URL(path, import.meta.url));

/**
 * The preview UI, as a build.
 *
 * `app/` used to be served from source by the same Vite server that compiles
 * the author's documents, which made every dependency this UI has — React DOM,
 * Tailwind, the component library — a dependency somebody had to install in
 * order to render an invoice. Tailwind is what settled it: it is a build tool,
 * and shipping a build tool to a project that only wants a PDF is the wrong
 * trade. So the UI is compiled here into `app/dist`, and `imprenta dev` serves
 * those files. Nothing in this config is reached at the author's run time.
 *
 * `pnpm --filter @imprentajs/cli dev:app` runs it as a dev server instead, with
 * hot reload, proxying the API to a real `imprenta dev` on 4321. That is now
 * the way to work on this UI, and it is better than what it replaces, where a
 * change to the UI invalidated the whole document module graph along with it.
 */
export default defineConfig({
  root: here('./app'),
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: { '@': here('./app/src') },
  },
  // Relative, because the built page is served from wherever the package was
  // installed rather than from a site root, and `/assets/…` would only resolve
  // by luck.
  base: './',
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    // No source map. It was 2.3 MB of a 3.3 MB package — seventy per cent of
    // what somebody downloads to render an invoice, to debug a UI that anyone
    // debugging it would run through `dev:app` instead, with the real sources.
    sourcemap: false,
  },
  server: {
    port: 4322,
    proxy: { '/api': { target: 'http://localhost:4321', changeOrigin: false } },
  },
});
