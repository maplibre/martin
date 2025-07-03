/**
 * Parses Prometheus metrics text and extracts sum and count for each endpoint.
 * Returns an object with two maps: sum and count, keyed by endpoint string.
 */
export function parsePrometheusMetrics(text: string): {
  sum: Record<string, number>;
  count: Record<string, number>;
} {
  const lines = text.split("\n");
  const sum: Record<string, number> = {};
  const count: Record<string, number> = {};

  for (const line of lines) {
    const trimmed = line.trim();
    // Example: martin_http_requests_duration_seconds_sum{endpoint="/sprite/{source_ids}.json",method="GET",status="200"} 123.456
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
 * Aggregates endpoint metrics into logical groups and computes average duration and request count.
 *
 * @param sum - Record of endpoint to sum of durations
 * @param count - Record of endpoint to count of requests
 * @param endpointGroups - Object mapping group name to array of endpoint patterns
 * @returns Object mapping group name to { averageRequestDurationMs, requestCount }
 */
export function aggregateEndpointGroups(
  sum: Record<string, number>,
  count: Record<string, number>,
  endpointGroups: Record<string, string[]>,
): Record<string, { averageRequestDurationMs: number; requestCount: number }> {
  const result: Record<string, { averageRequestDurationMs: number; requestCount: number }> = {};
  for (const [group, endpoints] of Object.entries(endpointGroups)) {
    let totalSum = 0;
    let totalCount = 0;
    for (const endpoint of endpoints) {
      totalSum += sum[endpoint] || 0;
      totalCount += count[endpoint] || 0;
    }
    result[group] = {
      averageRequestDurationMs: totalCount > 0 ? (totalSum / totalCount) * 1000 : 0,
      requestCount: totalCount,
    };
  }
  return result;
}

export const ENDPOINT_GROUPS = {
  fonts: ["/font/{fontstack}/{start}-{end}"],
  sprites: [
    "/sprite/{source_ids}.json",
    "/sprite/{source_ids}.png",
    "/sdf_sprite/{source_ids}.json",
    "/sdf_sprite/{source_ids}.png",
  ],
  styles: ["/style/{style_id}"],
  tiles: ["/{source_ids}/{z}/{x}/{y}"],
} as Record<string, string[]>;