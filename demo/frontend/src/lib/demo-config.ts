/**
 * Martin base URL for the demo (tiles and metrics).
 * Use PUBLIC_MARTIN_BASE_URL env at build time, or fall back to the public demo instance.
 */
export const MARTIN_BASE_URL =
  (typeof import.meta.env !== 'undefined' &&
    (import.meta.env as Record<string, string>).PUBLIC_MARTIN_BASE_URL) ||
  'https://martin.maplibre.org';

export function buildMartinTileUrl(baseUrl: string, pathOrFull: string): string {
  if (pathOrFull.startsWith('http://') || pathOrFull.startsWith('https://')) {
    return pathOrFull;
  }
  const base = baseUrl.replace(/\/$/, '');
  const path = pathOrFull.startsWith('/') ? pathOrFull : `/${pathOrFull}`;
  return `${base}${path}`;
}
