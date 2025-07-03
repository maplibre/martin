# Martin UI Snapshot Tests

This directory contains snapshot tests for the Martin UI components. Snapshot tests help ensure that the UI doesn't change unexpectedly between builds.

## Directory Structure

- `__tests__/components/`: Contains tests for components in the corresponding directories
- `__tests__/utils/`: Contains test utilities and helpers
- `__mocks__/`: Contains mock implementations for various modules

## Running Tests

To run the tests, you can use the following npm scripts:

```bash
# Run all tests
npm test

# Run tests in watch mode (good for development)
npm run test:watch

# Generate a coverage report
npm run test:coverage

# Update snapshots when intentional UI changes are made
npm run test:update-snapshots
```

## Adding New Tests

When adding new components to the UI, you should create corresponding snapshot tests:

1. Create a new test file in the appropriate directory (e.g., `__tests__/components/your-component.test.tsx`)
2. Import the component and render it with React Testing Library
3. Add test cases for different states and props of the component
4. Use `expect(container).toMatchSnapshot()` to create snapshots

Example:

```tsx
import { render } from "@testing-library/react";
import React from "react";
import { YourComponent } from "@/components/your-component";

describe("YourComponent", () => {
  it("renders correctly", () => {
    const { container } = render(<YourComponent />);
    expect(container).toMatchSnapshot();
  });

  it("renders with custom props", () => {
    const { container } = render(<YourComponent prop1="value" prop2={true} />);
    expect(container).toMatchSnapshot();
  });
});
```

## Updating Snapshots

When you make intentional changes to a component's UI, you'll need to update the snapshots:

```bash
npm run test:update-snapshots
```

You can also update specific snapshots in watch mode by pressing `u`.

## Testing Complex Components

For components that use context providers, hooks, or have complex dependencies:

1. Use the custom render method from `__tests__/utils/test-utils.tsx`
2. Mock the required dependencies and hooks
3. Break down the tests into smaller, focused test cases

## Integration with GitHub Actions

Snapshot tests are automatically run in GitHub Actions CI pipeline. The workflow configuration is in `.github/workflows/tests.yml`.