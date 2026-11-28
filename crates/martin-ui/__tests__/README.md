# Martin UI unit tests

Vitest runs these in jsdom with `just test` (or `npm test`).
They cover logic that is cheaper to check without a browser: the style editor's models, parsers and widgets.

Anything a user sees through the whole stack (catalog pages, dialogs, maps, downloads, URLs) belongs in the end-to-end tests under `e2e/`, which drive the built UI against a real martin. See the [UI README](../README.md#end-to-end-tests).

## What to write

- **Properties first.** `*.properties.test.ts` files use [fast-check](https://fast-check.dev) to state rules that hold for every input, such as "every filter the editor writes passes the style spec" or "every edit sequence keeps the style valid". When a property finds a bug, also add the shrunk counterexample as a plain example test.
- **Examples for specific behaviour.** Name the test after the behaviour, inline its input, and assert on the output (inline snapshots are fine for data).
- **Widgets through their props.** Render a component with `@testing-library/react`, act like a user, and assert on what it calls or shows. Do not mock the project's own modules.
- **No DOM snapshots.** They break on every restyle and never catch a broken page.
