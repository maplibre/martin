import path from 'node:path';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';
import tailwindcss from "@tailwindcss/vite";

// https://vitejs.dev/config/
export default defineConfig({
  build: {
    // assets can also be the name of a tile source
    // so we use /_/assets to avoid conflicts
    assetsDir: '_/assets',
    outDir: 'dist',
    rollupOptions: {
      output: {
        manualChunks: {
          maplibre: ['maplibre-gl', '@vis.gl/react-maplibre', '@maplibre/maplibre-gl-inspect'],
        },
      },
    },
    sourcemap: true,
  },
  envPrefix: 'VITE_',
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  server: {
    port: 3001,
  },
});
