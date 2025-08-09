/**
 * Get the base URL for Martin API endpoints
 * Uses VITE_MARTIN_BASE environment variable if set, otherwise defaults to current origin
 */
export function getMartinBaseUrl(): string {
  // we want to construct via origin+pathname as otherwise query/hash params result in wrong api urls
  return import.meta.env.VITE_MARTIN_BASE || window.location.origin + window.location.pathname;
}

/**
 * Build a complete URL for a Martin API endpoint
 * @param path - The API path (e.g., '/catalog', '/_/metrics')
 * @returns Complete URL with base URL prepended
 */
export function buildMartinUrl(path: string): string {
  const baseUrl = getMartinBaseUrl();

  // Ensure path starts with /
  const normalizedPath = path.startsWith('/') ? path : `/${path}`;

  // Remove trailing slash from base URL if present
  const normalizedBaseUrl = baseUrl.endsWith('/') ? baseUrl.slice(0, -1) : baseUrl;

  return `${normalizedBaseUrl}${normalizedPath}`;
}
