import { describe, expect, it } from 'vitest';
import {
  aggregateEndpointGroups,
  aggregateHistogramGroups,
  hitRate,
  parseCacheMetrics,
  parseCompletePrometheusMetrics,
  parsePrometheusHistogram,
  parsePrometheusMetrics,
} from '@/lib/prometheus';

describe('parsePrometheusMetrics', () => {
  it('parses sum and count for multiple endpoints', () => {
    const metrics = [
      '# HELP martin_http_requests_duration_seconds HTTP request duration in seconds for all requests',
      '# TYPE martin_http_requests_duration_seconds histogram',
      'martin_http_requests_duration_seconds_sum{endpoint="/sprite/{source_ids}.json",method="GET",status="200"} 12.5',
      'martin_http_requests_duration_seconds_count{endpoint="/sprite/{source_ids}.json",method="GET",status="200"} 50',
      'martin_http_requests_duration_seconds_sum{endpoint="/font/{fontstack}/{start}-{end}",method="GET",status="200"} 3.2',
      'martin_http_requests_duration_seconds_count{endpoint="/font/{fontstack}/{start}-{end}",method="GET",status="200"} 8',
      'martin_http_requests_duration_seconds_sum{endpoint="/style/{style_id}",method="GET",status="200"} 7.0',
      'martin_http_requests_duration_seconds_count{endpoint="/style/{style_id}",method="GET",status="200"} 14',
    ].join('\n');
    const { sum, count } = parsePrometheusMetrics(metrics);

    expect(sum['/sprite/{source_ids}.json']).toBe(12.5);
    expect(count['/sprite/{source_ids}.json']).toBe(50);

    expect(sum['/font/{fontstack}/{start}-{end}']).toBe(3.2);
    expect(count['/font/{fontstack}/{start}-{end}']).toBe(8);

    expect(sum['/style/{style_id}']).toBe(7.0);
    expect(count['/style/{style_id}']).toBe(14);
  });

  it('ignores unrelated metrics', () => {
    const metrics = [
      'other_metric{foo="bar"} 123',
      'martin_http_requests_duration_seconds_sum{endpoint="/catalog",method="GET",status="200"} 1.1',
      'martin_http_requests_duration_seconds_count{endpoint="/catalog",method="GET",status="200"} 2',
    ].join('\n');

    const { sum, count } = parsePrometheusMetrics(metrics);

    expect(sum['/catalog']).toBe(1.1);
    expect(count['/catalog']).toBe(2);
    expect(sum.foo).toBeUndefined();
    expect(count.foo).toBeUndefined();
  });

  it('handles missing sum or count values gracefully', () => {
    const metrics = [
      'martin_http_requests_duration_seconds_sum{endpoint="/sprite/{source_ids}.json",method="GET",status="200"} 5.5',
      '# No count for this endpoint',
      'martin_http_requests_duration_seconds_count{endpoint="/style/{style_id}",method="GET",status="200"} 10',
      '# No sum for this endpoint',
    ].join('\n');

    const { sum, count } = parsePrometheusMetrics(metrics);

    expect(sum['/sprite/{source_ids}.json']).toBe(5.5);
    expect(count['/sprite/{source_ids}.json']).toBeUndefined();

    expect(sum['/style/{style_id}']).toBeUndefined();
    expect(count['/style/{style_id}']).toBe(10);
  });

  describe('aggregateEndpointGroups', () => {
    it('aggregates metrics for defined endpoint groups', () => {
      const sum = {
        '/font/{fontstack}/{start}-{end}': 5,
        '/sprite/{source_ids}.json': 10,
        '/sprite/{source_ids}.png': 20,
        '/style/{style_id}': 8,
      };
      const count = {
        '/font/{fontstack}/{start}-{end}': 1,
        '/sprite/{source_ids}.json': 2,
        '/sprite/{source_ids}.png': 4,
        '/style/{style_id}': 8,
      };
      const endpointGroups = {
        fonts: ['/font/{fontstack}/{start}-{end}'],
        missing: ['/not_present'],
        sprites: ['/sprite/{source_ids}.json', '/sprite/{source_ids}.png'],
        styles: ['/style/{style_id}'],
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

    it('handles empty input gracefully', () => {
      const sum = {};
      const count = {};
      const endpointGroups = {
        group1: ['/foo'],
        group2: ['/bar'],
      };
      const result = aggregateEndpointGroups(sum, count, endpointGroups);
      expect(result.group1.averageRequestDurationMs).toBe(0);
      expect(result.group1.requestCount).toBe(0);
      expect(result.group2.averageRequestDurationMs).toBe(0);
      expect(result.group2.requestCount).toBe(0);
    });
  });

  describe('parsePrometheusHistogram', () => {
    it('parses histogram buckets for an endpoint', () => {
      const metrics = [
        '# HELP martin_http_requests_duration_seconds HTTP request duration in seconds for all requests',
        '# TYPE martin_http_requests_duration_seconds histogram',
        'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="0.005"} 1',
        'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="0.01"} 2',
        'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="0.025"} 4',
        'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="0.05"} 5',
        'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="0.1"} 5',
        'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="0.25"} 10',
        'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="0.5"} 15',
        'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="1"} 15',
        'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="2.5"} 20',
        'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="5"} 20',
        'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="10"} 20',
        'martin_http_requests_duration_seconds_bucket{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200",le="+Inf"} 20',
        'martin_http_requests_duration_seconds_sum{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200"} 20',
        'martin_http_requests_duration_seconds_count{endpoint="/{source_ids}/{z}/{x}/{y}",method="GET",status="200"} 20',
      ].join('\n');
      const histogram = parsePrometheusHistogram(metrics);
      const tileEndpoint = '/{source_ids}/{z}/{x}/{y}';

      expect(histogram[tileEndpoint]).toBeDefined();
      expect(histogram[tileEndpoint]).toHaveLength(11); // All buckets except +Inf

      const expectedCounts = [1, 2, 4, 5, 5, 10, 15, 15, 20, 20, 20];
      expect(histogram[tileEndpoint].map((bucket) => bucket.count)).toEqual(expectedCounts);
      const expectedLe = [0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1, 2.5, 5, 10];
      expect(histogram[tileEndpoint].map((bucket) => bucket.le)).toEqual(expectedLe);
    });

    it('handles multiple endpoints with histograms', () => {
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
      ].join('\n');

      const histogram = parsePrometheusHistogram(metrics);

      expect(Object.keys(histogram)).toHaveLength(2);
      expect(histogram['/sprite/{source_ids}.json']).toHaveLength(2);
      expect(histogram['/style/{style_id}']).toHaveLength(2);
    });

    it('ignores non-histogram metrics', () => {
      const metrics = [
        'other_metric{foo="bar"} 123',
        'martin_http_requests_duration_seconds_bucket{endpoint="/test",method="GET",status="200",le="0.1"} 50',
        'martin_http_requests_duration_seconds_sum{endpoint="/test",method="GET",status="200"} 1.0',
        'martin_http_requests_duration_seconds_count{endpoint="/test",method="GET",status="200"} 50',
        'unrelated_bucket{le="0.1"} 999',
      ].join('\n');

      const histogram = parsePrometheusHistogram(metrics);

      expect(Object.keys(histogram)).toHaveLength(1);
      expect(histogram['/test']).toBeDefined();
      expect(histogram.other_metric).toBeUndefined();
    });

    it('handles missing sum or count gracefully', () => {
      const metrics = [
        'martin_http_requests_duration_seconds_bucket{endpoint="/incomplete",method="GET",status="200",le="0.1"} 50',
        'martin_http_requests_duration_seconds_sum{endpoint="/incomplete",method="GET",status="200"} 1.0',
        // Missing count
      ].join('\n');

      const histogram = parsePrometheusHistogram(metrics);

      expect(histogram['/incomplete']).toBeDefined();
      expect(histogram['/incomplete']).toHaveLength(1);
    });
  });

  describe('Real world histogram data', () => {
    const sampleMetrics = [
      '# HELP martin_http_requests_duration_seconds HTTP request duration in seconds for all requests',
      '# TYPE martin_http_requests_duration_seconds histogram',
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
    ].join('\n');
    it('parses the provided sample histogram data correctly', () => {
      const histogram = parsePrometheusHistogram(sampleMetrics);
      const tileEndpoint = '/{source_ids}/{z}/{x}/{y}';

      expect(histogram[tileEndpoint]).toBeDefined();
      const expectedCounts = [
        23004, 23045, 23228, 23410, 23637, 23722, 23735, 23746, 23747, 23747, 23747,
      ];
      expect(histogram[tileEndpoint].map((hist) => hist.count)).toEqual(expectedCounts);
      const expectedLe = [0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1, 2.5, 5, 10];
      expect(histogram[tileEndpoint].map((hist) => hist.le)).toEqual(expectedLe);
    });

    describe('parseCompletePrometheusMetrics', () => {
      it('parses all metrics types (sum, count, histograms) in one call', () => {
        const result = parseCompletePrometheusMetrics(sampleMetrics);

        // Check sum and count are parsed
        expect(result.sum['/{source_ids}/{z}/{x}/{y}']).toBe(61.49839745299979);
        expect(result.count['/{source_ids}/{z}/{x}/{y}']).toBe(23747);

        // Check histogram is parsed
        expect(result.histograms).toEqual(parsePrometheusHistogram(sampleMetrics));
      });
    });

    describe('aggregateHistogramGroups', () => {
      it('aggregates multiple endpoints into a single group histogram', () => {
        const histograms = {
          '/sprite/{source_ids}.json': [
            { count: 100, le: 0.005 },
            { count: 150, le: 0.01 },
            { count: 180, le: 0.025 },
          ],
          '/sprite/{source_ids}.png': [
            { count: 50, le: 0.005 },
            { count: 80, le: 0.01 },
            { count: 120, le: 0.025 },
          ],
        };

        const endpointGroups = {
          sprites: ['/sprite/{source_ids}.json', '/sprite/{source_ids}.png'],
          tiles: ['/{source_ids}/{z}/{x}/{y}'],
        };

        const result = aggregateHistogramGroups(histograms, endpointGroups);

        expect(result.sprites).toBeDefined();
        expect(result.sprites).toHaveLength(3);

        // Buckets should be aggregated: 100+50=150, 150+80=230, 180+120=300
        expect(result.sprites[0]).toEqual({ count: 150, le: 0.005 });
        expect(result.sprites[1]).toEqual({ count: 230, le: 0.01 });
        expect(result.sprites[2]).toEqual({ count: 300, le: 0.025 });

        // Tiles group should exist but be empty since no histogram data
        expect(result.tiles).toBeDefined();
        expect(result.tiles).toHaveLength(0);
      });

      it('handles different bucket boundaries across endpoints', () => {
        const histograms = {
          '/sprite/{source_ids}.json': [
            { count: 100, le: 0.005 },
            { count: 150, le: 0.025 },
          ],
          '/sprite/{source_ids}.png': [
            { count: 50, le: 0.01 },
            { count: 80, le: 0.05 },
          ],
        };

        const endpointGroups = {
          sprites: ['/sprite/{source_ids}.json', '/sprite/{source_ids}.png'],
        };

        const result = aggregateHistogramGroups(histograms, endpointGroups);

        expect(result.sprites).toBeDefined();
        // Should have 4 unique bucket boundaries: 0.005, 0.01, 0.025, 0.05
        expect(result.sprites).toHaveLength(4);

        // Check simple aggregation - only combines buckets with same le values
        expect(result.sprites[0]).toEqual({ count: 100, le: 0.005 }); // Only from json
        expect(result.sprites[1]).toEqual({ count: 50, le: 0.01 }); // Only from png
        expect(result.sprites[2]).toEqual({ count: 150, le: 0.025 }); // Only from json
        expect(result.sprites[3]).toEqual({ count: 80, le: 0.05 }); // Only from png
      });

      it('handles single endpoint in group', () => {
        const histograms = {
          '/{source_ids}/{z}/{x}/{y}': [
            { count: 1000, le: 0.005 },
            { count: 1200, le: 0.01 },
          ],
        };

        const endpointGroups = {
          tiles: ['/{source_ids}/{z}/{x}/{y}'],
        };

        const result = aggregateHistogramGroups(histograms, endpointGroups);

        expect(result.tiles).toBeDefined();
        expect(result.tiles).toHaveLength(2);
        expect(result.tiles[0]).toEqual({ count: 1000, le: 0.005 });
        expect(result.tiles[1]).toEqual({ count: 1200, le: 0.01 });
      });

      it('returns empty result when no histogram data available', () => {
        const histograms = {};
        const endpointGroups = {
          sprites: ['/sprite/{source_ids}.json'],
          tiles: ['/{source_ids}/{z}/{x}/{y}'],
        };

        const result = aggregateHistogramGroups(histograms, endpointGroups);

        expect(Object.keys(result)).toHaveLength(2);
        expect(result.sprites).toBeDefined();
        expect(result.sprites).toHaveLength(0);
        expect(result.tiles).toBeDefined();
        expect(result.tiles).toHaveLength(0);
      });

      it('handles partial histogram data (some endpoints missing)', () => {
        const histograms = {
          '/sprite/{source_ids}.json': [{ count: 100, le: 0.01 }],
          // "/sprite/{source_ids}.png" is missing
        };

        const endpointGroups = {
          sprites: ['/sprite/{source_ids}.json', '/sprite/{source_ids}.png'],
        };

        const result = aggregateHistogramGroups(histograms, endpointGroups);

        expect(result.sprites).toBeDefined();
        expect(result.sprites).toHaveLength(1);
        expect(result.sprites[0]).toEqual({ count: 100, le: 0.01 });
      });
    });
  });

  describe('Integration test: Corrected histogram processing', () => {
    it('properly handles cumulative histogram buckets and multi-endpoint aggregation', () => {
      // Sample data with multiple sprite endpoints showing cumulative histogram nature
      const sampleMetrics = [
        '# HELP martin_http_requests_duration_seconds HTTP request duration in seconds for all requests',
        '# TYPE martin_http_requests_duration_seconds histogram',
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
      ].join('\n');

      // Parse all metrics
      const { sum, count, histograms } = parseCompletePrometheusMetrics(sampleMetrics);

      // Verify individual endpoint parsing
      expect(histograms['/sprite/{source_ids}.json']).toBeDefined();
      expect(histograms['/sprite/{source_ids}.png']).toBeDefined();

      // Test histogram aggregation
      const endpointGroups = {
        sprites: ['/sprite/{source_ids}.json', '/sprite/{source_ids}.png'],
      };

      const aggregatedHistograms = aggregateHistogramGroups(histograms, endpointGroups);
      expect(aggregatedHistograms.sprites).toBeDefined();

      // Verify aggregated buckets are cumulative sums
      const spritesHist = aggregatedHistograms.sprites;
      expect(spritesHist).toHaveLength(3);

      // le=0.005: 100 + 50 = 150
      expect(spritesHist[0]).toEqual({ count: 150, le: 0.005 });
      // le=0.01: 150 + 80 = 230
      expect(spritesHist[1]).toEqual({ count: 230, le: 0.01 });
      // le=0.025: 180 + 120 = 300
      expect(spritesHist[2]).toEqual({ count: 300, le: 0.025 });

      // Test MiniHistogram visualization with aggregated data
      // This tests that bucket differences are calculated correctly:
      // Bucket diffs: 150-0=150, 230-150=80, 300-230=70
      // Max diff: 150, so heights: 150/150=100%, 80/150=53%, 70/150=47%
      const bucketDifferences = [];
      for (let i = 0; i < spritesHist.length; i++) {
        const bucket = spritesHist[i];
        const prevCount = i > 0 ? spritesHist[i - 1].count : 0;
        bucketDifferences.push(bucket.count - prevCount);
      }

      expect(bucketDifferences).toEqual([150, 80, 70]);
      const maxDiff = Math.max(...bucketDifferences);
      expect(maxDiff).toBe(150);

      // Test traditional sum/count aggregation for comparison
      const aggregatedMetrics = aggregateEndpointGroups(sum, count, endpointGroups);
      expect(aggregatedMetrics.sprites.requestCount).toBe(350);
      expect(aggregatedMetrics.sprites.averageRequestDurationMs).toBeCloseTo(10); // (3.5/350)*1000
    });
  });
});

describe('parseCacheMetrics', () => {
  it('parses zoom-less caches from martin_cache_requests_total', () => {
    const text = [
      '# HELP martin_cache_requests_total Martin cache lookups',
      '# TYPE martin_cache_requests_total counter',
      'martin_cache_requests_total{cache="font",result="hit"} 5',
      'martin_cache_requests_total{cache="font",result="miss"} 5',
      'martin_cache_requests_total{cache="sprite",result="hit"} 9',
      'martin_cache_requests_total{cache="sprite",result="miss"} 1',
    ].join('\n');

    const result = parseCacheMetrics(text);

    expect(result.font).toEqual({ byZoom: [], hits: 5, misses: 5 });
    expect(result.sprite).toEqual({ byZoom: [], hits: 9, misses: 1 });
    expect(hitRate(result.font)).toBe(0.5);
    expect(hitRate(result.sprite)).toBe(0.9);
  });

  it('parses tile-coordinate caches from martin_tile_cache_requests_total with per-zoom breakdown', () => {
    const text = [
      '# HELP martin_tile_cache_requests_total Tile-coord cache lookups',
      '# TYPE martin_tile_cache_requests_total counter',
      'martin_tile_cache_requests_total{cache="tile",result="hit",zoom="0"} 50',
      'martin_tile_cache_requests_total{cache="tile",result="miss",zoom="0"} 0',
      'martin_tile_cache_requests_total{cache="tile",result="hit",zoom="5"} 30',
      'martin_tile_cache_requests_total{cache="tile",result="miss",zoom="5"} 20',
      'martin_tile_cache_requests_total{cache="pmtiles_directory",result="hit",zoom="0"} 10',
      'martin_tile_cache_requests_total{cache="pmtiles_directory",result="miss",zoom="0"} 0',
    ].join('\n');

    const result = parseCacheMetrics(text);

    expect(result.tile).toEqual({
      byZoom: [
        { hits: 50, misses: 0, zoom: 0 },
        { hits: 30, misses: 20, zoom: 5 },
      ],
      hits: 80,
      misses: 20,
    });
    expect(hitRate(result.tile)).toBe(0.8);
    expect(hitRate(result.tile.byZoom[1])).toBe(0.6);
    expect(result.pmtiles_directory).toEqual({
      byZoom: [{ hits: 10, misses: 0, zoom: 0 }],
      hits: 10,
      misses: 0,
    });
  });

  it('hitRate returns null for caches with no recorded requests', () => {
    const text = [
      'martin_cache_requests_total{cache="sprite",result="hit"} 0',
      'martin_cache_requests_total{cache="sprite",result="miss"} 0',
    ].join('\n');

    const sprite = parseCacheMetrics(text).sprite;
    expect(sprite).toEqual({ byZoom: [], hits: 0, misses: 0 });
    expect(hitRate(sprite)).toBeNull();
  });

  it('sorts byZoom by zoom level regardless of input order', () => {
    const text = [
      'martin_tile_cache_requests_total{cache="tile",result="hit",zoom="14"} 1',
      'martin_tile_cache_requests_total{cache="tile",result="hit",zoom="2"} 1',
      'martin_tile_cache_requests_total{cache="tile",result="hit",zoom="0"} 1',
    ].join('\n');

    const zooms = parseCacheMetrics(text).tile.byZoom.map((b) => b.zoom);
    expect(zooms).toEqual([0, 2, 14]);
  });

  it('omits caches that do not appear in the input', () => {
    const text = 'martin_tile_cache_requests_total{cache="tile",result="hit",zoom="3"} 1';
    const result = parseCacheMetrics(text);
    expect(Object.keys(result)).toEqual(['tile']);
    expect(result.tile.hits).toBe(1);
    expect(result.tile.byZoom).toEqual([{ hits: 1, misses: 0, zoom: 3 }]);
  });

  it('ignores unrelated metrics, malformed lines, and unknown result labels', () => {
    const text = [
      'other_metric{cache="tile",result="hit"} 999',
      'martin_tile_cache_requests_total{cache="tile",result="other",zoom="0"} 42',
      'martin_tile_cache_requests_total{cache="tile",result="hit",zoom="0"} 7',
      'martin_tile_cache_requests_total{cache="tile",result="miss",zoom="0"} 3',
      'martin_tile_cache_requests_total{cache="tile",result="hit",zoom="0"} not-a-number',
    ].join('\n');

    const tile = parseCacheMetrics(text).tile;
    expect(tile.hits).toBe(7);
    expect(tile.misses).toBe(3);
    expect(hitRate(tile)).toBe(0.7);
  });

  it('handles empty input', () => {
    expect(parseCacheMetrics('')).toEqual({});
  });

  it('sums duplicated label combinations (e.g. across workers)', () => {
    const text = [
      'martin_cache_requests_total{cache="font",result="hit"} 4',
      'martin_cache_requests_total{cache="font",result="hit"} 6',
      'martin_cache_requests_total{cache="font",result="miss"} 10',
    ].join('\n');
    expect(parseCacheMetrics(text).font).toEqual({ byZoom: [], hits: 10, misses: 10 });
  });
});
