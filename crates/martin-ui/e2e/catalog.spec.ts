import { expect, test } from './test';

const TABS = [
  { key: 'tiles', noun: ['tile source', 'tile sources'] },
  { key: 'styles', noun: ['style', 'styles'] },
  { key: 'fonts', noun: ['font', 'fonts'] },
  { key: 'sprites', noun: ['sprite sheet', 'sprite sheets'] },
] as const;

for (const { key, noun } of TABS) {
  test(`the ${key} tab lists every ${noun[0]} martin serves`, async ({ page, catalog }) => {
    const names = Object.keys(catalog[key] ?? {});
    expect(names.length).toBeGreaterThan(0);

    const heading = `${names.length} ${names.length === 1 ? noun[0] : noun[1]}`;

    await page.goto(`/?tab=${key}`);

    await expect(page.getByRole('heading', { name: heading })).toBeVisible();
    for (const name of names) {
      await expect(page.getByText(name, { exact: true }).first()).toBeVisible();
    }
  });
}
