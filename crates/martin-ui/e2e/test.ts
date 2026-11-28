import { test as base, expect, type Page } from '@playwright/test';

/** Hosts the UI may contact besides martin, answered with a blank tile so the tests stay offline. */
const STUBBED_HOSTS = ['tile.openstreetmap.org'];

/** A 1x1 transparent PNG. */
const BLANK_PNG = Buffer.from(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=',
  'base64',
);

/** Performance hints from Chromium's software renderer, which are not UI faults. */
const IGNORED_CONSOLE = /GL Driver Message/;

/** Records everything that went wrong on the page: console errors and warnings, uncaught exceptions, failed or 4xx/5xx requests, and requests to unknown hosts. */
async function watchProblems(page: Page, origin: string): Promise<string[]> {
  const problems: string[] = [];
  await page.context().route(
    (url) => url.origin !== origin && url.protocol.startsWith('http'),
    (route) => {
      const url = new URL(route.request().url());
      if (STUBBED_HOSTS.includes(url.hostname)) {
        return route.fulfill({ body: BLANK_PNG, contentType: 'image/png' });
      }
      problems.push(`request to unknown host: ${url.href}`);
      return route.abort('blockedbyclient');
    },
  );
  page.on('console', (message) => {
    const type = message.type();
    if ((type === 'error' || type === 'warning') && !IGNORED_CONSOLE.test(message.text())) {
      problems.push(`console.${type}: ${message.text()}`);
    }
  });
  page.on('pageerror', (error) => problems.push(`uncaught: ${error.message}`));
  page.on('requestfailed', (request) => {
    const reason = request.failure()?.errorText;
    if (reason !== 'net::ERR_ABORTED' && reason !== 'net::ERR_BLOCKED_BY_CLIENT') {
      problems.push(`request failed: ${request.url()} ${reason}`);
    }
  });
  page.on('response', (response) => {
    if (response.status() >= 400) {
      problems.push(`HTTP ${response.status()}: ${response.url()}`);
    }
  });
  return problems;
}

/** Playwright `test` whose pages fail the test on any problem. */
export const test = base.extend<{ problems: string[] }>({
  problems: [
    async ({ page, baseURL }, use) => {
      const problems = await watchProblems(page, new URL(baseURL ?? '').origin);
      await use(problems);
      await page.waitForLoadState('networkidle');
      expect(problems, 'problems seen on the page').toEqual([]);
    },
    { auto: true },
  ],
});

export { expect };
