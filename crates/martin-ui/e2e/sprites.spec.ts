import { expect, test } from './test';

test('the sprite preview shows every icon martin lists for the sheet', async ({
  page,
  catalog,
}) => {
  const icons = catalog.sprites?.src1?.images ?? [];
  expect(icons.length).toBeGreaterThan(0);
  await page.goto('/?tab=sprites');

  await page
    .getByRole('article', { name: 'src1' })
    .getByRole('button', { name: 'Preview' })
    .click();

  const dialog = page.getByRole('dialog', { name: 'src1' });
  for (const icon of icons) {
    await expect(dialog.getByText(icon, { exact: true })).toBeVisible();
  }
});

test('every download link of a sprite sheet resolves', async ({ page, request }) => {
  await page.goto('/?tab=sprites&download=src1');

  const links = page.getByRole('dialog').getByRole('link', { name: /^Download / });
  await expect(links.first()).toBeVisible();
  for (const href of await links.evaluateAll((as) =>
    as.map((a) => (a as HTMLAnchorElement).href),
  )) {
    const response = await request.get(href);
    expect(response.status(), href).toBe(200);
    expect(response.headers()['content-type'], href).toBe(
      href.endsWith('.json') ? 'application/json' : 'image/png',
    );
  }
});
