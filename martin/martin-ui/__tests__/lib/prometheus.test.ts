import { describe, expect, it } from "@jest/globals";

// Import the functions from the correct location
import {
	aggregateEndpointGroups,
	aggregateHistogramGroups,
	calculateHistogramPercentiles,
	parseCompletePrometheusMetrics,
	parsePrometheusHistogram,
	parsePrometheusMetrics,
} from "@/lib/prometheus";

describe("parsePrometheusMetrics", () => {
	it("parses sum and count for multiple endpoints", () => {
		const metrics = [
			"# HELP martin_http_requests_duration_seconds HTTP request duration in seconds for all requests",
			"# TYPE martin_http_requests_duration_seconds histogram",
			'martin_http_requests_duration_seconds_sum{endpoint="/sprite/{source_ids}.json",method="GET",status="200"} 12.5',
			'martin_http_requests_duration_seconds_count{endpoint="/sprite/{source_ids}.json",method="GET",status="200"} 50',
			'martin_http_requests_duration_seconds_sum{endpoint="/font/{fontstack}/{start}-{end}",method="GET",status="200"} 3.2',
			'martin_http_requests_duration_seconds_count{endpoint="/font/{fontstack}/{start}-{end}",method="GET",status="200"} 8',
			'martin_http_requests_duration_seconds_sum{endpoint="/style/{style_id}",method="GET",status="200"} 7.0',
			'martin_http_requests_duration_seconds_count{endpoint="/style/{style_id}",method="GET",status="200"} 14',
		].join("\n");
		const { sum, count } = parsePrometheusMetrics(metrics);

		expect(sum["/sprite/{source_ids}.json"]).toBe(12.5);
		expect(count["/sprite/{source_ids}.json"]).toBe(50);

		expect(sum["/font/{fontstack}/{start}-{end}"]).toBe(3.2);
		expect(count["/font/{fontstack}/{start}-{end}"]).toBe(8);

		expect(sum["/style/{style_id}"]).toBe(7.0);
		expect(count["/style/{style_id}"]).toBe(14);
	});

	it("ignores unrelated metrics", () => {
		const metrics = [
			'other_metric{foo="bar"} 123',
			'martin_http_requests_duration_seconds_sum{endpoint="/catalog",method="GET",status="200"} 1.1',
			'martin_http_requests_duration_seconds_count{endpoint="/catalog",method="GET",status="200"} 2',
		].join("\n");

		const { sum, count } = parsePrometheusMetrics(metrics);

		expect(sum["/catalog"]).toBe(1.1);
		expect(count["/catalog"]).toBe(2);
		expect(sum.foo).toBeUndefined();
		expect(count.foo).toBeUndefined();
	});

	it("handles missing sum or count values gracefully", () => {
		const metrics = [
			'martin_http_requests_duration_seconds_sum{endpoint="/sprite/{source_ids}.json",method="GET",status="200"} 5.5',
			"# No count for this endpoint",
			'martin_http_requests_duration_seconds_count{endpoint="/style/{style_id}",method="GET",status="200"} 10',
			"# No sum for this endpoint",
		].join("\n");

		const { sum, count } = parsePrometheusMetrics(metrics);

		expect(sum["/sprite/{source_ids}.json"]).toBe(5.5);
		expect(count["/sprite/{source_ids}.json"]).toBeUndefined();

		expect(sum["/style/{style_id}"]).toBeUndefined();
		expect(count["/style/{style_id}"]).toBe(10);
	});

	describe("aggregateEndpointGroups", () => {
		it("aggregates metrics for defined endpoint groups", () => {
			const sum = {
				"/font/{fontstack}/{start}-{end}": 5,
				"/sprite/{source_ids}.json": 10,
				"/sprite/{source_ids}.png": 20,
				"/style/{style_id}": 8,
			};
			const count = {
				"/font/{fontstack}/{start}-{end}": 1,
				"/sprite/{source_ids}.json": 2,
				"/sprite/{source_ids}.png": 4,
				"/style/{style_id}": 8,
			};
			const endpointGroups = {
				fonts: ["/font/{fontstack}/{start}-{end}"],
				missing: ["/not_present"],
				sprites: ["/sprite/{source_ids}.json", "/sprite/{source_ids}.png"],
				styles: ["/style/{style_id}"],
			};

			const result = aggregateEndpointGroups(sum, count, endpointGroups);

			// Sprites: sum = 10+20=30, count = 2+4=6, avg = (30/6)*1000 = 5000
			expect(result.sprites.averageRequestDurationMs).toBeCloseTo(5000);
			expect(result.sprites.requestCount).toBe(6);

			// Fonts: sum = 5, count = 1, avg = 5000
			expect(result.fonts.averageRequestDurationMs).toBeCloseTo(5000);
			expect(result.fonts.requestCount).toBe(1);

			// Styles: sum = 8, count = 8, avg = 1000
			expect(result.styles.averageRequestDurationMs).toBeCloseTo(1000);
			expect(result.styles.requestCount).toBe(8);

			// Missing: sum = 0, count = 0, avg = 0
			expect(result.missing.averageRequestDurationMs).toBe(0);
			expect(result.missing.requestCount).toBe(0);
		});

		it("handles empty input gracefully", () => {
			const sum = {};
			const count = {};
			const endpointGroups = {
				group1: ["/foo"],
				group2: ["/bar"],
			};
			const result = aggregateEndpointGroups(sum, count, endpointGroups);
			expect(result.group1.averageRequestDurationMs).toBe(0);
			expect(result.group1.requestCount).toBe(0);
			expect(result.group2.averageRequestDurationMs).toBe(0);
			expect(result.group2.requestCount).toBe(0);
		});
	});

	describe("parsePrometheusHistogram", () => {
		it("parses histogram buckets for an endpoint", () => {
			const metrics = [
				"# HELP martin_http_requests_duration_seconds HTTP request duration in seconds for all requests",
				"# TYPE martin_http_requests_duration_seconds histogram",
				'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="0.005"} 23004',
				'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="0.01"} 23045',
				'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="0.025"} 23228',
				'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="0.05"} 23410',
				'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="0.1"} 23637',
				'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="0.25"} 23722',
				'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="0.5"} 23735',
				'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="1"} 23746',
				'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="2.5"} 23747',
				'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="5"} 23747',
				'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="10"} 23747',
				'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="+Inf"} 23747',
				'martin_http_requests_duration_seconds_sum{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200"} 61.49839745299979',
				'martin_http_requests_duration_seconds_count{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200"} 23747',
			].join("\n");

			const histogram = parsePrometheusHistogram(metrics);
			const tileEndpoint = "/{source_ids}/{z}/{x}/{y}";

			expect(histogram[tileEndpoint]).toBeDefined();
			expect(histogram[tileEndpoint].buckets).toHaveLength(11); // All buckets except +Inf
			expect(histogram[tileEndpoint].buckets[0]).toEqual({
				count: 23004,
				le: 0.005,
			});
			expect(histogram[tileEndpoint].buckets[10]).toEqual({
				count: 23747,
				le: 10,
			});
			expect(histogram[tileEndpoint].sum).toBe(61.49839745299979);
			expect(histogram[tileEndpoint].count).toBe(23747);
		});

		it("handles multiple endpoints with histograms", () => {
			const metrics = [
				'martin_http_requests_duration_seconds_bucket{endpoint="/sprite/{source_ids}.json",method="GET",status="200",le="0.005"} 100',
				'martin_http_requests_duration_seconds_bucket{endpoint="/sprite/{source_ids}.json",method="GET",status="200",le="0.01"} 150',
				'martin_http_requests_duration_seconds_bucket{endpoint="/sprite/{source_ids}.json",method="GET",status="200",le="+Inf"} 200',
				'martin_http_requests_duration_seconds_sum{endpoint="/sprite/{source_ids}.json",method="GET",status="200"} 2.5',
				'martin_http_requests_duration_seconds_count{endpoint="/sprite/{source_ids}.json",method="GET",status="200"} 200',
				'martin_http_requests_duration_seconds_bucket{endpoint="/style/{style_id}",method="GET",status="200",le="0.005"} 50',
				'martin_http_requests_duration_seconds_bucket{endpoint="/style/{style_id}",method="GET",status="200",le="0.01"} 80',
				'martin_http_requests_duration_seconds_bucket{endpoint="/style/{style_id}",method="GET",status="200",le="+Inf"} 100',
				'martin_http_requests_duration_seconds_sum{endpoint="/style/{style_id}",method="GET",status="200"} 1.2',
				'martin_http_requests_duration_seconds_count{endpoint="/style/{style_id}",method="GET",status="200"} 100',
			].join("\n");

			const histogram = parsePrometheusHistogram(metrics);

			expect(Object.keys(histogram)).toHaveLength(2);
			expect(histogram["/sprite/{source_ids}.json"].buckets).toHaveLength(2);
			expect(histogram["/style/{style_id}"].buckets).toHaveLength(2);
			expect(histogram["/sprite/{source_ids}.json"].sum).toBe(2.5);
			expect(histogram["/style/{style_id}"].count).toBe(100);
		});

		it("ignores non-histogram metrics", () => {
			const metrics = [
				'other_metric{foo="bar"} 123',
				'martin_http_requests_duration_seconds_bucket{endpoint="/test",method="GET",status="200",le="0.1"} 50',
				'martin_http_requests_duration_seconds_sum{endpoint="/test",method="GET",status="200"} 1.0',
				'martin_http_requests_duration_seconds_count{endpoint="/test",method="GET",status="200"} 50',
				'unrelated_bucket{le="0.1"} 999',
			].join("\n");

			const histogram = parsePrometheusHistogram(metrics);

			expect(Object.keys(histogram)).toHaveLength(1);
			expect(histogram["/test"]).toBeDefined();
			expect(histogram.other_metric).toBeUndefined();
		});

		it("handles missing sum or count gracefully", () => {
			const metrics = [
				'martin_http_requests_duration_seconds_bucket{endpoint="/incomplete",method="GET",status="200",le="0.1"} 50',
				'martin_http_requests_duration_seconds_sum{endpoint="/incomplete",method="GET",status="200"} 1.0',
				// Missing count
			].join("\n");

			const histogram = parsePrometheusHistogram(metrics);

			expect(histogram["/incomplete"]).toBeDefined();
			expect(histogram["/incomplete"].buckets).toHaveLength(1);
			expect(histogram["/incomplete"].sum).toBe(1.0);
			expect(histogram["/incomplete"].count).toBeUndefined();
		});
	});

	describe("calculateHistogramPercentiles", () => {
		it("calculates percentiles correctly from histogram buckets", () => {
			const histogram = {
				buckets: [
					{ count: 100, le: 0.005 },
					{ count: 200, le: 0.01 },
					{ count: 300, le: 0.025 },
					{ count: 400, le: 0.05 },
					{ count: 450, le: 0.1 },
					{ count: 480, le: 0.25 },
					{ count: 490, le: 0.5 },
					{ count: 500, le: 1 },
				],
				count: 500,
				sum: 25.0,
			};

			const percentiles = calculateHistogramPercentiles(
				histogram,
				[50, 95, 99],
			);

			// P50 should be around 0.025 (300/500 = 60%, so 50% is between 0.01 and 0.025)
			expect(percentiles.p50).toBeGreaterThan(0.01);
			expect(percentiles.p50).toBeLessThanOrEqual(0.025);

			// P95 should be around 0.25 (480/500 = 96%, so 95% is between 0.1 and 0.25)
			expect(percentiles.p95).toBeGreaterThan(0.1);
			expect(percentiles.p95).toBeLessThanOrEqual(0.25);

			// P99 should be around 0.5 (490/500 = 98%, so 99% is between 0.25 and 0.5)
			expect(percentiles.p99).toBeGreaterThan(0.25);
			expect(percentiles.p99).toBeLessThanOrEqual(1.0);
		});

		it("handles edge cases for percentile calculation", () => {
			const histogram = {
				buckets: [{ count: 1000, le: 0.1 }],
				count: 1000,
				sum: 50.0,
			};

			const percentiles = calculateHistogramPercentiles(
				histogram,
				[50, 95, 99],
			);

			// All requests are in the first bucket, so all percentiles should be <= 0.1
			expect(percentiles.p50).toBeLessThanOrEqual(0.1);
			expect(percentiles.p95).toBeLessThanOrEqual(0.1);
			expect(percentiles.p99).toBeLessThanOrEqual(0.1);
		});

		it("returns 0 for empty histogram", () => {
			const histogram = {
				buckets: [],
				count: 0,
				sum: 0,
			};

			const percentiles = calculateHistogramPercentiles(
				histogram,
				[50, 95, 99],
			);

			expect(percentiles.p50).toBe(0);
			expect(percentiles.p95).toBe(0);
			expect(percentiles.p99).toBe(0);
		});
	});

	describe("Real world histogram data", () => {
		it("parses the provided sample histogram data correctly", () => {
			const sampleMetrics = [
				"# HELP martin_http_requests_duration_seconds HTTP request duration in seconds for all requests",
				"# TYPE martin_http_requests_duration_seconds histogram",
				'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="0.005"} 23004',
				'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="0.01"} 23045',
				'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="0.025"} 23228',
				'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="0.05"} 23410',
				'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="0.1"} 23637',
				'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="0.25"} 23722',
				'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="0.5"} 23735',
				'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="1"} 23746',
				'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="2.5"} 23747',
				'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="5"} 23747',
				'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="10"} 23747',
				'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="+Inf"} 23747',
				'martin_http_requests_duration_seconds_sum{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200"} 61.49839745299979',
				'martin_http_requests_duration_seconds_count{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200"} 23747',
			].join("\n");

			const histogram = parsePrometheusHistogram(sampleMetrics);
			const tileEndpoint = "/{source_ids}/{z}/{x}/{y}";

			expect(histogram[tileEndpoint]).toBeDefined();
			expect(histogram[tileEndpoint].count).toBe(23747);
			expect(histogram[tileEndpoint].sum).toBeCloseTo(61.49839745299979);

			// Calculate percentiles for the sample data
			const percentiles = calculateHistogramPercentiles(
				histogram[tileEndpoint],
				[50, 95, 99],
			);

			// Most requests (23004/23747 = 97%) are under 5ms, so percentiles should be very low
			expect(percentiles.p50).toBeLessThan(0.005);
			expect(percentiles.p95).toBeLessThan(0.1);
			expect(percentiles.p99).toBeLessThan(0.25);

			// Verify average request duration
			const avgDurationMs =
				(histogram[tileEndpoint].sum! / histogram[tileEndpoint].count!) * 1000;
			expect(avgDurationMs).toBeCloseTo(2.59, 1); // ~2.59ms average
		});

		describe("parseCompletePrometheusMetrics", () => {
			it("parses all metrics types (sum, count, histograms) in one call", () => {
				const metrics = [
					"# HELP martin_http_requests_duration_seconds HTTP request duration in seconds for all requests",
					"# TYPE martin_http_requests_duration_seconds histogram",
					'martin_http_requests_duration_seconds_bucket{endpoint="/test",method="GET",status="200",le="0.1"} 50',
					'martin_http_requests_duration_seconds_bucket{endpoint="/test",method="GET",status="200",le="0.5"} 80',
					'martin_http_requests_duration_seconds_bucket{endpoint="/test",method="GET",status="200",le="+Inf"} 100',
					'martin_http_requests_duration_seconds_sum{endpoint="/test",method="GET",status="200"} 5.0',
					'martin_http_requests_duration_seconds_count{endpoint="/test",method="GET",status="200"} 100',
				].join("\n");

				const result = parseCompletePrometheusMetrics(metrics);

				// Check sum and count are parsed
				expect(result.sum["/test"]).toBe(5.0);
				expect(result.count["/test"]).toBe(100);

				// Check histogram is parsed
				expect(result.histograms["/test"]).toBeDefined();
				expect(result.histograms["/test"].buckets).toHaveLength(2);
				expect(result.histograms["/test"].sum).toBe(5.0);
				expect(result.histograms["/test"].count).toBe(100);
			});
		});

		describe("aggregateHistogramGroups", () => {
			it("aggregates multiple endpoints into a single group histogram", () => {
				const histograms = {
					"/sprite/{source_ids}.json": {
						buckets: [
							{ le: 0.005, count: 100 },
							{ le: 0.01, count: 150 },
							{ le: 0.025, count: 180 },
						],
						sum: 1.5,
						count: 180,
					},
					"/sprite/{source_ids}.png": {
						buckets: [
							{ le: 0.005, count: 50 },
							{ le: 0.01, count: 80 },
							{ le: 0.025, count: 120 },
						],
						sum: 1.0,
						count: 120,
					},
				};

				const endpointGroups = {
					sprites: ["/sprite/{source_ids}.json", "/sprite/{source_ids}.png"],
					tiles: ["/{source_ids}/{z}/{x}/{y}"],
				};

				const result = aggregateHistogramGroups(histograms, endpointGroups);

				expect(result.sprites).toBeDefined();
				expect(result.sprites.buckets).toHaveLength(3);

				// Buckets should be aggregated: 100+50=150, 150+80=230, 180+120=300
				expect(result.sprites.buckets[0]).toEqual({ le: 0.005, count: 150 });
				expect(result.sprites.buckets[1]).toEqual({ le: 0.01, count: 230 });
				expect(result.sprites.buckets[2]).toEqual({ le: 0.025, count: 300 });

				// Sum and count should be aggregated
				expect(result.sprites.sum).toBe(2.5);
				expect(result.sprites.count).toBe(300);

				// Tiles group should not exist since no histogram data
				expect(result.tiles).toBeUndefined();
			});

			it("handles different bucket boundaries across endpoints", () => {
				const histograms = {
					"/sprite/{source_ids}.json": {
						buckets: [
							{ le: 0.005, count: 100 },
							{ le: 0.025, count: 150 },
						],
						sum: 1.0,
						count: 150,
					},
					"/sprite/{source_ids}.png": {
						buckets: [
							{ le: 0.01, count: 50 },
							{ le: 0.05, count: 80 },
						],
						sum: 0.8,
						count: 80,
					},
				};

				const endpointGroups = {
					sprites: ["/sprite/{source_ids}.json", "/sprite/{source_ids}.png"],
				};

				const result = aggregateHistogramGroups(histograms, endpointGroups);

				expect(result.sprites).toBeDefined();
				// Should have 4 unique bucket boundaries: 0.005, 0.01, 0.025, 0.05
				expect(result.sprites.buckets).toHaveLength(4);

				// Check aggregation with different bucket boundaries
				expect(result.sprites.buckets[0]).toEqual({ le: 0.005, count: 100 }); // Only from json
				expect(result.sprites.buckets[1]).toEqual({ le: 0.01, count: 150 }); // 100 + 50
				expect(result.sprites.buckets[2]).toEqual({ le: 0.025, count: 200 }); // 150 + 50
				expect(result.sprites.buckets[3]).toEqual({ le: 0.05, count: 230 }); // 150 + 80
			});

			it("handles single endpoint in group", () => {
				const histograms = {
					"/{source_ids}/{z}/{x}/{y}": {
						buckets: [
							{ le: 0.005, count: 1000 },
							{ le: 0.01, count: 1200 },
						],
						sum: 5.0,
						count: 1200,
					},
				};

				const endpointGroups = {
					tiles: ["/{source_ids}/{z}/{x}/{y}"],
				};

				const result = aggregateHistogramGroups(histograms, endpointGroups);

				expect(result.tiles).toBeDefined();
				expect(result.tiles.buckets).toHaveLength(2);
				expect(result.tiles.buckets[0]).toEqual({ le: 0.005, count: 1000 });
				expect(result.tiles.buckets[1]).toEqual({ le: 0.01, count: 1200 });
				expect(result.tiles.sum).toBe(5.0);
				expect(result.tiles.count).toBe(1200);
			});

			it("returns empty result when no histogram data available", () => {
				const histograms = {};
				const endpointGroups = {
					sprites: ["/sprite/{source_ids}.json"],
					tiles: ["/{source_ids}/{z}/{x}/{y}"],
				};

				const result = aggregateHistogramGroups(histograms, endpointGroups);

				expect(Object.keys(result)).toHaveLength(0);
			});

			it("handles partial histogram data (some endpoints missing)", () => {
				const histograms = {
					"/sprite/{source_ids}.json": {
						buckets: [{ le: 0.01, count: 100 }],
						sum: 1.0,
						count: 100,
					},
					// "/sprite/{source_ids}.png" is missing
				};

				const endpointGroups = {
					sprites: ["/sprite/{source_ids}.json", "/sprite/{source_ids}.png"],
				};

				const result = aggregateHistogramGroups(histograms, endpointGroups);

				expect(result.sprites).toBeDefined();
				expect(result.sprites.buckets).toHaveLength(1);
				expect(result.sprites.buckets[0]).toEqual({ le: 0.01, count: 100 });
				expect(result.sprites.sum).toBe(1.0);
				expect(result.sprites.count).toBe(100);
			});
		});
	});

	describe("Integration test: Corrected histogram processing", () => {
		it("properly handles cumulative histogram buckets and multi-endpoint aggregation", () => {
			// Sample data with multiple sprite endpoints showing cumulative histogram nature
			const sampleMetrics = [
				"# HELP martin_http_requests_duration_seconds HTTP request duration in seconds for all requests",
				"# TYPE martin_http_requests_duration_seconds histogram",
				// Sprite JSON endpoint - cumulative buckets (le = "less than or equal")
				'martin_http_requests_duration_seconds_bucket{endpoint="/sprite/{source_ids}.json",method="GET",status="200",le="0.005"} 100',
				'martin_http_requests_duration_seconds_bucket{endpoint="/sprite/{source_ids}.json",method="GET",status="200",le="0.01"} 150', // 50 more requests (150-100)
				'martin_http_requests_duration_seconds_bucket{endpoint="/sprite/{source_ids}.json",method="GET",status="200",le="0.025"} 180', // 30 more requests (180-150)
				'martin_http_requests_duration_seconds_bucket{endpoint="/sprite/{source_ids}.json",method="GET",status="200",le="+Inf"} 200',
				'martin_http_requests_duration_seconds_sum{endpoint="/sprite/{source_ids}.json",method="GET",status="200"} 2.0',
				'martin_http_requests_duration_seconds_count{endpoint="/sprite/{source_ids}.json",method="GET",status="200"} 200',
				// Sprite PNG endpoint - different distribution
				'martin_http_requests_duration_seconds_bucket{endpoint="/sprite/{source_ids}.png",method="GET",status="200",le="0.005"} 50',
				'martin_http_requests_duration_seconds_bucket{endpoint="/sprite/{source_ids}.png",method="GET",status="200",le="0.01"} 80', // 30 more requests (80-50)
				'martin_http_requests_duration_seconds_bucket{endpoint="/sprite/{source_ids}.png",method="GET",status="200",le="0.025"} 120', // 40 more requests (120-80)
				'martin_http_requests_duration_seconds_bucket{endpoint="/sprite/{source_ids}.png",method="GET",status="200",le="+Inf"} 150',
				'martin_http_requests_duration_seconds_sum{endpoint="/sprite/{source_ids}.png",method="GET",status="200"} 1.5',
				'martin_http_requests_duration_seconds_count{endpoint="/sprite/{source_ids}.png",method="GET",status="200"} 150',
			].join("\n");

			// Parse all metrics
			const { sum, count, histograms } =
				parseCompletePrometheusMetrics(sampleMetrics);

			// Verify individual endpoint parsing
			expect(histograms["/sprite/{source_ids}.json"]).toBeDefined();
			expect(histograms["/sprite/{source_ids}.png"]).toBeDefined();

			// Test histogram aggregation
			const endpointGroups = {
				sprites: ["/sprite/{source_ids}.json", "/sprite/{source_ids}.png"],
			};

			const aggregatedHistograms = aggregateHistogramGroups(
				histograms,
				endpointGroups,
			);
			expect(aggregatedHistograms.sprites).toBeDefined();

			// Verify aggregated buckets are cumulative sums
			const spritesHist = aggregatedHistograms.sprites;
			expect(spritesHist.buckets).toHaveLength(3);

			// le=0.005: 100 + 50 = 150
			expect(spritesHist.buckets[0]).toEqual({ le: 0.005, count: 150 });
			// le=0.01: 150 + 80 = 230
			expect(spritesHist.buckets[1]).toEqual({ le: 0.01, count: 230 });
			// le=0.025: 180 + 120 = 300
			expect(spritesHist.buckets[2]).toEqual({ le: 0.025, count: 300 });

			// Verify sum and count aggregation
			expect(spritesHist.sum).toBe(3.5); // 2.0 + 1.5
			expect(spritesHist.count).toBe(350); // 200 + 150

			// Test MiniHistogram visualization with aggregated data
			// This tests that bucket differences are calculated correctly:
			// Bucket diffs: 150-0=150, 230-150=80, 300-230=70
			// Max diff: 150, so heights: 150/150=100%, 80/150=53%, 70/150=47%
			const bucketDifferences = [];
			for (let i = 0; i < spritesHist.buckets.length; i++) {
				const bucket = spritesHist.buckets[i];
				const prevCount = i > 0 ? spritesHist.buckets[i - 1].count : 0;
				bucketDifferences.push(bucket.count - prevCount);
			}

			expect(bucketDifferences).toEqual([150, 80, 70]);
			const maxDiff = Math.max(...bucketDifferences);
			expect(maxDiff).toBe(150);

			// Verify percentile calculation works with aggregated data
			const percentiles = calculateHistogramPercentiles(spritesHist, [50, 95]);
			expect(percentiles.p50).toBeGreaterThan(0);
			expect(percentiles.p95).toBeGreaterThan(percentiles.p50);

			// Test traditional sum/count aggregation for comparison
			const aggregatedMetrics = aggregateEndpointGroups(
				sum,
				count,
				endpointGroups,
			);
			expect(aggregatedMetrics.sprites.requestCount).toBe(350);
			expect(aggregatedMetrics.sprites.averageRequestDurationMs).toBeCloseTo(
				10,
			); // (3.5/350)*1000
		});
	});
});
