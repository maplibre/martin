import { buildMartinUrl, getMartinBaseUrl } from "@/lib/api";

// Mock the environment variables by setting process.env
// (Jest transform converts import.meta.env to process.env)
const originalProcessEnv = process.env;

describe("getMartinBaseUrl", () => {
  afterEach(() => {
    // Restore original process.env
    process.env = { ...originalProcessEnv };
  });

  it("returns environment variable value when VITE_MARTIN_BASE is set", () => {
    process.env.VITE_MARTIN_BASE = "https://api.example.com";
    expect(getMartinBaseUrl()).toBe("https://api.example.com");
  });

  it("returns window.location.origin when VITE_MARTIN_BASE is not set", () => {
    delete process.env.VITE_MARTIN_BASE;

    // In Jest/jsdom environment, window.location.origin is "http://localhost"
    // This is set up in jest.setup.js
    const result = getMartinBaseUrl();
    expect(result).toBeDefined();
    expect(typeof result).toBe("string");
    // The actual value depends on the Jest setup, but it should be a valid URL origin
    expect(result).toMatch(/^https?:\/\/[^\/]+$/);
  });
});

describe("buildMartinUrl", () => {
  afterEach(() => {
    // Restore original process.env
    process.env = { ...originalProcessEnv };
  });

  it("builds URL with custom base URL from environment", () => {
    process.env.VITE_MARTIN_BASE = "https://api.example.com";

    const result = buildMartinUrl("/catalog");

    expect(result).toBe("https://api.example.com/catalog");
  });

  it("builds URL with fallback base URL when no environment variable is set", () => {
    delete process.env.VITE_MARTIN_BASE;

    const result = buildMartinUrl("/catalog");

    // Should use window.location.origin as fallback
    expect(result).toMatch(/^https?:\/\/[^\/]+\/catalog$/);
  });

  it("handles paths without leading slash", () => {
    process.env.VITE_MARTIN_BASE = "https://api.example.com";

    const result = buildMartinUrl("catalog");

    expect(result).toBe("https://api.example.com/catalog");
  });

  it("handles base URLs with trailing slash", () => {
    process.env.VITE_MARTIN_BASE = "https://api.example.com/";

    const result = buildMartinUrl("/catalog");

    expect(result).toBe("https://api.example.com/catalog");
  });

  it("handles complex paths", () => {
    process.env.VITE_MARTIN_BASE = "https://api.example.com";

    const result = buildMartinUrl("/sprite/test@2x.png");

    expect(result).toBe("https://api.example.com/sprite/test@2x.png");
  });

  it("handles metrics endpoint", () => {
    process.env.VITE_MARTIN_BASE = "https://api.example.com";

    const result = buildMartinUrl("/_/metrics");

    expect(result).toBe("https://api.example.com/_/metrics");
  });

  it("works with empty base URL", () => {
    process.env.VITE_MARTIN_BASE = "";

    const result = buildMartinUrl("/catalog");

    expect(result).toBe("/catalog");
  });
});
