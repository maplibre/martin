import { afterEach, describe, expect, it, vi } from 'vitest';
import { buildMartinUrl, getMartinBaseUrl } from '@/lib/api';

// Mock window.location for fallback tests
const mockLocation = {
  origin: 'http://localhost',
  pathname: '/',
};

describe('getMartinBaseUrl', () => {
  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it('returns environment variable value when VITE_MARTIN_BASE is set', () => {
    vi.stubEnv('VITE_MARTIN_BASE', 'https://api.example.com');
    expect(getMartinBaseUrl()).toBe('https://api.example.com');
  });

  it('returns origin + pathname when VITE_MARTIN_BASE is not set', () => {
    vi.stubEnv('VITE_MARTIN_BASE', '');

    // Mock window.location
    Object.defineProperty(window, 'location', {
      value: mockLocation,
      writable: true,
    });

    // window.location.pathname is "/"
    const result = getMartinBaseUrl();
    expect(result).toBe('http://localhost/');
  });
});

describe('buildMartinUrl', () => {
  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it('builds URL with custom base URL from environment', () => {
    vi.stubEnv('VITE_MARTIN_BASE', 'https://api.example.com');

    const result = buildMartinUrl('/catalog');

    expect(result).toBe('https://api.example.com/catalog');
  });

  it('builds URL with fallback base URL when no environment variable is set', () => {
    vi.stubEnv('VITE_MARTIN_BASE', '');

    // Mock window.location
    Object.defineProperty(window, 'location', {
      value: mockLocation,
      writable: true,
    });

    const result = buildMartinUrl('/catalog');

    // pathname
    expect(result).toBe('http://localhost/catalog');
  });

  it('handles paths without leading slash', () => {
    vi.stubEnv('VITE_MARTIN_BASE', 'https://api.example.com');

    const result = buildMartinUrl('catalog');

    expect(result).toBe('https://api.example.com/catalog');
  });

  it('handles base URLs with trailing slash', () => {
    vi.stubEnv('VITE_MARTIN_BASE', 'https://api.example.com/');

    const result = buildMartinUrl('/catalog');

    expect(result).toBe('https://api.example.com/catalog');
  });

  it('handles complex paths', () => {
    vi.stubEnv('VITE_MARTIN_BASE', 'https://api.example.com');

    const result = buildMartinUrl('/sprite/test@2x.png');

    expect(result).toBe('https://api.example.com/sprite/test@2x.png');
  });

  it('handles metrics endpoint', () => {
    vi.stubEnv('VITE_MARTIN_BASE', 'https://api.example.com');

    const result = buildMartinUrl('/_/metrics');

    expect(result).toBe('https://api.example.com/_/metrics');
  });

  it('works with empty base URL', () => {
    vi.stubEnv('VITE_MARTIN_BASE', '');

    // Mock window.location
    Object.defineProperty(window, 'location', {
      value: mockLocation,
      writable: true,
    });

    const result = buildMartinUrl('/catalog');

    expect(result).toBe('http://localhost/catalog');
  });
});
