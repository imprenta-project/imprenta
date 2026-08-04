import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vitest/config';

/**
 * Vitest would otherwise read `vite.config.ts`, whose root is `app/` — that
 * config exists to build the preview UI and nothing else, and adopting its
 * root leaves the runner looking for `test/` inside the app. The two configs
 * agree on the one thing they have to: what `@/` means.
 */
export default defineConfig({
  resolve: {
    alias: { '@': fileURLToPath(new URL('./app/src', import.meta.url)) },
  },
  test: {
    include: ['test/**/*.test.{ts,tsx}'],
  },
});
