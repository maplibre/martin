import { buildMartinUrl, getMartinBaseUrl } from "@/lib/api";

// Mock process.env
const originalEnv = process.env;

describe("getMartinBaseUrl", () => {
  beforeEach(() => {
    // Reset process.env
    process.env = { ...originalEnv };
  });

  afterEach(() => {
    process.env = originalEnv;
  });

  it("returns environment variable value when NEXT_PUBLIC_MARTIN_BASE is set", () => {
    process.env.NEXT_PUBLIC_MARTIN_BASE = "https://api.example.com";
    expect(getMartinBaseUrl()).toBe("https://api.example.com");
  });

  it("returns fallback when NEXT_PUBLIC_MARTIN_BASE is not set", () => {
    delete process.env.NEXT_PUBLIC_MARTIN_BASE;

    const result = getMartinBaseUrl();

    // In test environment, this will be the jsdom default
    expect(result).toBe("http://localhost");
  });
});

describe("buildMartinUrl", () => {
  beforeEach(() => {
    // Reset process.env
    process.env = { ...originalEnv };
  });

  afterEach(() => {
    process.env = originalEnv;
  });

  it("builds URL with custom base URL from environment", () => {
    process.env.NEXT_PUBLIC_MARTIN_BASE = "https://api.example.com";

    const result = buildMartinUrl("/catalog");

    expect(result).toBe("https://api.example.com/catalog");
  });

  it("builds URL with fallback base URL when no environment variable is set", () => {
    delete process.env.NEXT_PUBLIC_MARTIN_BASE;

    const result = buildMartinUrl("/catalog");

    expect(result).toBe("http://localhost/catalog");
  });

  it("handles paths without leading slash", () => {
    process.env.NEXT_PUBLIC_MARTIN_BASE = "https://api.example.com";

    const result = buildMartinUrl("catalog");

    expect(result).toBe("https://api.example.com/catalog");
  });

  it("handles base URLs with trailing slash", () => {
    process.env.NEXT_PUBLIC_MARTIN_BASE = "https://api.example.com/";

    const result = buildMartinUrl("/catalog");

    expect(result).toBe("https://api.example.com/catalog");
  });

  it("handles complex paths", () => {
    process.env.NEXT_PUBLIC_MARTIN_BASE = "https://api.example.com";

    const result = buildMartinUrl("/sprite/test@2x.png");

    expect(result).toBe("https://api.example.com/sprite/test@2x.png");
  });

  it("handles metrics endpoint", () => {
    process.env.NEXT_PUBLIC_MARTIN_BASE = "https://api.example.com";

    const result = buildMartinUrl("/_/metrics");

    expect(result).toBe("https://api.example.com/_/metrics");
  });

  it("works with empty base URL", () => {
    process.env.NEXT_PUBLIC_MARTIN_BASE = "";

    const result = buildMartinUrl("/catalog");

    expect(result).toBe("/catalog");
  });
});
