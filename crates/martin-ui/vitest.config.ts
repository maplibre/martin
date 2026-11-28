import path from 'node:path';
import react from '@vitejs/plugin-react';
import { configDefaults, defineConfig } from 'vitest/config';

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  test: {
    coverage: {
      exclude: [
        '**/*.d.ts',
        '**/node_modules/**',
        '**/dist/**',
        '**/*.config.js',
        '**/coverage/**',
        '**/src/components/ui/**',
        '**/src/hooks/use-toast.ts',
        '**/src/lib/types.ts',
        '**/*.config.{js,ts}',
        '**/vitest.*.ts',
      ],
      provider: 'v8',
      reporter: ['text', 'html'],
    },
    environment: 'jsdom',
    exclude: [...configDefaults.exclude, 'e2e/**'],
    globals: false,

    include: ['**/__tests__/components/**/*.[jt]s?(x)', '**/?(*.)+(spec|test).[jt]s?(x)'],
  },
});
