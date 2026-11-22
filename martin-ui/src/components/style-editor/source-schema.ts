import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import type { Map as MapLibre } from 'maplibre-gl';
import { useEffect, useRef, useState } from 'react';

/** What the editor knows about one feature attribute of a source layer. */
export interface AttributeInfo {
  /** Attribute name as used in `['get', name]`. */
  name: string;
  /** Description from the TileJSON `vector_layers` entry, if any. */
  description?: string;
  /** Observed value type; `unknown` when only the TileJSON names it. */
  kind: 'string' | 'number' | 'boolean' | 'mixed' | 'unknown';
  /** Distinct observed values, most frequent first, capped at 100. */
  values: (string | number | boolean)[];
  /** Observed numeric range, when numeric. */
  range?: [number, number];
}

/** Looks up attributes of the features a layer draws. */
export interface SourceSchema {
  /** Attributes of `sourceLayer` in `source`, sorted by name; empty when nothing is known yet. */
  attributes: (source: string, sourceLayer: string | undefined) => AttributeInfo[];
  /** Source layers of a vector source from its TileJSON plus those seen in loaded tiles, sorted. */
  sourceLayers: (source: string) => string[];
  /** Grows whenever something new is learned; a memo dependency for values derived from this schema. */
  version: number;
}

interface Sample {
  description?: string;
  counts: Map<string | number | boolean, number>;
  types: Set<string>;
  min: number;
  max: number;
}

/** Samples per source, then per source layer (`''` for GeoJSON), then per attribute. */
type Store = Map<string, Map<string, Map<string, Sample>>>;

const MAX_DISTINCT = 500;
const MAX_FEATURES = 3000;

function sampleFor(store: Store, source: string, sourceLayer: string, attr: string): Sample {
  const layers = store.get(source) ?? new Map<string, Map<string, Sample>>();
  store.set(source, layers);
  const attrs = layers.get(sourceLayer) ?? new Map<string, Sample>();
  layers.set(sourceLayer, attrs);
  let s = attrs.get(attr);
  if (!s) {
    s = {
      counts: new Map(),
      max: Number.NEGATIVE_INFINITY,
      min: Number.POSITIVE_INFINITY,
      types: new Set(),
    };
    attrs.set(attr, s);
  }
  return s;
}

/** Collects attribute names from TileJSON and observed values from loaded tiles, growing as the map is panned. */
export function useSourceSchema(
  style: StyleSpecification | null,
  map: MapLibre | null,
): SourceSchema {
  const store = useRef<Store>(new Map());
  const layerIds = useRef<Map<string, Set<string>>>(new Map());
  const definitions = useRef<Map<string, string>>(new Map());
  const [version, setVersion] = useState(0);

  useEffect(() => {
    if (!style) return;
    const current = new Map(
      Object.entries(style.sources).map(([id, s]) => [id, JSON.stringify(s)]),
    );
    for (const [id, definition] of definitions.current) {
      if (current.get(id) !== definition) {
        store.current.delete(id);
        layerIds.current.delete(id);
      }
    }
    definitions.current = current;
    const controller = new AbortController();
    for (const [id, source] of Object.entries(style.sources)) {
      if (source.type !== 'vector' || !source.url) continue;
      fetch(source.url, { signal: controller.signal })
        .then((r) => (r.ok ? r.json() : undefined))
        .then(
          (tilejson?: { vector_layers?: { id: string; fields?: Record<string, string> }[] }) => {
            for (const vl of tilejson?.vector_layers ?? []) {
              layerIds.current.set(id, (layerIds.current.get(id) ?? new Set()).add(vl.id));
              for (const [attr, description] of Object.entries(vl.fields ?? {})) {
                sampleFor(store.current, id, vl.id, attr).description = description;
              }
            }
            setVersion((v) => v + 1);
          },
        )
        .catch(() => undefined);
    }
    setVersion((v) => v + 1);
    return () => controller.abort();
  }, [style?.sources]);

  useEffect(() => {
    if (!map || !style) return;
    const targets = new Map<string, { source: string; sourceLayer?: string }>();
    for (const l of style.layers) {
      if (!('source' in l) || typeof l.source !== 'string') continue;
      const sourceLayer = 'source-layer' in l ? l['source-layer'] : undefined;
      const type = style.sources[l.source]?.type;
      if (sourceLayer || type === 'geojson') {
        targets.set(JSON.stringify([l.source, sourceLayer ?? '']), {
          source: l.source,
          sourceLayer,
        });
      }
    }
    const collect = () => {
      for (const { source, sourceLayer } of targets.values()) {
        if (!map.getSource(source)) continue;
        const features = map
          .querySourceFeatures(source, sourceLayer ? { sourceLayer } : undefined)
          .slice(0, MAX_FEATURES);
        for (const f of features) {
          for (const [attr, value] of Object.entries(f.properties ?? {})) {
            const s = sampleFor(store.current, source, sourceLayer ?? '', attr);
            s.types.add(typeof value);
            if (s.counts.size < MAX_DISTINCT || s.counts.has(value))
              s.counts.set(value, (s.counts.get(value) ?? 0) + 1);
            if (typeof value === 'number') {
              s.min = Math.min(s.min, value);
              s.max = Math.max(s.max, value);
            }
          }
        }
      }
      setVersion((v) => v + 1);
    };
    map.on('idle', collect);
    return () => {
      map.off('idle', collect);
    };
  }, [map, style?.layers]);

  return {
    attributes: (source, sourceLayer) => {
      void version;
      const layer = store.current.get(source)?.get(sourceLayer ?? '');
      if (!layer) return [];
      return [...layer.entries()]
        .map(([name, s]): AttributeInfo => {
          const types = [...s.types].filter((t) => ['string', 'number', 'boolean'].includes(t));
          const kind =
            types.length === 0
              ? 'unknown'
              : types.length > 1
                ? 'mixed'
                : (types[0] as AttributeInfo['kind']);
          const values = [...s.counts.entries()]
            .sort((a, b) => b[1] - a[1])
            .slice(0, 100)
            .map(([v]) => v);
          const range: [number, number] | undefined = Number.isFinite(s.min)
            ? [s.min, s.max]
            : undefined;
          return { description: s.description, kind, name, range, values };
        })
        .sort((a, b) => a.name.localeCompare(b.name));
    },
    sourceLayers: (source) => {
      void version;
      const names = new Set(layerIds.current.get(source));
      for (const sourceLayer of store.current.get(source)?.keys() ?? []) {
        if (sourceLayer) names.add(sourceLayer);
      }
      return [...names].sort();
    },
    version,
  };
}
