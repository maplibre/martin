import { describe, expect, it } from "@jest/globals";

// Import the functions from the correct location
import { aggregateEndpointGroups, parsePrometheusMetrics } from "@/lib/prometheus";

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
    expect(sum["foo"]).toBeUndefined();
    expect(count["foo"]).toBeUndefined();
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
});
