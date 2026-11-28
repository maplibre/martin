import { expect, test } from './test';

test('the sprite preview shows every icon of the src1 sheet', async ({ page }) => {
  await page.goto('/?tab=sprites');

  await page
    .getByRole('article', { name: 'src1' })
    .getByRole('button', { name: 'Preview' })
    .click();

  const dialog = page.getByRole('dialog', { name: 'src1' });
  await expect(dialog.getByText('another_bicycle', { exact: true })).toBeVisible();
  await expect(dialog.getByText('bear', { exact: true })).toBeVisible();
  await expect(dialog.getByText('sub/circle', { exact: true })).toBeVisible();
});

test('the PNG JSON download of src1 is a file martin serves', async ({
  page,
  request,
  baseURL,
}) => {
  await page.goto('/?tab=sprites&download=src1');

  const link = page.getByRole('link', { name: 'Download PNG JSON' });

  await expect(link).toHaveAttribute('href', `${baseURL}/sprite/src1.json`);
  const file = await request.get('/sprite/src1.json');
  expect(file.status()).toBe(200);
  expect(file.headers()['content-type']).toBe('application/json');
});

test('the PNG Spritesheet download of src1 is a file martin serves', async ({
  page,
  request,
  baseURL,
}) => {
  await page.goto('/?tab=sprites&download=src1');

  const link = page.getByRole('link', { name: 'Download PNG Spritesheet' });

  await expect(link).toHaveAttribute('href', `${baseURL}/sprite/src1.png`);
  const file = await request.get('/sprite/src1.png');
  expect(file.status()).toBe(200);
  expect(file.headers()['content-type']).toBe('image/png');
});

test('the High DPI PNG Spritesheet download of src1 is a file martin serves', async ({
  page,
  request,
  baseURL,
}) => {
  await page.goto('/?tab=sprites&download=src1');

  const link = page.getByRole('link', { name: 'Download High DPI PNG Spritesheet' });

  await expect(link).toHaveAttribute('href', `${baseURL}/sprite/src1@2x.png`);
  const file = await request.get('/sprite/src1@2x.png');
  expect(file.status()).toBe(200);
  expect(file.headers()['content-type']).toBe('image/png');
});

test('the SDF JSON download of src1 is a file martin serves', async ({
  page,
  request,
  baseURL,
}) => {
  await page.goto('/?tab=sprites&download=src1');

  const link = page.getByRole('link', { name: 'Download SDF JSON' });

  await expect(link).toHaveAttribute('href', `${baseURL}/sdf_sprite/src1.json`);
  const file = await request.get('/sdf_sprite/src1.json');
  expect(file.status()).toBe(200);
  expect(file.headers()['content-type']).toBe('application/json');
});

test('the SDF Spritesheet download of src1 is a file martin serves', async ({
  page,
  request,
  baseURL,
}) => {
  await page.goto('/?tab=sprites&download=src1');

  const link = page.getByRole('link', { name: 'Download SDF Spritesheet' });

  await expect(link).toHaveAttribute('href', `${baseURL}/sdf_sprite/src1.png`);
  const file = await request.get('/sdf_sprite/src1.png');
  expect(file.status()).toBe(200);
  expect(file.headers()['content-type']).toBe('image/png');
});

test('the High DPI SDF Spritesheet download of src1 is a file martin serves', async ({
  page,
  request,
  baseURL,
}) => {
  await page.goto('/?tab=sprites&download=src1');

  const link = page.getByRole('link', { name: 'Download High DPI SDF Spritesheet' });

  await expect(link).toHaveAttribute('href', `${baseURL}/sdf_sprite/src1@2x.png`);
  const file = await request.get('/sdf_sprite/src1@2x.png');
  expect(file.status()).toBe(200);
  expect(file.headers()['content-type']).toBe('image/png');
});
