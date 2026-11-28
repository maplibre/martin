import { expect, test } from './test';

test('a catalog request that fails once recovers on its own', async ({ page, problems }) => {
  let failures = 0;
  await page.route('**/catalog', (route) =>
    failures++ === 0 ? route.fulfill({ status: 500 }) : route.fallback(),
  );

  await page.goto('/?tab=tiles');

  await expect(page.getByRole('heading', { name: '11 tile sources' })).toBeVisible();
  problems.splice(
    0,
    problems.length,
    ...problems.filter((p) => !/\/catalog\b|status of 500|Catalog (fetch|count) failed/.test(p)),
  );
});

test('when martin keeps failing, the page says so and try again recovers', async ({
  page,
  problems,
}) => {
  let down = true;
  await page.route('**/catalog', (route) =>
    down ? route.fulfill({ status: 500 }) : route.fallback(),
  );
  await page.goto('/?tab=tiles');
  await expect(page.getByText('Failed to Load Tiles Catalog')).toBeVisible({ timeout: 10_000 });
  down = false;

  await page.getByRole('button', { name: 'Try Again' }).click();

  await expect(page.getByRole('heading', { name: '11 tile sources' })).toBeVisible();
  problems.splice(
    0,
    problems.length,
    ...problems.filter((p) => !/\/catalog\b|status of 500|Catalog (fetch|count) failed/.test(p)),
  );
});
