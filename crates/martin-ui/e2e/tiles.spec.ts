import { expect, test } from './test';

test('a source starting at zoom 16 gets its thumbnail from a zoom 16 tile', async ({ page }) => {
  const requested: string[] = [];
  page.on('request', (r) => {
    const path = new URL(r.url()).pathname;
    if (path.startsWith('/usda_naip_256_lzw_z3/')) requested.push(path);
  });
  await page.goto('/?tab=tiles');

  const card = page.getByRole('article', { name: 'usda_naip_256_lzw_z3' });
  await card.scrollIntoViewIfNeeded();

  await expect(card.locator('img')).toBeVisible();
  expect(requested).toEqual(['/usda_naip_256_lzw_z3/16/10677/24336']);
});
