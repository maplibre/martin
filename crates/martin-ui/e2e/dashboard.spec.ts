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
  expect((await request.get('/png/0/0/0')).ok()).toBe(true);

  await page.goto('/');

  await expect(page.getByText(/^[\d,]+ requests$/).first()).toBeVisible();
});

test('every tab is reachable from the tab bar and lands in the url', async ({ page }) => {
  await page.goto('/');

  for (const [label, tab] of [
    ['Styles', 'styles'],
    ['Fonts', 'fonts'],
    ['Sprites', 'sprites'],
    ['Tiles', 'tiles'],
  ]) {
    await page.getByRole('tab', { name: label }).click();
    await expect(page.getByRole('tab', { name: label })).toHaveAttribute('aria-selected', 'true');
    await expect.poll(() => new URL(page.url()).searchParams.get('tab')).toBe(tab);
  }
});
