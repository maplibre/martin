import path from 'node:path';
import { fileURLToPath } from 'node:url';
import react from '@astrojs/react';
import sitemap from '@astrojs/sitemap';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'astro/config';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// https://astro.build/config
export default defineConfig({
  base: '/',
  integrations: [react(), sitemap()],
  site: 'https://martin.maplibre.org',
  vite: {
    optimizeDeps: {
      include: ['react', 'react-dom'],
    },
    plugins: [tailwindcss()],
    resolve: {
      alias: { '@': path.join(__dirname, 'src') },
      dedupe: ['react', 'react-dom'],
    },
  },
});
