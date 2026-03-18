/**
 * Parse Prometheus metrics text for request count and duration.
 * Used by MetricsPanel to show tile request stats from Martin's /_/metrics.
 */

export interface HistogramBucket {
  le: number;
  count: number;
}

/**
 * Parses Prometheus metrics text and extracts sum and count for each endpoint.
 */
export function parsePrometheusMetrics(text: string): {
  sum: Record<string, number>;
  count: Record<string, number>;
} {
  const lines = text.split('\n');
  const sum: Record<string, number> = {};
  const count: Record<string, number> = {};

  for (const line of lines) {
    const trimmed = line.trim();
    let match = trimmed.match(
      /^martin_http_requests_duration_seconds_sum\{(.*)\}\s+([0-9.eE+-]+)$/,
    );
    if (match) {
      const labels = match[1];
      const value = parseFloat(match[2]);
      const endpoint = /endpoint="([^"]+)"/.exec(labels)?.[1];
      if (endpoint) sum[endpoint] = (sum[endpoint] || 0) + value;
      continue;
    }
    match = trimmed.match(/^martin_http_requests_duration_seconds_count\{(.*)\}\s+([0-9.eE+-]+)$/);
    if (match) {
      const labels = match[1];
      const value = parseFloat(match[2]);
      const endpoint = /endpoint="([^"]+)"/.exec(labels)?.[1];
      if (endpoint) count[endpoint] = (count[endpoint] || 0) + value;
    }
  }
  return { count, sum };
}

/**
 * Martin tile route pattern in Prometheus metrics.
 * Metrics use the route template as endpoint label.
 */
const TILE_ROUTE = '/{source_ids}/{z}/{x}/{y}';

export function aggregateTileMetrics(
  sum: Record<string, number>,
  count: Record<string, number>,
): { requestCount: number; averageDurationMs: number } {
  const s = sum[TILE_ROUTE] ?? 0;
  const c = count[TILE_ROUTE] ?? 0;
  return {
    averageDurationMs: c > 0 ? (s / c) * 1000 : 0,
    requestCount: c,
  };
}
