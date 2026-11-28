import { expect, test } from './test';

test('a source starting above zoom 0 gets its thumbnail from inside its zoom range', async ({
  page,
  request,
}) => {
  const tileJson = await (await request.get('/usda_naip_256_lzw_z3')).json();
  const zooms: number[] = [];
  page.on('request', (r) => {
    const match = r.url().match(/\/usda_naip_256_lzw_z3\/(\d+)\/\d+\/\d+$/);
    if (match) zooms.push(Number(match[1]));
  });
  await page.goto('/?tab=tiles');

  const card = page.getByRole('article', { name: 'usda_naip_256_lzw_z3' });
  await card.scrollIntoViewIfNeeded();

  await expect(card.locator('img')).toBeVisible();
  expect(Math.min(...zooms)).toBeGreaterThanOrEqual(tileJson.minzoom);
  expect(Math.max(...zooms)).toBeLessThanOrEqual(tileJson.maxzoom);
});
