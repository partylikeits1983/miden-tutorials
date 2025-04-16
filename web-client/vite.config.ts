import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  
  // Force the import to the ESM build
  resolve: {
    alias: {
      '@demox-labs/miden-sdk': '@demox-labs/miden-sdk/dist/esm/index.js',
    },
  },

  assetsInclude: ['**/*.wasm'], // so .wasm is recognized as a binary asset

  build: {
    target: 'esnext',
    // no external here
  },

  optimizeDeps: {
    include: [
      '@demox-labs/miden-sdk',
    ],
    esbuildOptions: {
      target: 'esnext',
      supported: {
        'top-level-await': true,
      },
    },
  },
});
