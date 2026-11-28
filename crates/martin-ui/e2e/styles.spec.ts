import { readFile } from 'node:fs/promises';
import { validateStyleMin } from '@maplibre/maplibre-gl-style-spec';
import type { Page } from '@playwright/test';
import { expect, test } from './test';

async function hide(page: Page, layerId: string) {
  const row = page.getByRole('option', { name: new RegExp(`^${layerId}\\b`) });
  await row.hover();
  await row.getByLabel(`Hide ${layerId}`).click();
}

test('the style preview loads every source, sprite and font of the style', async ({ page }) => {
  const requested = new Set<string>();
  page.on('response', (r) => {
    if (r.ok()) requested.add(new URL(r.url()).pathname);
  });

  await page.goto('/?tab=styles');

  await expect(page.getByRole('article', { name: 'local' })).toBeVisible();
  await expect
    .poll(() => [...requested].sort())
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

test('the editor lists every layer of the style', async ({ page, request }) => {
  const style = await (await request.get('/style/local')).json();

  await page.goto('/?tab=styles');
  await page.getByRole('article', { name: 'local' }).getByRole('button', { name: 'Edit' }).click();

  await expect(page).toHaveURL(/[?&]style=local(&|#|$)/);
  const layers = page.getByRole('listbox', { name: 'Layers' }).getByRole('option');
  await expect(layers).toHaveText(style.layers.map((l: { id: string }) => l.id).reverse());
});

test('an edited style downloads as a valid style with the edit applied', async ({ page }) => {
  await page.goto('/?tab=styles&style=local');

  await hide(page, 'labels');
  await expect(page.getByRole('button', { name: '1 edit' })).toBeVisible();
  const download = page.waitForEvent('download');
  await page.getByRole('button', { name: 'Download' }).click();

  const file = await download;
  expect(file.suggestedFilename()).toBe('local.json');
  const style = JSON.parse(await readFile(await file.path(), 'utf8'));
  expect(validateStyleMin(style)).toEqual([]);
  expect(style.layers.find((l: { id: string }) => l.id === 'labels').layout.visibility).toBe(
    'none',
  );
});

test('undo restores the style as martin served it', async ({ page, request }) => {
  const served = await (await request.get('/style/local')).json();
  await page.goto('/?tab=styles&style=local');

  await hide(page, 'areas');
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

  await expect(page).not.toHaveURL(/style=/);
  await expect(page.getByRole('article', { name: 'local' })).toBeVisible();
});
