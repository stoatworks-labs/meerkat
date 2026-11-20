import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';

import { readFileSync } from 'node:fs';

const pkg = JSON.parse(readFileSync(new URL('./package.json', import.meta.url), 'utf8'));

// Desktop app, not a website: the output is loaded from the Tauri bundle over
// the custom protocol, so `base` is relative and there is no dev-server proxy.
// `clearScreen: false` keeps cargo's errors visible under `tauri dev`.
export default defineConfig({
  define: { __APP_VERSION__: JSON.stringify(`v${pkg.version}`) },
  plugins: [react()],
  base: './',
  clearScreen: false,
  server: {
    port: 5175,
    strictPort: true,
  },
  build: {
    outDir: 'dist',
    sourcemap: true,
    target: 'es2023',
  },
  test: { environment: 'node' },
});
