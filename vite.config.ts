import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import path from 'node:path';
import { readFileSync } from 'node:fs';
import { publicUiLabels } from './scripts/public-ui-labels.mjs';

export default defineConfig({
  define: {
    __NEOSERIAL_PUBLIC_LABELS__: JSON.stringify(publicUiLabels(path.resolve('./src'))),
    __NEOSERIAL_BUILD__: JSON.stringify({
      version: JSON.parse(readFileSync(new URL('./package.json', import.meta.url), 'utf8')).version,
      id: process.env.NEOSERIAL_FRONTEND_BUILD_ID || process.env.GITHUB_SHA || new Date().toISOString(),
    }),
  },
  plugins: [svelte()],
  resolve: {
    alias: {
      $lib: path.resolve('./src/lib'),
      $components: path.resolve('./src/components'),
      $assets: path.resolve('./src/assets'),
    },
    extensions: ['.mjs', '.js', '.ts', '.svelte.ts', '.svelte', '.json'],
  },
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    watch: {
      ignored: ['**/src-tauri/**'],
    },
  },
  envPrefix: ['VITE_', 'TAURI_'],
  build: {
    target: ['es2022', 'chrome100', 'safari13'],
    minify: !process.env.TAURI_DEBUG ? 'esbuild' : false,
    // npm run build moves maps out of dist before Tauri embeds the frontend.
    sourcemap: 'hidden',
  },
});
