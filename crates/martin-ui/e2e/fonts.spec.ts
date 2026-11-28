import { expect, test } from './test';

test('the Overpass Mono Regular card draws its glyphs', async ({ page }) => {
  const glyphs = page.waitForResponse(
    (r) => r.url().endsWith('/font/Overpass%20Mono%20Regular/0-255') && r.ok(),
  );

  await page.goto('/?tab=fonts');

  await glyphs;
  const canvas = page.getByRole('article', { name: 'Overpass Mono Regular' }).locator('canvas');
  await expect
    .poll(() =>
      canvas.evaluate((c: HTMLCanvasElement) => {
        const pixels = c.getContext('2d')?.getImageData(0, 0, c.width, c.height).data ?? [];
        return pixels.some((value, i) => i % 4 === 3 && value > 0);
      }),
    )
    .toBe(true);
});

test('the Overpass Mono Light card draws its glyphs', async ({ page }) => {
  const glyphs = page.waitForResponse(
    (r) => r.url().endsWith('/font/Overpass%20Mono%20Light/0-255') && r.ok(),
  );

  await page.goto('/?tab=fonts');

  await glyphs;
  const canvas = page.getByRole('article', { name: 'Overpass Mono Light' }).locator('canvas');
  await expect
    .poll(() =>
      canvas.evaluate((c: HTMLCanvasElement) => {
        const pixels = c.getContext('2d')?.getImageData(0, 0, c.width, c.height).data ?? [];
        return pixels.some((value, i) => i % 4 === 3 && value > 0);
      }),
    )
    .toBe(true);
});

test('copy link on a font card copies a glyph url template martin answers', async ({
  page,
  request,
  baseURL,
}) => {
  await page.goto('/?tab=fonts');

  await page
    .getByRole('article', { name: 'Overpass Mono Light' })
    .getByRole('button', { name: 'Copy link' })
    .click();

  const copied = await page.evaluate(() => navigator.clipboard.readText());
  expect(copied).toBe(`${baseURL}/font/Overpass Mono Light/{range}`);
  const glyphs = await request.get(copied.replace('{range}', '0-255'));
  expect(glyphs.status()).toBe(200);
  expect(glyphs.headers()['content-type']).toBe('application/x-protobuf');
});
