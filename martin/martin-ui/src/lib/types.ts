import type { CacheMetrics, HistogramBucket } from './prometheus';
import type { components } from './types.gen';

type GeneratedCatalog = components['schemas']['Catalog'];

// The OpenAPI spec doesn't yet expose every field the dashboard renders, so
// each entry type is the spec-derived shape intersected with a small client
// patch carrying the still-missing optionals. Drop the patch field once the
// matching `utoipa::ToSchema` derive on the Rust side is enriched.

interface FontPatch {
  // todo: make this provided as required upstream
  format?: 'otf' | 'ttf' | 'ttc';
  // todo: make this provided as required upstream
  lastModifiedAt?: Date;
}

interface StylePatch {
  // todo: make this provided as required upstream
  type?: 'vector' | 'raster' | 'hybrid';
  // todo: make this provided as required upstream
  versionHash?: string;
  // todo: make this provided as required upstream
  layerCount?: number;
  // todo: make this provided as required upstream
  colors?: readonly string[];
  // todo: make this provided as required upstream
  lastModifiedAt?: Date;
}

interface TileSourcePatch {
  // todo: make this provided as required upstream
  layerCount?: number;
  // todo: make this provided as required upstream
  lastModifiedAt?: Date;
}

interface SpriteCollectionPatch {
  // todo: make this provided as required upstream
  sizeInBytes?: number;
  // todo: make this provided as required upstream
  lastModifiedAt?: Date;
}

export type Font = GeneratedCatalog['fonts'][string] & FontPatch;
export type Style = GeneratedCatalog['styles'][string] & StylePatch;
export type TileSource = GeneratedCatalog['tiles'][string] & TileSourcePatch;
export type SpriteCollection = GeneratedCatalog['sprites'][string] & SpriteCollectionPatch;

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
