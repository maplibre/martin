import { defineConfig, devices } from '@playwright/test';

const martinPort = Number(process.env.MARTIN_E2E_PORT ?? 3010);
const uiPort = Number(process.env.UI_E2E_PORT ?? 3011);
const martinUrl = `http://127.0.0.1:${martinPort}`;
const uiUrl = `http://127.0.0.1:${uiPort}`;

export default defineConfig({
  forbidOnly: !!process.env.CI,
  fullyParallel: true,
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
  reporter: process.env.CI ? [['github'], ['html', { open: 'never' }]] : 'list',
  retries: process.env.CI ? 1 : 0,
  testDir: 'e2e',
  use: {
    baseURL: uiUrl,
    permissions: ['clipboard-read', 'clipboard-write'],
    trace: 'retain-on-failure',
  },
  webServer: [
    {
      command: process.env.MARTIN_BIN
        ? `just e2e-martin-bin ${process.env.MARTIN_BIN} ${martinPort}`
        : `just e2e-martin ${martinPort}`,
      gracefulShutdown: { signal: 'SIGTERM', timeout: 5000 },
      name: 'martin',
      reuseExistingServer: !process.env.CI,
      stdout: 'ignore',
      timeout: 120_000,
      url: `${martinUrl}/health`,
    },
    {
      command: `npm run build && npx vite preview --host 127.0.0.1 --port ${uiPort} --strictPort`,
      env: { MARTIN_PROXY_TARGET: martinUrl },
      name: 'ui',
      reuseExistingServer: !process.env.CI,
      timeout: 120_000,
      url: uiUrl,
    },
  ],
});
