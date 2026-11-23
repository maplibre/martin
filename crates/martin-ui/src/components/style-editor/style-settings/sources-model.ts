import { latest, type SourceSpecification } from '@maplibre/maplibre-gl-style-spec';
import type { PropertySpec } from '../style-spec';

/** Source types the Add source form can create. */
export const CUSTOM_SOURCE_TYPES = ['vector', 'raster', 'raster-dem', 'geojson'] as const;

/** A source type the Add source form can create. */
export type CustomSourceType = (typeof CUSTOM_SOURCE_TYPES)[number];

/** One editable field of a source type with its v8 definition. */
export interface SourceField {
  /** Field name as written in the style. */
  name: string;
  /** Its v8 definition. */
  spec: PropertySpec;
}

/** The editable fields of a source type in spec order, without `type`. */
export function sourceFields(type: string): SourceField[] {
  const table = latest[`source_${type.replace('-', '_')}` as keyof typeof latest] as
    | Record<string, PropertySpec>
    | undefined;
  return Object.entries(table ?? {})
    .filter(([name]) => name !== 'type' && name !== '*')
    .map(([name, spec]) => ({ name, spec }));
}

/** A source as a record of its fields. */
export function sourceRecord(source: SourceSpecification): Record<string, unknown> {
  return source as unknown as Record<string, unknown>;
}

/** Where a source gets its data: its URL, or "inline tiles" or "inline data". */
export function sourceSummary(source: SourceSpecification): string {
  const fields = sourceRecord(source);
  if (typeof fields.url === 'string') return fields.url;
  if (typeof fields.data === 'string') return fields.data;
  if (Array.isArray(fields.urls) && typeof fields.urls[0] === 'string') return fields.urls[0];
  if (Array.isArray(fields.tiles)) return 'inline tiles';
  return source.type === 'geojson' ? 'inline data' : 'no data';
}

/** The definition a new custom source gets for its type and URL. */
export function customSource(type: CustomSourceType, url: string): SourceSpecification {
  switch (type) {
    case 'geojson':
      return { data: url, type };
    case 'raster':
      return { tileSize: 256, type, url };
    case 'raster-dem':
      return { type, url };
    case 'vector':
      return { type, url };
  }
}
