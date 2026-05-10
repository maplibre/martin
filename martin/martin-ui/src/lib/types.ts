import type { CacheMetrics, HistogramBucket } from './prometheus';
import type { components } from './types.gen';

type GeneratedCatalog = components['schemas']['Catalog'];

export type Font = GeneratedCatalog['fonts'][string];
export type Style = GeneratedCatalog['styles'][string];
export type TileSource = GeneratedCatalog['tiles'][string];
export type SpriteCollection = GeneratedCatalog['sprites'][string];

export interface CatalogSchema {
  tiles: { readonly [tile_id: string]: TileSource };
  sprites: { readonly [sprite_collection_id: string]: SpriteCollection };
  fonts: { readonly [name: string]: Font };
  styles: { readonly [name: string]: Style };
}

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
