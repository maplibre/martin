import type { CacheMetrics, HistogramBucket } from './prometheus';

export type { Catalog } from './types.gen';

export interface EndpointAnalytics {
  averageRequestDurationMs: number;
  requestCount: number;
  histogram: HistogramBucket[];
}

export interface AnalyticsData {
  sprites: EndpointAnalytics;
  tiles: EndpointAnalytics;
  fonts: EndpointAnalytics;
  styles: EndpointAnalytics;
  caches: Record<string, CacheMetrics>;
}
