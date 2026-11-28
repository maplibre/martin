import { expect, test } from './test';

test('the tiles tab lists every tile source martin serves', async ({ page }) => {
  await page.goto('/?tab=tiles');

  await expect(page.getByRole('heading', { name: '11 tile sources' })).toBeVisible();
  await expect(page.getByText('bare_geometry', { exact: true })).toBeVisible();
  await expect(page.getByText('clip', { exact: true })).toBeVisible();
  await expect(page.getByText('feature_1', { exact: true })).toBeVisible();
  await expect(page.getByText('feature_collection_1', { exact: true })).toBeVisible();
  await expect(page.getByText('feature_collection_2', { exact: true })).toBeVisible();
  await expect(page.getByText('feature_collection_3', { exact: true })).toBeVisible();
  await expect(page.getByText('multi_geometries', { exact: true })).toBeVisible();
  await expect(page.getByText('png', { exact: true })).toBeVisible();
  await expect(page.getByText('properties', { exact: true })).toBeVisible();
  await expect(page.getByText('stamen_toner__raster_CC-BY-ODbL_z3', { exact: true })).toBeVisible();
  await expect(page.getByText('usda_naip_256_lzw_z3', { exact: true })).toBeVisible();
});

test('the styles tab lists every style martin serves', async ({ page }) => {
  await page.goto('/?tab=styles');

  await expect(page.getByRole('heading', { name: '1 style' })).toBeVisible();
  await expect(page.getByText('local', { exact: true })).toBeVisible();
});

test('the fonts tab lists every font martin serves', async ({ page }) => {
  await page.goto('/?tab=fonts');

  await expect(page.getByRole('heading', { name: '2 fonts' })).toBeVisible();
  await expect(page.getByText('Overpass Mono Light', { exact: true })).toBeVisible();
  await expect(page.getByText('Overpass Mono Regular', { exact: true })).toBeVisible();
});

test('the sprites tab lists every sprite sheet martin serves', async ({ page }) => {
  await page.goto('/?tab=sprites');

  await expect(page.getByRole('heading', { name: '1 sprite sheet' })).toBeVisible();
  await expect(page.getByText('src1', { exact: true })).toBeVisible();
});
