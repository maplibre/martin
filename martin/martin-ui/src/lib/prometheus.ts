export interface HistogramBucket {
	le: number;
	count: number;
}

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
		match = trimmed.match(
			/^martin_http_requests_duration_seconds_count\{(.*)\}\s+([0-9.eE+-]+)$/,
		);
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
	endpointGroups: Record<string, readonly string[]>,
): Record<string, { averageRequestDurationMs: number; requestCount: number }> {
	const result: Record<
		string,
		{ averageRequestDurationMs: number; requestCount: number }
	> = {};
	for (const [group, endpoints] of Object.entries(endpointGroups)) {
		let totalSum = 0;
		let totalCount = 0;
		for (const endpoint of endpoints) {
			totalSum += sum[endpoint] || 0;
			totalCount += count[endpoint] || 0;
		}
		result[group] = {
			averageRequestDurationMs:
				totalCount > 0 ? (totalSum / totalCount) * 1000 : 0,
			requestCount: totalCount,
		};
	}
	return result;
}

/**
 * Parses Prometheus histogram metrics and extracts bucket data for each endpoint.
 * Returns an object mapping endpoint strings to histogram data.
 */
export function parsePrometheusHistogram(
	text: string,
): Record<string, HistogramBucket[]> {
	const lines = text.split("\n");
	const histograms: Record<string, HistogramBucket[]> = {};

	for (const line of lines) {
		const trimmed = line.trim();

		// Parse bucket lines: martin_http_requests_duration_seconds_bucket{...le="0.1"} 123
		const match = trimmed.match(
			/^martin_http_requests_duration_seconds_bucket\{(.*)\}\s+([0-9.eE+-]+)$/,
		);
		if (match) {
			const labels = match[1];
			const count = parseFloat(match[2]);
			const endpoint = /endpoint="([^"]+)"/.exec(labels)?.[1];
			const leMatch = /le="([^"]+)"/.exec(labels);

			if (endpoint && leMatch) {
				const le = leMatch[1] === "+Inf" ? Infinity : parseFloat(leMatch[1]);

				if (!histograms[endpoint]) {
					histograms[endpoint] = [];
				}

				// Skip +Inf bucket as it's redundant with count
				if (le !== Infinity) {
					histograms[endpoint].push({ count, le });
				}
			}
		}
	}

	// Sort buckets by le value for each endpoint
	for (const histogram of Object.values(histograms)) {
		histogram.sort((a, b) => a.le - b.le);
	}

	return histograms;
}

/**
 * Parses complete Prometheus metrics including sum, count, and histogram data.
 * Returns an object with all three types of metrics for comprehensive analysis.
 */
export function parseCompletePrometheusMetrics(text: string): {
	sum: Record<string, number>;
	count: Record<string, number>;
	histograms: Record<string, HistogramBucket[]>;
} {
	return {
		...parsePrometheusMetrics(text),
		histograms: parsePrometheusHistogram(text),
	};
}

/**
 * Aggregates histogram data for endpoint groups by combining multiple endpoints.
 * Properly handles cumulative histogram buckets and adds histograms together.
 *
 * @param histograms - Record of endpoint to histogram data
 * @param endpointGroups - Object mapping group name to array of endpoint patterns
 * @returns Object mapping group name to aggregated histogram data
 */
export function aggregateHistogramGroups(
	histograms: Record<string, HistogramBucket[]>,
	endpointGroups: Record<string, readonly string[]>,
): Record<string, HistogramBucket[]> {
	const result: Record<string, HistogramBucket[]> = {};

	for (const [group, endpoints] of Object.entries(endpointGroups)) {
		// Find all histograms that belong to this group
		const groupHistograms: HistogramBucket[][] = [];
		for (const endpoint of endpoints) {
			if (histograms[endpoint]) {
				groupHistograms.push(histograms[endpoint]);
			}
		}

		if (groupHistograms.length === 0) {
			// No histogram data for this group
			continue;
		}

		// Collect all unique bucket boundaries (le values)
		const allBuckets = new Set<number>();
		for (const hist of groupHistograms) {
			for (const bucket of hist) {
				allBuckets.add(bucket.le);
			}
		}

		// Sort bucket boundaries
		const sortedBuckets = Array.from(allBuckets).sort((a, b) => a - b);

		// Create aggregated histogram
		const aggregatedBuckets: HistogramBucket[] = [];

		// For each bucket boundary, sum up counts from all histograms
		for (const le of sortedBuckets) {
			let bucketCount = 0;

			for (const hist of groupHistograms) {
				// Find the cumulative count up to this le value
				let cumulativeCount = 0;
				for (const bucket of hist) {
					if (bucket.le <= le) {
						cumulativeCount = bucket.count;
					} else {
						break;
					}
				}
				bucketCount += cumulativeCount;
			}

			aggregatedBuckets.push({ count: bucketCount, le });
		}

		result[group] = aggregatedBuckets;
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
	tiles: ["/{source_ids}/{z}/{x}/{y}", "/{source_ids}"],
} as Record<string, readonly string[]>;
