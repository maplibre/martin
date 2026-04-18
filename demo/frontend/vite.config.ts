import babel from '@rolldown/plugin-babel';
import react, { reactCompilerPreset } from '@vitejs/plugin-react';
import { defineConfig } from 'vite';
import mkcert from 'vite-plugin-mkcert';
import viteTsConfigPaths from 'vite-tsconfig-paths';

export default defineConfig({
  build: {
    target: 'esnext',
  },
  plugins: [
    react(),
    babel({
      presets: [reactCompilerPreset()],
    }),
    viteTsConfigPaths({
      root: './',
    }),
    mkcert(),
  ],
  server: { host: true, port: 8080 },
});
