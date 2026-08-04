import { defineConfig, google } from '@imprentajs/cli';

export default defineConfig({
  documents: './documents',
  port: 4321,

  // Downloaded once into `.imprenta/fonts` and cached there, the way
  // `next/font/google` self-hosts what it fetches. Nothing to check in.
  fonts: google('Roboto', { weights: ['regular', 'bold'] }),

  images: {
    logo: './assets/logo.png',
  },
});
