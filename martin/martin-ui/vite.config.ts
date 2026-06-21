import path from 'node:path';
import babel from '@rolldown/plugin-babel';
import tailwindcss from '@tailwindcss/vite';
import react, { reactCompilerPreset } from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

// https://vitejs.dev/config/
export default defineConfig({
  base: './', // A relative path supports the `route-prefix` config
  build: {
    // assets can also be the name of a tile source
    // so we use /_/assets to avoid conflicts
    assetsDir: '_/assets',
    outDir: 'dist',
    rollupOptions: {
      output: {
        manualChunks: (id) => {
          if (id.includes('maplibre')) {
            return 'maplibre';
          }
          return undefined;
        },
      },
    },
    sourcemap: true,
  },
  envPrefix: 'VITE_',
  plugins: [
    react(),
    babel({
      presets: [reactCompilerPreset()],
    }),
    tailwindcss(),
  ],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  server: {
    port: 3001,
  },
});
