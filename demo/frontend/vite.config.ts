import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import mkcert from 'vite-plugin-mkcert'
import viteTsConfigPaths from 'vite-tsconfig-paths'

export default defineConfig({
  plugins: [
    react(),
    viteTsConfigPaths({
      root: './',
    }),
    mkcert(),
  ],
  build: {
    target: 'esnext',
  },
  server: { https: false, host: true, port: 8080 },
})
