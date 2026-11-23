import {
  type FilterSpecification,
  featureFilter,
  type LayerSpecification,
  type StyleSpecification,
} from '@maplibre/maplibre-gl-style-spec';
import type { MapGeoJSONFeature } from 'maplibre-gl';
import {
  describeCondition,
  type FilterCondition,
  GEOMETRY_TYPE,
  parseFilter,
} from '../inspector/filter-model';

type Geometry = MapGeoJSONFeature['geometry'];

/** Geometry family of a feature, the way filters see it in `$type`. */
export type GeometryKind = 'Point' | 'LineString' | 'Polygon';

/** Scalar value of a feature property. */
export type PropertyValue = string | number | boolean | null;

/** A source feature under the cursor, independent of which style layer drew it. */
export interface InspectedFeature {
  /** Groups the same feature across the layers and tiles that return it. */
  key: string;
  /** Style source id. */
  source: string;
  /** Source layer inside a vector source. */
  sourceLayer?: string;
  /** Feature id, when the tile has one. */
  id?: string | number;
  /** Geometry family. */
  kind: GeometryKind;
  /** Raw properties as stored in the tile. */
  properties: Readonly<Record<string, PropertyValue>>;
  /** Geometry, used to outline the feature on the map. */
  geometry: Geometry;
}

/** Why a style layer does or does not draw a feature. */
export type LayerVerdict =
  | { status: 'drawn' }
  | { status: 'hidden' }
  | { status: 'zoom'; minzoom: number; maxzoom: number }
  | { status: 'filter'; failing?: string }
  | { status: 'geometry' }
  | { status: 'unknown'; reason: string };

/** A style layer reading the feature's data, with its verdict. */
export interface LayerMatch {
  /** Style layer id. */
  id: string;
  /** Layer type. */
  type: LayerSpecification['type'];
  /** Whether and why the layer draws the feature. */
  verdict: LayerVerdict;
}

/** One top-level condition of a filter and how it evaluates against a feature. */
export interface ClauseResult {
  /** The condition as written in the style. */
  clause: unknown;
  /** The condition as a short sentence, or its JSON when it has no simple reading. */
  label: string;
  /** Whether it passes; `null` when it cannot be evaluated without the full geometry. */
  pass: boolean | null;
  /** Property names the condition reads. */
  keys: string[];
  /** Set when the condition fails because the property has another type than the value it is compared with. */
  typeMismatch?: TypeMismatch;
}

/** A property compared with a value of another type, e.g. `1` against `true`; MapLibre never treats these as equal. */
export interface TypeMismatch {
  /** Property name. */
  key: string;
  /** Type of the property in the feature. */
  actual: string;
  /** Type of the value the filter compares with. */
  expected: string;
}

const GEOMETRY_CODE = { LineString: 2, Point: 1, Polygon: 3 } as const satisfies Record<
  GeometryKind,
  number
>;

const LEGACY_OPS = new Set(['==', '!=', '<', '<=', '>', '>=', 'in', '!in', 'has', '!has']);

/** Maps a GeoJSON geometry type to the family filters compare against. */
export function geometryKind(type: Geometry['type']): GeometryKind {
  switch (type) {
    case 'Point':
    case 'MultiPoint':
      return 'Point';
    case 'LineString':
    case 'MultiLineString':
      return 'LineString';
    default:
      return 'Polygon';
  }
}

/** Collapses the copies MapLibre returns per layer into one entry per feature; features without an id are only merged when their properties and geometry are equal. */
export function dedupeFeatures(
  features: readonly Omit<InspectedFeature, 'key'>[],
): InspectedFeature[] {
  const seen = new Map<string, InspectedFeature>();
  for (const f of features) {
    const identity = f.id === undefined ? JSON.stringify([f.properties, f.geometry]) : String(f.id);
    const key = `${f.source}/${f.sourceLayer ?? ''}/${f.kind}/${identity}`;
    if (!seen.has(key)) seen.set(key, { ...f, key });
  }
  return [...seen.values()];
}

function evaluate(filter: unknown, feature: InspectedFeature, zoom: number): boolean | null {
  const compiled = featureFilter(filter as FilterSpecification, 'filter');
  if (compiled.needGeometry) return null;
  return compiled.filter(
    { zoom: Math.floor(zoom) },
    {
      geometry: [],
      id: feature.id ?? null,
      properties: { ...feature.properties },
      type: GEOMETRY_CODE[feature.kind],
    },
  );
}

const DRAWS: Partial<Record<LayerSpecification['type'], readonly GeometryKind[]>> = {
  fill: ['Polygon'],
  'fill-extrusion': ['Polygon'],
  line: ['LineString', 'Polygon'],
};

function verdict(layer: LayerSpecification, feature: InspectedFeature, zoom: number): LayerVerdict {
  if (layer.layout?.visibility === 'none') return { status: 'hidden' };
  const minzoom = layer.minzoom ?? 0;
  const maxzoom = layer.maxzoom ?? 24;
  if (zoom < minzoom || zoom >= maxzoom) return { maxzoom, minzoom, status: 'zoom' };
  const draws = DRAWS[layer.type];
  if (draws && !draws.includes(feature.kind)) return { status: 'geometry' };
  if (!('filter' in layer) || layer.filter === undefined) return { status: 'drawn' };
  try {
    const pass = evaluate(layer.filter, feature, zoom);
    if (pass === null) return { reason: 'needs the full geometry', status: 'unknown' };
    if (pass) return { status: 'drawn' };
    const failing = filterClauses(layer, feature, zoom).find((c) => c.pass === false)?.label;
    return failing ? { failing, status: 'filter' } : { status: 'filter' };
  } catch (e) {
    return { reason: e instanceof Error ? e.message : String(e), status: 'unknown' };
  }
}

/** Every style layer reading the feature's source and source layer, topmost first, with whether it draws it. */
export function layerMatches(
  style: StyleSpecification,
  feature: InspectedFeature,
  zoom: number,
): LayerMatch[] {
  return style.layers
    .filter(
      (l) =>
        'source' in l &&
        l.source === feature.source &&
        ('source-layer' in l ? l['source-layer'] : undefined) === feature.sourceLayer,
    )
    .map((l) => ({ id: l.id, type: l.type, verdict: verdict(l, feature, zoom) }))
    .reverse();
}

/** Property names a filter condition reads, through `get`/`has` expressions and legacy `[op, key, …]` filters. */
export function filterKeys(clause: unknown): string[] {
  const keys = new Set<string>();
  const walk = (node: unknown, top: boolean) => {
    if (!Array.isArray(node)) return;
    const [op, first] = node;
    if ((op === 'get' || op === 'has') && typeof first === 'string' && node.length === 2) {
      keys.add(first);
      return;
    }
    if (top && typeof op === 'string' && LEGACY_OPS.has(op) && typeof first === 'string') {
      if (!first.startsWith('$')) keys.add(first);
      return;
    }
    const legacyAny = top && (op === 'all' || op === 'any' || op === 'none');
    for (const child of node.slice(1)) walk(child, legacyAny);
  };
  walk(clause, true);
  return [...keys];
}

function singleCondition(clause: unknown): FilterCondition | undefined {
  const model = parseFilter(clause);
  return model.mode === 'structured' && model.conditions.length === 1
    ? model.conditions[0]
    : undefined;
}

function typeMismatch(
  condition: FilterCondition | undefined,
  feature: InspectedFeature,
): TypeMismatch | undefined {
  if (
    !condition ||
    condition.attribute === GEOMETRY_TYPE ||
    !(condition.attribute in feature.properties)
  ) {
    return undefined;
  }
  const compared =
    'value' in condition ? [condition.value] : 'values' in condition ? condition.values : [];
  if (compared.length === 0) return undefined;
  const value = feature.properties[condition.attribute];
  const actual = value === null ? 'null' : typeof value;
  const expected = typeof compared[0];
  if (compared.some((v) => typeof v === actual)) return undefined;
  return { actual, expected, key: condition.attribute };
}

/** Splits a layer filter into its top-level `all` conditions and evaluates each against the feature. */
export function filterClauses(
  layer: LayerSpecification,
  feature: InspectedFeature,
  zoom: number,
): ClauseResult[] {
  if (!('filter' in layer) || layer.filter === undefined) return [];
  const filter: unknown = layer.filter;
  const clauses = Array.isArray(filter) && filter[0] === 'all' ? filter.slice(1) : [filter];
  return clauses.map((clause) => {
    let pass: boolean | null;
    try {
      pass = evaluate(clause, feature, zoom);
    } catch {
      pass = null;
    }
    const condition = singleCondition(clause);
    const mismatch = pass === false ? typeMismatch(condition, feature) : undefined;
    return {
      clause,
      keys: filterKeys(clause),
      label: condition ? describeCondition(condition) : JSON.stringify(clause),
      pass,
      ...(mismatch ? { typeMismatch: mismatch } : {}),
    };
  });
}

/** Prefix of the layers and source {@link withOutline} adds. */
export const OUTLINE_PREFIX = 'inspect:';

/** Returns `style` with `geometry` outlined on top in `accent`. */
export function withOutline(
  style: StyleSpecification,
  geometry: Geometry,
  accent: string,
): StyleSpecification {
  const source = `${OUTLINE_PREFIX}feature`;
  return {
    ...style,
    layers: [
      ...style.layers,
      {
        filter: ['match', ['geometry-type'], ['Polygon', 'MultiPolygon'], true, false],
        id: `${OUTLINE_PREFIX}fill`,
        paint: { 'fill-color': accent, 'fill-opacity': 0.2 },
        source,
        type: 'fill',
      },
      {
        id: `${OUTLINE_PREFIX}line`,
        paint: { 'line-color': accent, 'line-width': 2.5 },
        source,
        type: 'line',
      },
      {
        filter: ['match', ['geometry-type'], ['Point', 'MultiPoint'], true, false],
        id: `${OUTLINE_PREFIX}point`,
        paint: {
          'circle-color': accent,
          'circle-radius': 6,
          'circle-stroke-color': '#fff',
          'circle-stroke-width': 2,
        },
        source,
        type: 'circle',
      },
    ],
    sources: { ...style.sources, [source]: { data: geometry, type: 'geojson' } },
  };
}
