import path from 'node:path';
import babel from '@rolldown/plugin-babel';
import tailwindcss from '@tailwindcss/vite';
import react, { reactCompilerPreset } from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

/** Martin server that `vite preview` forwards non-UI requests to, as martin does when it embeds the UI. */
const martinProxyTarget = process.env.MARTIN_PROXY_TARGET;

/** Every path except the page itself and `/_/assets/`, which the UI bundle owns. */
const MARTIN_PATHS = '^/(?!(?:index\\.html)?(?:[?#]|$)|_/assets/)';

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
  },
  envPrefix: 'VITE_',
  plugins: [
    react(),
    babel({
      presets: [reactCompilerPreset()],
    }),
    tailwindcss(),
  ],
  preview: martinProxyTarget
    ? { proxy: { [MARTIN_PATHS]: { changeOrigin: false, target: martinProxyTarget } } }
    : undefined,
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  server: {
    port: 3001,
  },
});
