import { defineConfig } from 'vite';

// The launcher (RELEASES.md §5), built on its own into dist-launcher/ and put at the site's root
// by the Pages workflow. It is served at https://dropkickarcade.com/dwell/ (ADR 0005).
export default defineConfig({
  root: 'launcher',
  base: '/dwell/',
  build: {
    outDir: '../dist-launcher',
    emptyOutDir: true,
    target: 'es2022',
  },
});
