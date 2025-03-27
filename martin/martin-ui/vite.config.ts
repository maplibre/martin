import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [react()],
  build: {
    // assets can also be the name of a tile source
    // so we use /_/assets to avoid conflicts
    assetsDir: '_/assets'
  },
});
