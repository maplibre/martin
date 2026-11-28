import { expect, test } from './test';

test('inspecting a vector source draws tiles martin serves for it', async ({ page }) => {
  await page.goto('/?tab=tiles');
  const tile = page.waitForResponse(
    (r) => /\/feature_collection_2\/\d+\/\d+\/\d+$/.test(r.url()) && r.ok(),
  );

  await page
    .getByRole('article', { name: 'feature_collection_2' })
    .getByRole('button', { name: 'Inspect' })
    .click();

  await expect(
    page.getByRole('dialog', { name: 'Inspect Tile Source: feature_collection_2' }),
  ).toBeVisible();
  await expect
    .poll(() => new URL(page.url()).searchParams.get('inspect'))
    .toBe('feature_collection_2');
  await tile;
});

test('zoom to data requests deeper tiles of the inspected source', async ({ page }) => {
  await page.goto('/?tab=tiles&inspect=feature_collection_2');
  await page.waitForResponse((r) => r.url().endsWith('/feature_collection_2/0/0/0'));
  const deeper = page.waitForResponse(
    (r) => /\/feature_collection_2\/([1-9]\d*)\/\d+\/\d+$/.test(r.url()) && r.ok(),
  );

  await page.getByRole('button', { name: 'Zoom to data' }).click();

  await deeper;
});

test('the tilejson url in the inspect dialog is the one martin serves', async ({
  page,
  request,
  baseURL,
}) => {
  await page.goto('/?tab=tiles&inspect=usda_naip_256_lzw_z3');

  await page
    .getByRole('group', { name: 'TileJSON' })
    .getByRole('button', { name: 'Copy link' })
    .click();

  const copied = await page.evaluate(() => navigator.clipboard.readText());
  expect(copied).toBe(`${baseURL}/usda_naip_256_lzw_z3`);
  expect((await request.get(copied)).status()).toBe(200);
});

test('the xyz url in the inspect dialog is the template martin advertises', async ({
  page,
  request,
  baseURL,
}) => {
  await page.goto('/?tab=tiles&inspect=usda_naip_256_lzw_z3');

  await page
    .getByRole('group', { name: 'XYZ Tiles' })
    .getByRole('button', { name: 'Copy link' })
    .click();

  const copied = await page.evaluate(() => navigator.clipboard.readText());
  expect(copied).toBe(`${baseURL}/usda_naip_256_lzw_z3/{z}/{x}/{y}`);
  const tileJson = await (await request.get('/usda_naip_256_lzw_z3')).json();
  expect(tileJson.tiles).toEqual([copied]);
  const tile = await request.get(copied.replace('{z}/{x}/{y}', '16/10677/24336'));
  expect(tile.status()).toBe(200);
  expect(tile.headers()['content-type']).toBe('image/png');
});

test('closing the inspect dialog drops it from the url', async ({ page }) => {
  await page.goto('/?tab=tiles&inspect=png');
  await expect(page.getByRole('dialog', { name: 'Inspect Tile Source: png' })).toBeVisible();

  await page.keyboard.press('Escape');

  await expect(page.getByRole('dialog')).toBeHidden();
  await expect.poll(() => new URL(page.url()).searchParams.has('inspect')).toBe(false);
});

test('copy link on a tile card copies the tilejson url of the source', async ({
  page,
  request,
  baseURL,
}) => {
  await page.goto('/?tab=tiles');

  await page
    .getByRole('article', { name: 'png' })
    .getByRole('button', { name: 'Copy link' })
    .click();

  const copied = await page.evaluate(() => navigator.clipboard.readText());
  expect(copied).toBe(`${baseURL}/png`);
  expect((await (await request.get(copied)).json()).tiles).toEqual([`${baseURL}/png/{z}/{x}/{y}`]);
});

test('searching narrows the tile sources', async ({ page }) => {
  await page.goto('/?tab=tiles');

  await page.getByRole('searchbox', { name: 'Search tile sources' }).fill('collection');

  await expect(page.getByRole('heading', { name: '3 of 11 tile sources' })).toBeVisible();
  await expect(page.getByRole('article')).toHaveCount(3);
});

test('a search survives a reload', async ({ page }) => {
  await page.goto('/?tab=tiles&search=collection');

  await expect(page.getByRole('heading', { name: '3 of 11 tile sources' })).toBeVisible();
  await expect(page.getByRole('searchbox', { name: 'Search tile sources' })).toHaveValue(
    'collection',
  );
});

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
