import { test as base, expect, type Page, type Request } from '@playwright/test';

/** Hosts the UI may contact besides martin, answered with a blank tile so the tests stay offline. */
const STUBBED_HOSTS = ['tile.openstreetmap.org'];

/** A 1x1 transparent PNG. */
const BLANK_PNG = Buffer.from(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=',
  'base64',
);

/** Performance hints from Chromium's software renderer, which are not UI faults. */
const IGNORED_CONSOLE = /GL Driver Message/;

/** What the guard saw on a page: its problems, and whether its network went quiet. */
interface Watch {
  /** Console errors and warnings, uncaught exceptions, failed or 4xx/5xx requests, and requests to unknown hosts. */
  problems: string[];
  /** Whether no request has been in flight for half a second. */
  quiet: () => boolean;
}

/** Starts recording the problems of a page and tracking its requests. */
async function watch(page: Page, origin: string): Promise<Watch> {
  const problems: string[] = [];
  const inFlight = new Set<Request>();
  let lastActivity = Date.now();
  const settle = (request: Request) => {
    inFlight.delete(request);
    lastActivity = Date.now();
  };
  page.on('request', (request) => {
    inFlight.add(request);
    lastActivity = Date.now();
  });
  page.on('requestfinished', settle);
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
    settle(request);
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
  return { problems, quiet: () => inFlight.size === 0 && Date.now() - lastActivity >= 500 };
}

/** Playwright `test` whose pages fail the test on any problem. */
export const test = base.extend<{ problems: string[] }>({
  problems: [
    async ({ page, baseURL }, use) => {
      const { problems, quiet } = await watch(page, new URL(baseURL ?? '').origin);
      await use(problems);
      await expect.poll(quiet, { message: 'requests still in flight', timeout: 20_000 }).toBe(true);
      expect(problems, 'problems seen on the page').toEqual([]);
    },
    { auto: true },
  ],
});

export { expect };
