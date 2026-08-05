import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';

export default defineConfig({
  root: fileURLToPath(new URL('./demo', import.meta.url)),
  base: './',
  build: {
    target: 'esnext',
    outDir: fileURLToPath(new URL('./demo-dist', import.meta.url)),
    emptyOutDir: true,
  },
  server: {
    host: '127.0.0.1',
    port: 5175,
    fs: {
      allow: [fileURLToPath(new URL('../..', import.meta.url))],
    },
  },
});
