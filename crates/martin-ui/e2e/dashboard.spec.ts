import { expect, test } from './test';

test('the chosen theme survives a reload', async ({ page }) => {
  await page.goto('/');

  await page.getByRole('button', { name: 'Switch to light theme' }).click();
  await page.getByRole('button', { name: 'Switch to dark theme' }).click();

  await expect(page.locator('html')).toHaveClass(/\bdark\b/);
  await page.reload();
  await expect(page.locator('html')).toHaveClass(/\bdark\b/);
  await expect(page.getByRole('button', { name: 'Switch to system theme' })).toBeVisible();
});

test('server activity counts the tiles martin has served', async ({ page, request }) => {
  expect((await request.get('/png/0/0/0')).status()).toBe(200);

  await page.goto('/');

  await expect(page.getByText(/^[\d,]+ requests$/).first()).toBeVisible();
});

test('clicking the Styles tab selects it and puts tab=styles in the url', async ({ page }) => {
  await page.goto('/');

  await page.getByRole('tab', { name: 'Styles' }).click();

  await expect(page.getByRole('tab', { name: 'Styles' })).toHaveAttribute('aria-selected', 'true');
  await expect.poll(() => new URL(page.url()).searchParams.get('tab')).toBe('styles');
});

test('clicking the Fonts tab selects it and puts tab=fonts in the url', async ({ page }) => {
  await page.goto('/');

  await page.getByRole('tab', { name: 'Fonts' }).click();

  await expect(page.getByRole('tab', { name: 'Fonts' })).toHaveAttribute('aria-selected', 'true');
  await expect.poll(() => new URL(page.url()).searchParams.get('tab')).toBe('fonts');
});

test('clicking the Sprites tab selects it and puts tab=sprites in the url', async ({ page }) => {
  await page.goto('/');

  await page.getByRole('tab', { name: 'Sprites' }).click();

  await expect(page.getByRole('tab', { name: 'Sprites' })).toHaveAttribute('aria-selected', 'true');
  await expect.poll(() => new URL(page.url()).searchParams.get('tab')).toBe('sprites');
});

test('clicking the Tiles tab selects it and puts tab=tiles in the url', async ({ page }) => {
  await page.goto('/?tab=styles');

  await page.getByRole('tab', { name: 'Tiles' }).click();

  await expect(page.getByRole('tab', { name: 'Tiles' })).toHaveAttribute('aria-selected', 'true');
  await expect.poll(() => new URL(page.url()).searchParams.get('tab')).toBe('tiles');
});
