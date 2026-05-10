import type { CacheMetrics, HistogramBucket } from './prometheus';
import type { SchemaCatalog } from './types.gen';

export type Catalog = SchemaCatalog;
export type Font = SchemaCatalog['fonts'][string];
export type Style = SchemaCatalog['styles'][string];
export type TileSource = SchemaCatalog['tiles'][string];
export type SpriteCollection = SchemaCatalog['sprites'][string];

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
