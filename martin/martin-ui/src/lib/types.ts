import type { CacheMetrics, HistogramBucket } from './prometheus';
import type { Catalog } from './types.gen';

export type { Catalog };
export type Font = Catalog['fonts'][string];
export type Style = Catalog['styles'][string];
export type TileSource = Catalog['tiles'][string];
export type SpriteCollection = Catalog['sprites'][string];

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
