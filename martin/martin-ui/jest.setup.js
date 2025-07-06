import "@testing-library/jest-dom";

// Mock Next.js router
jest.mock("next/router", () => ({
  useRouter: () => ({
    asPath: "/",
    back: jest.fn(),
    events: {
      emit: jest.fn(),
      off: jest.fn(),
      on: jest.fn(),
    },
    pathname: "/",
    push: jest.fn(),
    query: {},
    replace: jest.fn(),
  }),
}));

// Mock next/image
jest.mock("next/image", () => ({
  __esModule: true,
  // : Record<string, unknown>
  default: (props) => {
    // Use React.createElement to avoid JSX parse errors in non-TSX files
    // eslint-disable-next-line @typescript-eslint/no-var-requires
    const React = require("react");
    return React.createElement("img", props);
  },
}));

// Mock environment variables
process.env = {
  ...process.env,
  NEXT_PUBLIC_MARTIN_VERSION: "v0.0.0-test",
};

// Suppress console errors during tests
global.console = {
  ...console,
  // Uncomment to ignore specific console methods during tests
  // error: jest.fn(),
  // warn: jest.fn(),
};

// Mock window.matchMedia
Object.defineProperty(window, "matchMedia", {
  // query: string
  value: jest.fn().mockImplementation((query) => ({
    addEventListener: jest.fn(),
    addListener: jest.fn(),
    dispatchEvent: jest.fn(),
    matches: false, // Deprecated
    media: query, // Deprecated
    onchange: null,
    removeEventListener: jest.fn(),
    removeListener: jest.fn(),
  })),
  writable: true,
});
