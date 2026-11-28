import { expect, test } from './test';

test('every font card draws its own glyphs', async ({ page, catalog }) => {
  const names = Object.keys(catalog.fonts ?? {});
  const fetched = new Set<string>();
  page.on('response', (r) => {
    if (r.ok()) fetched.add(decodeURIComponent(new URL(r.url()).pathname));
  });

  await page.goto('/?tab=fonts');

  for (const name of names) {
    const canvas = page.getByRole('article', { name }).locator('canvas');
    await expect
      .poll(() =>
        canvas.evaluate((c: HTMLCanvasElement) => {
          const pixels = c.getContext('2d')?.getImageData(0, 0, c.width, c.height).data ?? [];
          return pixels.some((value, i) => i % 4 === 3 && value > 0);
        }),
      )
      .toBe(true);
    expect(fetched).toContain(`/font/${name}/0-255`);
  }
});

test('copy link copies a glyph url template martin answers', async ({ page, request }) => {
  await page.goto('/?tab=fonts');

  await page
    .getByRole('article', { name: 'Overpass Mono Light' })
    .getByRole('button', { name: 'Copy link' })
    .click();

  const template = await page.evaluate(() => navigator.clipboard.readText());
  expect(template).toMatch(/\{range\}$/);
  const glyphs = await request.get(template.replace('{range}', '0-255'));
  expect(glyphs.status()).toBe(200);
  expect(glyphs.headers()['content-type']).toBe('application/x-protobuf');
});
