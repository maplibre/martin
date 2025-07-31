/**
 * Get the base URL for Martin API endpoints
 * Uses VITE_MARTIN_BASE environment variable if set, otherwise defaults to current origin
 */
export function getMartinBaseUrl(): string {
  // grumble grumble
  // Belows try-except is the poor mans `import.meta.env?.VITE_MARTIN_BASE`
  //
  // - `import.meta.env` is `undefined` after building and
  // - `import.meta.env.VITE_MARTIN_BASE` is not replaced with a value if not set.
  //
  // We have to do this like this as jest does not understand `import.meta.env?.VITE_MARTIN_BASE`
  let importedMeta: string | undefined;
  try {
    importedMeta = import.meta.env.VITE_MARTIN_BASE;
  } catch (_error) {}
  return importedMeta ?? window.location.href ?? '';
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
