import { readFile } from 'node:fs/promises';
import { validateStyleMin } from '@maplibre/maplibre-gl-style-spec';
import { expect, test } from './test';

test('the style preview loads every source, sprite and font of the style', async ({ page }) => {
  const requested = new Set<string>();
  page.on('response', (r) => {
    if (r.ok()) requested.add(new URL(r.url()).pathname);
  });

  await page.goto('/?tab=styles');

  await expect(page.getByRole('article', { name: 'local' })).toBeVisible();
  await expect
    .poll(() => [...requested])
    .toEqual(
      expect.arrayContaining([
        '/style/local',
        '/feature_collection_2',
        '/png',
        '/sprite/src1.json',
        '/sprite/src1.png',
        '/font/Overpass%20Mono%20Regular/0-255',
      ]),
    );
});

test('the editor lists every layer of the style, top first', async ({ page }) => {
  await page.goto('/?tab=styles');

  await page.getByRole('article', { name: 'local' }).getByRole('button', { name: 'Edit' }).click();

  await expect.poll(() => new URL(page.url()).searchParams.get('style')).toBe('local');
  await expect(page.getByRole('listbox', { name: 'Layers' }).getByRole('option')).toHaveText([
    'labels',
    'lines',
    'areas',
    'relief',
    'background',
  ]);
});

test('a style with a hidden layer downloads as a valid style with that layer hidden', async ({
  page,
}) => {
  await page.goto('/?tab=styles&style=local');
  const row = page.getByRole('option', { name: /^labels\b/ });
  await row.hover();
  await row.getByLabel('Hide labels').click();
  await expect(page.getByRole('button', { name: '1 edit' })).toBeVisible();
  const download = page.waitForEvent('download');

  await page.getByRole('button', { name: 'Download' }).click();

  const file = await download;
  expect(file.suggestedFilename()).toBe('local.json');
  const style = JSON.parse(await readFile(await file.path(), 'utf8'));
  expect(validateStyleMin(style)).toEqual([]);
  expect(style.layers[4]).toEqual({
    filter: ['==', ['geometry-type'], 'Point'],
    id: 'labels',
    layout: {
      'icon-image': 'bear',
      'text-field': ['get', 'name'],
      'text-font': ['Overpass Mono Regular'],
      'text-offset': [0, 1.5],
      visibility: 'none',
    },
    source: 'features',
    'source-layer': 'feature_collection_2',
    type: 'symbol',
  });
});

test('undo restores the style as martin served it', async ({ page, request }) => {
  const served = await (await request.get('/style/local')).json();
  await page.goto('/?tab=styles&style=local');
  const row = page.getByRole('option', { name: /^areas\b/ });
  await row.hover();
  await row.getByLabel('Hide areas').click();

  await page.getByRole('button', { name: 'Undo (Ctrl+Z)' }).click();

  await expect(page.getByRole('button', { name: '0 edits' })).toBeVisible();
  const download = page.waitForEvent('download');
  await page.getByRole('button', { name: 'Download' }).click();
  const style = JSON.parse(await readFile(await (await download).path(), 'utf8'));
  expect(style).toEqual(served);
});

test('leaving the editor returns to the styles catalog', async ({ page }) => {
  await page.goto('/?tab=styles&style=local');

  await page.getByRole('button', { name: 'Catalog' }).click();

  await expect.poll(() => new URL(page.url()).searchParams.has('style')).toBe(false);
  await expect(page.getByRole('article', { name: 'local' })).toBeVisible();
});
