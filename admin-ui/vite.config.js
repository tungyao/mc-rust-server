import { fileURLToPath, URL } from 'node:url';
import { defineConfig } from 'vite';
import { viteSingleFile } from 'vite-plugin-singlefile';

export default defineConfig({
  resolve: {
    alias: {
      // Templates are authored as strings in JS, so we need the runtime compiler.
      // The alias applies to Naive UI too, guaranteeing a single Vue instance.
      vue: fileURLToPath(new URL('./node_modules/vue/dist/vue.esm-bundler.js', import.meta.url)),
    },
  },
  build: {
    target: 'es2020',
    cssCodeSplit: false,
    assetsInlineLimit: 100000000,
    // vite-plugin-singlefile inlines all JS/CSS into index.html; no chunk files.
    rollupOptions: {
      output: { inlineDynamicImports: true },
    },
  },
  plugins: [viteSingleFile()],
});
